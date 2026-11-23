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
aexlo compat MyGlow.plugin Other.plugin --all        # すべてのプラグインとシナリオを表示
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
| `input:text` | エフェクトのレイヤーを[テキストレイヤー](inspect.ja.md#テキストレイヤー)にする。`Text layer` と `42` の2行を表示 |
| `size:<1x1\|37x23\|1920x1080>` | 入力と出力のサイズ |
| `time:<90@30\|1@23.976>` | フレームとフレームレート |
| `iterate:serial` | iterate のコールバックを1スレッドで実行 |
| `param:#<i> <name>=<value>` | パラメーター1つ。スライダーは範囲の両端、チェックボックスは両方の状態、ポップアップは全項目、ポイント・カラー・角度は2つの値 |
| `layer:#<i> <name>=linked` | レイヤーパラメーター1つにチェッカーボードをリンク |

パラメーターの値は、プラグインが読み込み時に宣言した内容から決まります。
パラメーターの多いプラグインほどシナリオも多くなります。
ボタン、グループ、パス、カスタム UI、任意データは設定しません。

### テキストレイヤー上

テキストレイヤーでしか働かないエフェクトがあります。
ほかのレイヤーでは入力をそのまま通すので、上のシナリオではその処理に届きません。
`compat` は `input:text` と同じピクセルを AV レイヤーでもレンダリングします。
プラグインが2つを違う結果にレンダリングしたら、テキストレイヤーで働くエフェクトとみなし、`input:*` 以外のすべてのシナリオをテキストレイヤー上でもう一度実行します。
その ID は `text:param:#18 Gradient Mode=Radial` のように `text:` で始まります。
`--only text:` でそれだけに絞れます。

## 2. ステータス

| ステータス | 意味 |
| ---------- | ---- |
| `ok` | 動作し、aexlo が応えないものは何も求めなかった |
| `partial` | 動作したが、aexlo が応えないものを求めた。何を求めたかはレポートに出る |
| `n/a` | プラグインが宣言していない（深度、スマートレンダー、GPU）。スマートレンダーを宣言したプラグインが `PF_Cmd_RENDER` で `PF_Err` を返したときの `render:legacy@*` も `n/a`。After Effects はスマートレンダー対応のプラグインにこのコマンドを送らない |
| `invalid` | シナリオがプラグインに合わない。aexlo ではなく `compat` 側の不足 |
| `error` | プラグインが `PF_Err` を返した |
| `timeout` | `--timeout` 以内に結果が返らなかった |
| `crash` | ワーカーが落ちた。そのとき実行中だったコマンドをレポートに出す |

読み込み時にすでに出た不足は `load` に一度だけ記録します。
各シナリオが `partial` になるのは、それ以外の不足があったときだけです。

### 対応率（coverage）

対応率は、適用できるシナリオのうち `ok` だったものの割合です（`ok / (全体 - n/a - invalid)`）。
`n/a` と `invalid` は aexlo について何も示さないので除きます。
`partial` は対応していないものとして数えます。プラグインは動いたものの、aexlo が求められたことをすべてはしていないためです。
レポートでは全体とプラグインごとに、切り捨てで表示します。`100.0%` は全シナリオが `ok` だったときだけです。
そこに届いているプラグインは[互換プラグイン](compatible-plugins.ja.md)に一覧があります。

## 3. オプション

| オプション | 既定値 | |
| ---------- | ------ | - |
| `<plugin\|dir>...` | | プラグインのファイル、またはそれを含むディレクトリ。ディレクトリ内のリンク切れのシンボリックリンクは飛ばす |
| `--only <text>` | | ID に `<text>` を含むシナリオだけ実行する。`load` は常に実行する。`text:` シナリオを決めるため `input:text` も常に実行するが、一致したときだけ一覧に出す |
| `--jobs <n>` | CPU 数の半分 | 同時に動かすワーカーの数 |
| `--timeout <s>` | `20` | シナリオごとの秒数 |
| `--all` | | すべてのプラグインとシナリオを表示する。指定しない場合は、問題のあったものだけを表示する（[出力](#4-出力)） |
| `--json` | テキスト | レポートを1つの JSON で出力する |
| `--color <auto\|always\|never>` | `auto` | テキストのレポートに色、記号、バーを付ける。`auto` は端末に出すときだけ付ける（`NO_COLOR` があれば付けない） |
| `--save-baseline <file>` | | 各シナリオのステータスを書き出す |
| `--baseline <file>` | | 保存したベースラインと比べる |

終了コードは通常 0 です。
`--baseline` で悪化したシナリオが見つかると 1 になります。
知らないオプションや見つからないプラグインは 2、ワーカーを起動できなければ 3 です。

## 4. 出力

テキストのレポートは、最初に合計（対応率と、結果ごとのシナリオ数）を表示します。
続いて、問題のあったプラグインを重い順（crash、timeout、error、partial、invalid）に表示します。
各プラグインには対応率と、結果ごとにまとめたシナリオが付きます。
ステータス、メッセージの1行目、不足がすべて同じシナリオを1つのグループにまとめます。
グループには、ステータス、シナリオの数、そうなった理由、先頭のいくつかの ID を表示します。
すべてのシナリオが `ok` か `n/a` だったプラグインは、1つの一覧にまとめます。
最後に、すべての不足とそれに当たったプラグインを、多い順に表示します。
これが次に実装すべきものの一覧になります。
同じ情報は一度しか出しません。件数は合計にだけ、何が起きたかはグループにだけ表示します。

```text
aexlo compat: 34 plugins, 5323 scenarios, 94.2% coverage
4919 ok · 1 partial · 103 n/a · 8 error · 292 crash

Falloff  2.6%
    crash     ×145  worker killed by signal 11 (SIGSEGV) during PF_Cmd_SMART_PRE_RENDER
                    render:auto@8, render:smart@8, render:smart@16, render:smart@32, … (+141)

AutoFill2  did not load
    error       ×1  loading AutoFill2.plugin: Plugin not found: AutoFill2.plugin/Contents/MacOS

RainyDay  99.1%
    partial     ×1  missing-suite PF Effect Sequence Data Suite v1 (in PARAMS_SETUP)
                    load

Everything ok or n/a (25):
    AOD_ChannelRemap, AOD_ColorAjust, AOD_ColorChange, AOD_ColorComposite, AOD_ColorConvert,
    …, RXXR, SDK_Noise, TextGrad

Asked for, but not done by aexlo (plugins that asked):
    1  missing-suite PF Effect Sequence Data Suite v1  [RainyDay]
```

端末に出すときは色が付きます。
ステータスごとの色と記号（`✓` ok、`◐` partial、`○` n/a、`?` invalid、`✗` error と crash、`◷` timeout）を表示し、合計と各プラグインには、ステータスの割合を示すバーを付けます。
実行中は stderr に進捗の行を表示し、段階（読み込み、AV レイヤー、テキストレイヤー）と終わったシナリオの数を示します。
パイプやリダイレクトに出すときは上のプレーンテキストになり、進捗の行も出ません。

`--all` を付けると、すべてのプラグインを個別に表示し、`ok` と `n/a` のシナリオもまとめて、各グループのシナリオをすべて示します。

`--json` の形式は [英語版](compat.md#4-output) を参照してください。
各シナリオは `id`、`status`、`message`、`unsupported` を持ちます。
`error`、`crash`、`timeout` の場合は `last_command` も持ちます。
全体には `summary`、`coverage`、`plugins`、`gaps` があり、`--baseline` を指定すると `worse`、`better`、`coverage_change`（`was` と `now`）も加わります。
`coverage` は 0 から 1 の比率で、プラグインごとにもあります。適用できるシナリオがなければ `null` です。

同じ不足は `aexlo test --json` でも実行ごとに出力されます（`unsupported`）。
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

対応率も、両方の実行にあるシナリオだけで、以前と今を比べて表示します。
`--only` で絞った実行や、ベースラインにないプラグインを含む実行でも、同じ条件で比べられます。

```text
Coverage since the baseline: 93.1% -> 94.2% (+1.1 points, on the scenarios both runs have)
```

プラグインのフィクスチャはほとんどがマシンごとのローカルなもの（`fixtures/plugins/.gitignore`）です。
そのため、ベースラインはリポジトリではなくフィクスチャのそばに置いてください。
aexlo を変更する前に保存し、変更後に比べます。

不足を選んでから修正をコミットするまでの一連の手順は [compat-workflow.ja.md](compat-workflow.ja.md) にあります。
