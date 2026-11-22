# `aexlo compat` — aexlo がプラグインをどこまで動かせるか

[English](compat.md) | 日本語

`aexlo compat` が測るのはプラグインではなく aexlo です。
プラグインの出力が正しいかは調べません（それは `aexlo test` の役目です）。
調べるのは、aexlo が各プラグインを読み込み、さまざまな条件で動かせるかどうかです。
あわせて、プラグインが求めたのに aexlo が応えなかったものを一覧にします。

- aexlo が提供していないスイートやそのバージョン（`missing-suite`）
- スタブしかないホスト関数。成功を返すだけで何もしない（`stub`）
- 提供はしているが、渡された引数では応じない呼び出し（`declined`）

シナリオは1つずつワーカープロセスの新しいインスタンスで実行します。
プラグインがクラッシュしたりハングしたりしても、失うのはそのシナリオだけです。

```bash
aexlo compat fixtures/plugins/macos                  # ディレクトリ内のすべてのプラグイン
aexlo compat MyGlow.plugin Other.plugin --all        # すべてのシナリオを表示
aexlo compat fixtures/plugins/macos --save-baseline compat.json
aexlo compat fixtures/plugins/macos --baseline compat.json   # 悪化があれば終了コード 1
```

- [1. シナリオ](#1-シナリオ)
- [2. ステータス](#2-ステータス)
- [3. オプション](#3-オプション)
- [4. 出力](#4-出力)
- [5. ベースライン](#5-ベースライン)

## 1. シナリオ

`size:*` 以外のシナリオは 128×96 でレンダリングします。
特に記載のない条件は、グラデーションの入力、8 bpc、`auto` のレンダーパス、24 fps の 0 フレーム目、並列の iterate です。
シナリオごとに変える条件は1つだけです。

| ID | 変える条件 |
| -- | ---------- |
| `load` | なし。プラグインの読み込み（`GLOBAL_SETUP`、`PARAMS_SETUP`、`SEQUENCE_SETUP`） |
| `render:auto@8` | なし。すべて既定のまま1回レンダリング |
| `render:<legacy\|smart\|gpu>@<8\|16\|32>` | レンダーパスと深度。組み合わせごとに1シナリオ |
| `input:<solid\|transparent\|checker\|noise>` | 入力レイヤー |
| `size:<1x1\|37x23\|1920x1080>` | 入力と出力のサイズ |
| `time:<90@30\|1@23.976>` | フレームとフレームレート |
| `iterate:serial` | iterate のコールバックを1スレッドで実行 |
| `param:#<i> <name>=<value>` | パラメーター1つ。スライダーは範囲の両端、チェックボックスは両方の状態、ポップアップは全項目、ポイント・カラー・角度は2つの値 |
| `layer:#<i> <name>=linked` | レイヤーパラメーター1つにチェッカーボードをリンク |

パラメーターの値は、プラグインが読み込み時に宣言した内容から決まります。
パラメーターの多いプラグインほどシナリオも多くなります。
ボタン、グループ、パス、カスタム UI、任意データは設定しません。

## 2. ステータス

| ステータス | 意味 |
| ---------- | ---- |
| `ok` | 動作し、aexlo が応えないものは何も求めなかった |
| `partial` | 動作したが、aexlo が応えないものを求めた。何を求めたかはレポートに出る |
| `n/a` | プラグインが宣言していない（深度、スマートレンダー、GPU） |
| `invalid` | シナリオがプラグインに合わない。aexlo ではなく `compat` 側の不足 |
| `error` | プラグインが `PF_Err` を返した |
| `timeout` | `--timeout` 以内に結果が返らなかった |
| `crash` | ワーカーが落ちた。そのとき実行中だったコマンドをレポートに出す |

読み込み時にすでに出た不足は `load` に一度だけ記録します。
各シナリオが `partial` になるのは、それ以外の不足があったときだけです。

## 3. オプション

| オプション | 既定値 | |
| ---------- | ------ | - |
| `<plugin\|dir>...` | | プラグインのファイル、またはそれを含むディレクトリ。ディレクトリ内のリンク切れのシンボリックリンクは飛ばす |
| `--only <text>` | | ID に `<text>` を含むシナリオだけ実行する。`load` は常に実行する |
| `--jobs <n>` | CPU 数の半分 | 同時に動かすワーカーの数 |
| `--timeout <s>` | `20` | シナリオごとの秒数 |
| `--all` | | すべてのシナリオを1つずつ表示する。指定しない場合は `ok` と `n/a` 以外を、結果ごとにまとめて表示する |
| `--format json` | テキスト | レポートを1つの JSON で出力する |
| `--save-baseline <file>` | | 各シナリオのステータスを書き出す |
| `--baseline <file>` | | 保存したベースラインと比べる |

終了コードは通常 0 です。
`--baseline` で悪化したシナリオが見つかると 1 になります。
知らないオプションや見つからないプラグインは 2、ワーカーを起動できなければ 3 です。

## 4. 出力

テキストのレポートは、プラグインごとに件数と、うまくいかなかったシナリオを表示します。
同じ失敗の仕方をしたシナリオ（ステータス、メッセージの1行目、不足がすべて同じ）はまとめ、そのうちいくつかの ID を示します。
最後に、すべての不足とそれに当たったプラグインを、多い順に表示します。
これが次に実装すべきものの一覧になります。

```text
RainyDay  load partial · 20 ok · 93 partial
    partial  load
               missing-suite PF Effect Sequence Data Suite v1 (in PARAMS_SETUP)
    partial  ×93 scenarios
               missing-suite AEGP Stream Suite v11 (in UPDATE_PARAMS_UI)
               e.g. param:#1 Preset=Custom, param:#1 Preset=Default, … (+91)

Asked for, but not done by aexlo (plugins that asked):
   10  missing-suite AEGP Stream Suite v11  [AOD_ChannelRemap, …, RainyDay]
```

`--format json` の形式は [英語版](compat.md#4-output) を参照してください。
各シナリオは `id`、`status`、`message`、`unsupported` を持ちます。
`error`、`crash`、`timeout` の場合は `last_command` も持ちます。
全体には `summary`、`plugins`、`gaps` があり、`--baseline` を指定すると `worse` と `better` も加わります。

同じ不足は `aexlo test --format json` でも実行ごとに出力されます（`unsupported`）。
コードからは `Observer::unsupported` で `UnsupportedEvent` として受け取れます。

## 5. ベースライン

`--save-baseline` は、プラグインごとに各シナリオのステータスを書き出します。

```json
{ "plugins": { "RainyDay": { "load": "partial", "render:auto@8": "ok" } } }
```

`--baseline` は、変化したシナリオをすべて表示します。
`ok` → `partial` → `error` → `timeout` → `crash` の順に後ろへ動いたものが悪化です。
どちらかで `n/a` または `invalid` のシナリオは比べません。
片方にしかないシナリオやプラグイン（名前の変わったパラメーターなど）も比べません。

プラグインのフィクスチャはほとんどがマシンごとのローカルなもの（`fixtures/plugins/.gitignore`）です。
そのため、ベースラインはリポジトリではなくフィクスチャのそばに置いてください。
aexlo を変更する前に保存し、変更後に比べます。
