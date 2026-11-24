# 互換性の不足を埋める — ワークフロー

[English](compat-workflow.md) | 日本語

aexlo が動かせるプラグインを増やすための手順です。
1周ごとに、[`aexlo compat`](compat.ja.md) が報告する不足を1つ埋め、悪化がないことを確かめてコミットします。

```text
[0] ベースラインを保存
 └▶[1] 不足を選ぶ ─▶[2] 再現・調査 ─▶[3] 実装 ─▶[4] テスト ─▶[5] 比較 ─▶[6] コミット
        ▲                                                                    │
        └────────────────── ベースラインを保存し直し、次の不足へ ◀──────────┘
```

- [0. ベースラインを保存する](#0-ベースラインを保存する)
- [1. 不足を選ぶ](#1-不足を選ぶ)
- [2. 再現し、プラグインの動きを読む](#2-再現しプラグインの動きを読む)
- [3. 実装する](#3-実装する)
- [4. テストする](#4-テストする)
- [5. ベースラインと比べる](#5-ベースラインと比べる)
- [6. コミットする](#6-コミットする)
- [ループの回し方](#ループの回し方)

## 0. ベースラインを保存する

作業を始める前に、CLIをビルドし、各プラグインの現状を記録します。

```bash
cargo build -p aexlo-cli
mkdir -p target/compat
aexlo compat fixtures/plugins/<macos/windows>/<plugin.ext> --json \
  --save-baseline target/compat/base.json > target/compat/before.json
```

## 1. 問題を特定する

出力されたJSONレポートの `gaps`（テキストレポートでは末尾）には、`missing-suite` / `stub` / `declined` の問題が、影響の広い順に並びます。
`crash` / `timeout` / `invalid` はステータスとして、`summary` と各シナリオに出力されます。

| 順 | 対象 | 理由 |
| -- | ---- | ---- |
| 1 | `crash`, `timeout` | 壊れていて、その先のシナリオを測れない |
| 2 | `load` で出る `missing-suite` | そのプラグインの全シナリオがそのスイートなしで動く。報告は `load` に1回だけ出る |
| 3 | 多くのプラグインが当たる `missing-suite` | 1つの実装で良くなるシナリオが一番多い |
| 4 | `stub` | 成功を返すだけで何もしないので、ピクセルが間違っている可能性が高い |
| 5 | `declined` | 呼び出しは提供しているが、その引数には応じない。たいてい小さい |
| — | `invalid` | aexlo ではなく `compat` の不足。`compat` を直し、別のコミットにする |

## 2. 再現し、プラグインの動きを読む

その不足に当たるプラグインとシナリオに `compat` を絞ります。

```bash
aexlo compat fixtures/plugins/macos/Example.aex --only "param:#1" --all
```

次に [`aexlo inspect`](inspect.ja.md) でプラグインを動かし、どの呼び出しをどの引数で行い、結果をどう使うかを見ます。

```bash
aexlo inspect fixtures/plugins/macos/Example.aex --json --script -
# render / set / changed / updateui で呼び出しを起こし、focus #<seq> で掘り下げる。
```

プラグインがクラッシュすると `inspect` も道連れになります。落ちたコマンドのイベントは出ず、最後のレポートもありません（[inspect.ja.md §6](inspect.ja.md#6-終了コードとエラー)）。
`compat` の `last_command` が、落ちた `PF_Cmd_*` を示します。
`--strict --serial` を付け、そのコマンドの手前までのトレースを `report` と `focus` で読んでから、そのコマンドを送ります。
TUI なら、落ちた `PF_Cmd_*` とスイート呼び出しを表示します。

## 3. 実装する

| 不足 | 場所 |
| ---- | ---- |
| `missing-suite` | `crates/aexlo/src/suites/<name>.rs` を新しく作るか既存のものに足し（新しいファイルは `suites/mod.rs` で宣言する）、`SuiteContainer` にフィールドを足して `SUITE_CONTAINER` で埋め、`rusty_acquire_suite` に分岐を足す |
| `stub` | `stub_log!` の関数、または `UnsupportedKind::Stub` の報告を本来の動作に置き換える |
| `declined` | その引数を扱えるよう関数を拡張する |

- スイートは全インスタンスで共有するステートレスな vtable です。状態はコールバックが受け取るポインターの先に置きます。
  `suites/mod.rs` 冒頭の安全性の前提に従ってください。
- **まだできないことに成功を返さないでください。** aexlo がまだ扱えない分岐では、引き続き
  `observe::unsupported(UnsupportedKind::…, …)` を呼びます。
  そうしないと `compat` がそれを `ok` と数え、数字が意味を持たなくなります。

## 4. テストする

- スイートの単体テストはスイートのそばに書きます。
- 実際のプラグインでの動作は `tests/e2e/tests/<plugin>.rs` に書きます。
  フィクスチャは `test_e2e::fixture("<stem>")` で読み込み、`#[test]` の上に `#[needs_fixture("<stem>")]` を付けます。フィクスチャのほとんどはローカルなので、ない環境では成功ではなく ignored と報告されます。
- 呼び出しを誤用するプラグインへの耐性は `tests/misbehave` で確かめます。
- ピクセルは `aexlo test` で確かめます。`compat` はプラグインが動くかしか見ません。
- 最後にワークスペース全体:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
```

## 5. ベースラインと比べる

```bash
cargo build -p aexlo-cli
target/debug/aexlo compat fixtures/plugins/macos --json \
  --baseline target/compat/base.json > target/compat/after.json
echo $?
```

- 終了コードは0、`worse` は空でなければなりません。
- 狙ったシナリオが `better` に入っているはずです。入っていなければ、埋めた不足の後ろに別の不足が隠れています。

## ループの回し方

- 進み具合を記録するには、毎周の不足一覧（JSONなら `gaps`、テキストならレポートの末尾）を残しておきます。`summary` の件数で、aexloがどこまで動いたかがわかります。
- 作業を並行して回すなら、問題ごとにworktreeを分け、同じスイートに触れない問題を選びます。
  Cargoはビルドのたびに `target/` のdylibをリンクし直すので、複数のループで同じ `target/` を共有し、ビルドと読み込みを同時に行ってはいけません。
- Windowsのフィクスチャは `fixtures/plugins/windows` にあります。プラットフォーム固有の変更なら、`.aex` のプラグインで同じループを回します。
