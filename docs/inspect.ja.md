# `aexlo inspect` — プラグインを操作して分析する

[English](inspect.md) | 日本語

`aexlo inspect` は、ビルド済みの After Effects プラグインを読み込み、コマンドを実行します。
コマンドには、レンダリング、パラメーターの設定、`PF_Cmd_DO_DIALOG` の送信などがあります。
その間にプラグインが行ったことは、すべて記録されます。

- 送られたすべての `PF_Cmd_*`
- コールバックしたすべてのホストのスイート関数と、その引数
- 確保・解放したすべてのハンドルとワールド

記録をもとに、エラー、リーク、null ポインター、解放後の使用といった問題を指摘します。

使い方は3通りあります。

| モード | 起動方法 | 用途 |
| ------ | -------- | ---- |
| TUI | `aexlo inspect <plugin>` | 人が手で調べる |
| バッチ | `aexlo inspect <plugin> --exec "<コマンド>"` または `--script <file>` | スクリプトや CI。決まった手順を実行して分析を出力する |
| パイプ越しのセッション | `aexlo inspect <plugin> --script -` | エージェントやツール。1つ送って答えを読み、次を決める |

バッチとパイプの出力は、標準ではテキストです。
`--format json` を付けると1行に1つの JSON オブジェクトになり、プログラムでパースできます（[イベント](#4-イベント)）。
`aexlo inspect --help` でオプションとコマンドの一覧を表示します。

- [1. はじめに](#1-はじめに)
- [2. オプション](#2-オプション)
- [3. コマンド](#3-コマンド)
- [4. イベント](#4-イベント)
- [5. 共通オブジェクト](#5-共通オブジェクト)
- [6. 終了コードとエラー](#6-終了コードとエラー)
- [7. 使用例](#7-使用例)

## 1. はじめに

`<plugin>` には、プラグインの成果物（`.plugin`、`.aex`、`.dll`、`.dylib`）かクレートのディレクトリを指定します。
クレートのディレクトリは、先にビルドされます。

`SDK_Noise` のようにパス区切りを含まない名前は、カレントディレクトリでプラットフォームの拡張子（macOS は `.plugin`、Windows は `.aex`）を付けても探します。
パス区切りを含む場合は、ファイル名を正確に指定してください。
`fixtures/plugins/macos/SDK_Noise` ではなく `fixtures/plugins/macos/SDK_Noise.plugin` です。

```bash
# 手で調べる。`:help` でコマンド一覧、q で終了
aexlo inspect build/MyGlow.plugin

# 決まった手順を実行し、分析をテキストで出力する
aexlo inspect MyGlow --exec "set Radius 50; render smart; focus checkout"

# 同じことを JSON の行で出力する
aexlo inspect MyGlow --format json --exec "render; render smart"

# プラグインを読み込んだまま、stdin から1行ずつコマンドを送る
aexlo inspect MyGlow --format json --script -
```

### `--exec`、`--script <file>`、`--script -`

3つとも、TUI のプロンプトで入力するのと同じ[コマンド](#3-コマンド)を実行します。
違うのは、コマンドの受け取り方と、間違いがあったときの扱いです。
指定できるのはどれか1つで、2つ指定するとエラーになります。

| | `--exec "<cmds>"` | `--script <file>` | `--script -` |
| - | - | - | - |
| コマンドの取得元 | 引数 | ファイル | 標準入力 |
| 区切り | `;`（改行も可） | 改行と `;` | 改行と `;` |
| `#` コメント | `#` で始まる区切りを無視 | `#` で始まる行（または区切り）を無視 | `#` で始まる行（または区切り）を無視 |
| パースするタイミング | プラグインの読み込み前に全部 | プラグインの読み込み前に全部 | 1行届くたび |
| 知らないコマンドがあった場合 | 読み込み前に全体が止まる（終了コード 1） | 読み込み前に全体が止まる（終了コード 1）。`quit` より後ろにあっても同じ | その行だけ失敗（`ok: false`）。セッションは続く |
| 終了するタイミング | 最後のコマンドの後か `quit` | 最後のコマンドの後か `quit` | `quit` を受け取るか、stdin が閉じたとき |

`;` は常にコマンドを区切り、エスケープできません。
そのため、`;` を含む値やパスはどのコマンドにも渡せません。

`--script -` は、1行に答えてから次の行を読みます。
そのため、プログラムは `render` を送って結果を読み、それを見てから `focus #29` や `set Radius 10` を送るか決められます。
どれも、読み込んだ同じプラグインに対して実行されます。
ほかの2つのモードは、手順を最後まで続けて実行します。

どのモードも終わり方は同じです。
プラグインを破棄（`GLOBAL_SETDOWN`）してから、分析をもう一度出力します（[`report`](#report)）。

## 2. オプション

| オプション | 意味 | 既定値 |
| ---------- | ---- | ------ |
| `-i`, `--input <png>` | 入力レイヤー | グラデーション |
| `--text <string>` | [`input text`](#3-コマンド) と同じく、レイヤーを `<string>` を表示するテキストレイヤーにする。`--input` とは併用できない | テキストレイヤーではない |
| `--font <file>` | `--text` と `input text` で使う TrueType / OpenType フォント | システムのサンセリフ書体（macOS と Windows では Arial） |
| `--size <W>x<H>` | 既定の入力と出力のサイズ | 640x360、または `--input` のサイズ |
| `--depth <8\|16\|32>` | プロジェクトのビット深度（チャンネルあたり） | 8 |
| `--level <args\|calls\|commands>` | 記録する内容。下を参照 | `args` |
| `--strict` | 最初のコマンドから strict チェック（ポイズン、ガードバンド、確保とチェックアウトの追跡）をすべて有効にする | 無効 |
| `--serial` | iterate コールバックをプラグインのスレッドで実行し、その中のスイート呼び出しも記録する | 無効 |
| `--exec <cmds>` | バッチ。コマンドを実行する | — |
| `--script <file\|->` | ファイルからのバッチ。`-` なら stdin からのセッション | — |
| `--format <human\|json>` | バッチの出力形式。テキストか [JSON の行](#4-イベント) | `human` |
| `--trace` | バッチで、レポートにトレース全体も含める | 無効 |
| `-h`, `--help` | オプションとコマンドの一覧を表示する | — |

`--format` と `--trace` には `--exec` か `--script` が必要です。

`--level` は、スイート呼び出しをトレースに残すかを決めます。
ステップ、`PF_Cmd_*` の開始と終了、確保、解放、チェックアウトは、どのレベルでも記録されます。

| レベル | スイート呼び出し |
| ------ | ---------------- |
| `args` | 引数と戻り値を付けて記録する |
| `calls` | 引数なしで記録する |
| `commands` | 記録しない |

## 3. コマンド

TUI のプロンプトと同じコマンドです。
先頭の `:` はいくつ付いていても無視され、コマンド名の大文字小文字は区別しません。
プラグインのコマンドはプラグインのスレッドで実行され、「ステップ」の下に記録されます。
ステップとは、送ったコマンド行のことです。

| コマンド | 別名 | 動作 |
| -------- | ---- | ---- |
| `about` | | `PF_Cmd_ABOUT`。メッセージはプラグインのテキスト |
| `render [auto\|smart\|legacy\|gpu]` | `r` | 指定した経路でフレームをレンダリングする。`auto` は GPU、smart、legacy の順に選び、失敗すれば次の経路にフォールバックする |
| `set <param> <value>` | | パラメーターを設定する。[パラメーターの設定](#パラメーターの設定)を参照 |
| `key <param> <frame> <value>` | | 現在のフレームレートでのフレームに線形キーフレームを追加する。空白を含む名前は引用符で囲む。`set` はキーフレームを定数値に置き換える |
| `mask <rect\|ellipse\|triangle\|ring\|none>` | | レイヤーのマスクをその図形に差し替える |
| `changed <param>` | | `PF_Cmd_USER_CHANGED_PARAM`（プラグインが求めれば `UPDATE_PARAMS_UI` も）。`<param>` は行の残り全部で、引用符は省略できる |
| `updateui` | `update-ui` | `PF_Cmd_UPDATE_PARAMS_UI` |
| `dialog` | | `PF_Cmd_DO_DIALOG`（Options ボタン）。[ダイアログ](#ダイアログ)を参照 |
| `time <frame> [fps]` | | フレームを移動する（fps の既定は 30） |
| `size <w> <h>` | | 以降のレンダリングの出力サイズ（1〜16384） |
| `input <png>` | | 入力レイヤーを PNG に差し替え、現在のビット深度に変換する。行の残りがパスで、引用符は省略できる |
| `input <black\|white\|gradient> [w h]` | | 入力レイヤーを生成した画像に差し替える。サイズは指定したものか、現在のもの |
| `input text <string>` | | エフェクトのレイヤーを、現在のサイズで `<string>` を表示するテキストレイヤーにする。`\n` で改行し、引用符は省略できる。[テキストレイヤー](#テキストレイヤー)を参照 |
| `input shape <rect\|ellipse\|triangle\|ring>` | | エフェクトのレイヤーを、現在のサイズでその図形を表示するシェイプレイヤーにする。[シェイプレイヤー](#シェイプレイヤー)を参照 |
| `depth <8\|16\|32>` | | 入力（プロジェクト）のビット深度を変換する。プラグインがそのビット深度を宣言していなければ失敗する |
| `reload` | | プラグインを破棄して読み込み直す。クレートなら先に再ビルドする |
| `level <commands\|calls\|args>` | `cmd`、`call`、`arguments` | 記録する内容を変える |
| `strict <on\|off\|list>` | `all`、`none` | strict チェックを設定する。`list` は `poison`、`guard`、`alloc`、`checkout` をカンマで区切ったもの（空白も可）。現在の設定に追加するのではなく置き換えるので、`strict guard` はほかを無効にする |
| `serial <on\|off>` | | `--serial` と同じ |
| `save <path>` | | 出力フレームを `.png`（8 bpc、それより深い出力なら 16 bpc）か `.exr`（32 bpc）で書き出す。ほかの拡張子は拒否する。パスには空白を含められる。最初のレンダリングより前に実行すると、出力にその時点で入っている内容（ホストの初期バッファー）を書き出す |
| `focus <text\|#seq>` | `f`、`diag` | トレースの1項目を診断する。[`focus`](#focus) を参照 |
| `report` | | その時点の分析を出力する（バッチのみ） |
| `clear` | | トレースと集計を消す。[コマンド間で引き継がれる状態](#コマンド間で引き継がれる状態)を参照 |
| `help` | `?` | コマンド一覧を出力する |
| `quit` | `q`、`exit` | セッションを終える。バッチでは以降のコマンドを実行せず、プラグインを破棄して最後のレポートを出力する。`quit` 自身の `command` イベントは出ない |
| `mem <address\|global\|sequence>` | `memory` | TUI 専用。Memory タブに確保を表示する。バッチでは飛ばす |
| `find <text>` | `/` | TUI 専用。Trace タブと Calls タブを絞り込む。バッチでは飛ばす |

### パラメーターの設定

`<param>` は、次のどれかの形で指定します。

| 形 | 例 |
| -- | -- |
| インデックス（`#0` は入力レイヤーで、設定できない） | `set 3 0.5`、`set #3 0.5` |
| 引用符（`"` か `'`）で囲んだ名前 | `set "Noise variation" 25` |
| `=` の前の名前 | `set Noise variation=25` |
| そのまま並べた名前 | `set Noise variation 25` |

名前は完全一致で探します。
見つからなければ、大文字小文字を無視して1つに決まる場合に限り、それを使います。
そのまま並べる形では、先頭から続く単語のうち、パラメーター名に当たる最も長い部分を名前とし、残りを値とします。
たとえば `set Emitter Type 2` は、`Emitter` という名前のグループがあっても `Emitter Type` を設定します。
同じ名前のパラメーターが複数あれば、エラーにそのインデックスの一覧が出るので、どれかを指定してください。

`=` の形は最初の `=` で分けます。
そのため、`=` を含む値には引用符の形を使ってください。

値は、パラメーターの型（[`report.params`](#report) の `kind`）に応じて読み取ります。

| `kind` | 値 | 例 |
| ------ | -- | -- |
| `FloatSlider`、`FixedSlider` | 数値 | `set Gain 1.5` |
| `Slider` | 整数 | `set Count 10` |
| `Angle` | 度数。後ろに `°` か `deg` を付けてもよい | `set Rotation 90°` |
| `Checkbox` | `true`/`false`、`on`/`off`、`yes`/`no`、`1`/`0` | `set Enable on` |
| `Popup` | 1 から始まる選択肢の番号、または選択肢の名前（大文字小文字は区別しない） | `set Mode 3`、`set Mode Leak` |
| `Color` | 各チャンネル 0〜255 の `r,g,b` か `r,g,b,a`（アルファの既定は 255）、または `#rrggbb` / `#rrggbbaa` | `set Tint 255, 128, 0`、`set Tint #ff8000` |
| `Point` | `x,y` | `set Center 320, 180` |
| `Point3D` | `x,y,z` | `set Position 1, 2, 3` |
| `Path` | マスク ID（整数） | `set Mask 1` |

色はプロジェクトのビット深度にかかわらず 8 ビット（`PF_Pixel`）で、保存されている値をそのまま表示します。
プラグインがアルファを `1` と宣言していれば、色は `0, 0, 0, 1` と表示されます。
これは 255 段階中 1 のアルファという意味です。

`Button`、`Layer`、`GroupStart`、`GroupEnd`、`NoData`、`ArbitraryData`、`Custom` のパラメーターには設定できる値がありません。
`set` するとその旨と型を返します。

スライダーの範囲外の値や、選択肢の数を超えるポップアップの番号も、そのまま設定します。
プラグインをそうした値で試すこともあるからです。
メッセージには `#2 = 9  (outside the slider range 0..4)` のように注記が付きます。
After Effects では、ユーザーがこうした値を入力することはできません。

### コマンド間で引き継がれる状態

| 対象 | `clear` | `reload` |
| ---- | ------- | -------- |
| トレース、コマンドとスイート呼び出しの集計、`errors` | 消える。項目の番号は `#1` からやり直し | 残る。破棄と新しいセットアップが追記される |
| `failed` の数 | 残る | 残る |
| プラグイン、そのメモリとパラメーターの値 | 変わらない | 破棄して読み込み直す。パラメーターの値は、同じ型のパラメーターがまだあれば引き継ぐ |
| `size`、`input`（とそのテキスト）、`depth`、`level`、`strict`、`serial` | 変わらない | 引き継ぐ |

出力のサイズは、`--size` か `size` で指定しない限り入力のサイズになります。
あとで `input <png>` で入力を差し替えても出力のサイズは変わらないので、両者は異なることがあります。
その場合プラグインは、レイヤーを大きくするエフェクトと同じように、あるサイズの入力から別のサイズの出力をレンダリングします。

### テキストレイヤー

テキストレイヤーでしか動かないエフェクトがあります。`AEGP_GetLayerObjectType` でレイヤーの種類を尋ね、AEGP Text Layer Suite でグリフのアウトラインを読みます。ほかのレイヤーでは、たいてい入力をそのまま返します。`input text <string>`（または `--text`）は、エフェクトのレイヤーをテキストレイヤーにします。

- `AEGP_GetLayerObjectType` は `AEGP_ObjectType_TEXT` を返します。
- `AEGP_GetNewTextOutlines` は、グリフの輪郭ごとに閉じたアウトラインを 1 つ、読む順に、レイヤーのピクセル座標で渡します。PF Path Data Suite は、After Effects がテキストのアウトラインを渡すときと同じく、接線を絶対座標の点として返します。
- 入力レイヤーは、透明の上に白で描いたテキストです。After Effects がエフェクトの前にテキストレイヤーを描く結果と同じです。

テキストは中央揃えの段落として、レイヤーの中央に、収まる最大の大きさ（最大でレイヤーの高さの 40%）で配置します。ほかの `input` を実行すると、レイヤーは AV レイヤーに戻ります。

```bash
aexlo inspect TextGrad --text "TextGrad" --exec "render; save text.png"
```

### シェイプレイヤー

`input shape <図形>` は、1 つのコンポジションからなるプロジェクトのシェイプレイヤーにエフェクトを置きます。プラグインはレイヤーのストリーム（`ADBE Root Vectors Group` から `ADBE Vector Shape` まで）でパスを見つけ、AEGP Mask Outline Suite で読みます。入力は、透明の上に白で描いた図形です。
図形は中央より左上に置くので、誤った座標空間でパスを読むと位置のずれでわかります。`mask <図形>` は同じ図形をマスクとして渡します。

```bash
aexlo inspect Extrude --exec "input shape ring; render smart; save ring.png"
aexlo inspect Extrude --exec "input shape ring; key 27 0 0; key 27 30 200; time 15; render smart; save half.png"
```

### ダイアログ

`dialog` は `PF_Cmd_DO_DIALOG` を送ります。多くのプラグインはこれに応えてネイティブのウィンドウを開きます。
バッチやパイプ越しの実行では、そのためのイベントループやウィンドウサーバーがない場合があります。
するとプラグインが止まったままになるか、クラッシュしてプロセスごと終了します（`DeepGlow2` がそうです）。
`dialog` は TUI から送るか、ウィンドウが開いている間プロセスを維持する `aexlo dialog <plugin>` を使ってください。

## 4. イベント

`--format json` では、stdout の各行が1つの JSON オブジェクトです。
`event` フィールドが行の種類を表します。
stdout を1行ずつ読み、`event` で処理を分けてください。
順序は必ず次のとおりです。

```text
loaded
command | focus | report | help      （送ったコマンド1つにつき1行。quit には出ない）
report                              （最後）
```

プラグインが見つからない、または読み込めない場合、stdout には `error` の1行だけが出ます。

時間はマイクロ秒（`*_us`）です。
アドレスは16進の文字列（`"0x1238…"`）で、`focus` にそのまま渡せる形です。

### `loaded`

プラグインが `GLOBAL_SETUP`、`PARAMS_SETUP`、`SEQUENCE_SETUP` を終え、コマンドを受け付けられる状態になりました。

| フィールド | 型 | |
| ---------- | -- | - |
| `plugin` | string | 成果物のパス |
| `params` | number | パラメーターの数（入力レイヤーを除く） |

```json
{"event":"loaded","params":4,"plugin":"build/MyGlow.plugin"}
```

### `command`

コマンドを1つ実行しました。
どの `command` イベントにも、`line`（送ったコマンド）と `ok`（成功したか）があります。
ほかのフィールドは、何が起きたかによって変わります。

| フィールド | 型 | 付く条件 | |
| ---------- | -- | -------- | - |
| `message` | string | `ok` | 出力された内容（`rendered 16x8 (Smart)`、`#1 = 7`、ABOUT のテキストなど） |
| `elapsed_us` | number | `ok` | かかった時間 |
| `error` | string | `ok` でない | 失敗の理由 |
| `step` | string \| null | 実行した | このステップの項目番号（`"#10"` など）。`focus` に渡すと、ステップが引き起こしたことを診断できる。`clear` はステップとして記録されないので `null` |
| `commands` | array | 実行した | ステップが送った `PF_Cmd_*`（いちばん外側のものだけ）。各要素は `{command, elapsed_us, error}` で、`error` は[エラーオブジェクト](#エラーオブジェクト)か `null` |
| `suite_calls` | number | 実行した | ステップ中のスイート呼び出しの数 |
| `allocations`、`frees` | number | 実行した | ステップ中に確保・解放したハンドルとワールドの数 |
| `skipped` | string | `mem`、`find` | 何もしなかった理由（`"only meaningful in the TUI"`）。`ok` は `true` |

「実行した」は、飛ばしたコマンドとパースできなかった行を除くすべてです。
パースできない行がここまで届くのは `--script -` のときだけで、そのイベントは `line`、`ok: false`、`error` だけになります。

`ok: true` でも、エラーを返した `PF_Cmd_*` を含むことがあります。
たとえば `render`（auto）が `SMART_RENDER` に失敗し、legacy の経路で成功した場合です。
`commands[].error` も確認してください。

```json
{"event":"command","line":"render legacy","ok":false,
 "error":"Legacy render failed: Plugin rejected Render with error code: 512",
 "step":"#29",
 "commands":[{"command":"PF_Cmd_RENDER","elapsed_us":10,
              "error":{"code":512,"name":"PF_Err_INTERNAL_STRUCT_DAMAGED"}}],
 "suite_calls":0,"allocations":0,"frees":0}
```

### `focus`

トレースの1項目の診断です。

`focus #<n>` は番号 `n` の項目を選びます。
それ以外はテキストとして扱い、[`text`](#項目オブジェクト) にそれを含む最新の項目を選びます（大文字小文字は区別しません）。
検索の対象は記号や引数も含めた行全体なので、具体的に指定してください。

- `focus checkout` は、`⇄ CheckoutLayerPixels` の行ではなく、引数に `checkout_idL` を含む後の呼び出しに一致することがあります。チェックアウトを選ぶなら `focus ⇄ Checkout` です。
- `focus ◀ PF_Cmd_RENDER` は、最新の legacy レンダリングの終了を選びます。エラーはこの項目にあります。（`◀ PF_Cmd_SMART_RENDER` はこの文字列を含みません。）
- `focus + alloc Handle 0x1238…` は、そのアドレスの確保を選びます。`focus 0x1238…` だけだと、そのアドレスに触れた最新の項目が種類を問わず選ばれます。

`#` の後ろが数字でなければ、テキストとして検索します。
`focus` の後ろに何もなければパースエラーです。

| フィールド | 型 | |
| ---------- | -- | - |
| `line`、`query` | string | コマンドと、`focus` の後ろのテキストまたは `#seq` |
| `ok` | bool | 一致する項目がなければ `false`。そのときは `entry` と `diagnosis` の代わりに `error` が付き、失敗したコマンドとして数える |
| `entry` | object | 一致した[項目](#項目オブジェクト) |
| `diagnosis` | object | `{title, sections}`。[診断オブジェクト](#診断オブジェクト)を参照 |

診断の内容は、項目の種類によって変わります。

- **スイート呼び出し：** 引数と戻り値。アドレスは確保の履歴と照らし合わせて判定します（生存中、解放済み、まだ確保されていない）。呼び出しの結果ホストが確保したメモリとその解放状況、実行中だったコマンド、その関数が呼ばれた回数も示します。
- **`PF_Cmd_*`（開始・終了）：** かかった時間、エラーとその手がかり、中で実行されたこと、確保してまだ保持しているもの、エラー直前の呼び出し。
- **確保・解放：** そのアドレスの一生、現在生存しているか、役割（`global_data`、`sequence_data`）、それを引数に取った呼び出し。
- **ステップ：** 引き起こしたコマンド、スイート呼び出し、確保、エラー。

どの診断も、最後に前後の項目を示す `around it` セクションが付きます。

```json
{"event":"focus","line":"focus host_new_handle","query":"host_new_handle","ok":true,
 "entry":{"seq":23,"type":"call","suite":"PF_HandleSuite1","function":"host_new_handle",
          "args":[{"name":"size","value":"64"}],"result":null, ...},
 "diagnosis":{"title":"PF_HandleSuite1::host_new_handle","sections":[
   {"heading":null,"items":[{"key":"inside","value":"PF_Cmd_SMART_RENDER  (+70 µs)","tone":"plain"}, ...]},
   {"heading":"arguments","items":[{"key":"size","value":"64","tone":"plain"}]},
   {"heading":"memory it moved","items":[
     {"text":"#24 + alloc Handle 0x123805590 (64 B)","tone":"plain"},
     {"text":"never freed: still held","tone":"warn"}]},
   ...]}}
```

### `report`

それまでに記録したすべての分析です。
`report` を送ったときと、最後に1回出力されます。

最後のレポートは、プラグインを破棄した後に出力します。
トレース、集計、`errors` には、`unload` ステップとそれが送った `PF_Cmd_GLOBAL_SETDOWN` が含まれます。
`findings` には、そのとき strict モードが判定したものが含まれます。
`plugin`、`state`、`params`、`memory` は、破棄する直前のプラグインの状態です。

| フィールド | 型 | |
| ---------- | -- | - |
| `failed` | number | それまでに失敗したコマンドの数 |
| `loaded` | bool | プラグインが読み込まれているか。`false`（`reload` の失敗後）のときは、`plugin` 以下のフィールドがない |
| `trace` | object | `{entries, dropped}`。`--trace` 付きなら、全[項目](#項目オブジェクト)の配列 `lines` も入る。トレースは最新 200,000 項目まで保持し、それより古いものは `dropped` に数える |
| `commands` | array | `PF_Cmd_*` ごとの集計（合計時間の長い順）。`{command, count, errors, max_us, total_us}` |
| `suite_calls` | array | スイート関数ごとの集計（呼ばれた回数の多い順）。`{suite, function, count, commands}`。`commands` は呼び出し元の `PF_Cmd_*` |
| `errors` | array | エラーを返したすべての `PF_Cmd_*`。`{seq, command, error}` |
| `plugin` | object | `path`。`smart_render`、`gpu`、`custom_ui`（bool）。`options_button`（ボタンのラベルか `null`）。`pixel_formats`（下を参照）。`out_flags`、`out_flags2`（[フラグオブジェクト](#フラグオブジェクト)） |
| `state` | object | 下を参照 |
| `params` | array | パラメーターごとに1要素。下を参照 |
| `memory` | array | 生存中のハンドルとワールド。`{kind, address, bytes, allocated_in, role, head}`。`kind` は `Handle` か `World`。`role` は `global_data`、`sequence_data`、`null` のどれか。`head` は先頭 64 バイトの16進で、それより小さい確保なら全部。読めなかったときは `null` |
| `findings` | string の配列 | strict モードの指摘（古い順）。[下](#指摘とリークが出るタイミング)を参照 |

`params` の各要素は `{index, name, kind, value, detail, hidden, disabled}` です。

- `kind` は型で、[パラメーターの設定](#パラメーターの設定)と同じ名前です。
- `value` は `set` が読み取る書き方で表示します。ただし角度には `°` が付き、パスは `mask <id>` になります。値のないパラメーターでは空です。
- `detail` には、スライダーの範囲（`0..100`）、ポップアップの選択肢（`1=OK 2=Crash ...`）、UI フラグ、`custom UI` が、2つの空白で区切って入ります。

`plugin.pixel_formats` は、プラグインが宣言したピクセル形式の一覧です。
`ARGB32`、`ARGB64`、`ARGB128`、`BGRA32`、`VUYA32`、`GPU_BGRA128` などがあります。
aexlo が名前を知らない FourCC は16進（`0x61726762`）で出ます。

`state` の中身は次のとおりです。

| フィールド | |
| ---------- | - |
| `global_data`、`sequence_data` | プラグインが返したハンドル（アドレスか `null`） |
| `time` | 次のコマンドの `{value, step, scale}` |
| `input`、`output` | `{width, height, bpc}` |
| `last_out_flags`、`last_out_flags2`、`last_message` | 直前のコマンドが `PF_OutData` に残した値 |
| `output_mean` | 出力全体での、ストレート（乗算済みでない）RGBA の各チャンネルの平均（0..1）。有限でない値（32 bpc で `strict poison` が書く NaN など）は 0 として数える |
| `unwritten_pixels` | `strict poison` のとき、直前のレンダリングが書かなかった出力ピクセルの数。それ以外は `null` |
| `parallel_iterate` | iterate コールバックを複数スレッドで実行しているか（`serial off`） |
| `strict` | `{poison, guard, alloc, checkout}` |

#### 指摘とリークが出るタイミング

strict モードは決まった時点で判定します。
そのため、指摘が `findings` に出るのはその時点を過ぎてからです。

| 指摘 | 必要な設定 | 判定する時点 | テキスト |
| ---- | ---------- | ------------ | -------- |
| ガードバンドの上書き（範囲外書き込み） | `guard` | レンダリングのたび | `guard band <band> of <world> overwritten (<n> B)` |
| レンダリングから漏れたワールド | `alloc` | それを確保したレンダリングのコマンドが終わったとき | `leak: World 0x… (<n> B) from <command> still held after <command>` |
| ハンドルのリーク | `alloc` | `GLOBAL_SETDOWN` のとき。つまり最後のレポートか、`reload` の後 | `leak: Handle 0x… (<n> B) from <command> still held after GLOBAL_SETDOWN` |
| チェックアウトの違反 | `checkout` | 誤ったチェックアウトやチェックインの時点、またはレイヤーパラメーターをチェックアウトしたままコマンドが終わったとき | `checkout: …` |

それより前でも、保持されているものは `memory` でわかります。
`role` が `null` で `allocated_in` がレンダリングのコマンドなら、そのレンダリングが終わった後も残っています。
最終的に解放されたかどうかは、`strict alloc` を有効にした最後のレポートでわかります。

### `help`

`{"event":"help","text":"..."}`。コマンド一覧のテキストです。

### `error`

`{"event":"error","error":"no plugin at nope.dylib"}`。
プラグインが見つからない、ビルドできない、または読み込めませんでした。
stdout にはほかに何も出ず、終了コードは 1 です。
stderr にも同じ内容の `error: ...` が出ます。

## 5. 共通オブジェクト

### エラーオブジェクト

`{"code": 512, "name": "PF_Err_INTERNAL_STRUCT_DAMAGED"}`。
SDK に名前のないコード（プラグイン独自のエラー）では、`name` は `PF_Err <code>` になります。

### 項目オブジェクト

トレースの1行です。
すべての項目に次のフィールドがあります。

| フィールド | |
| ---------- | - |
| `seq` | 番号。`focus #seq` で指定できる |
| `at_us` | 記録開始からの時刻 |
| `depth` | 入れ子の深さ。0 はステップか最上位のコマンド、1 はそのコマンドが呼んだもの、以下同様 |
| `repeat` | この項目にまとめた呼び出しの数（下を参照）。ほとんどの項目では 1 |
| `text` | TUI やテキスト出力での表示。`focus` はこれを検索する |
| `type` | 下の種類のどれか。種類ごとにフィールドが加わる |

| `type` | `text` の例 | 追加のフィールド |
| ------ | ----------- | ---------------- |
| `step` | `» render smart` | `line`：送ったコマンド |
| `begin` | `▶ PF_Cmd_SMART_RENDER` | `command` |
| `end` | `◀ PF_Cmd_RENDER  PF_Err_INTERNAL_STRUCT_DAMAGED (512)  (10 µs)` | `command`、`elapsed_us`、`error`（エラーオブジェクトか `null`） |
| `call` | `PF_HandleSuite1::host_new_handle(size=64)` | `suite`、`function`、`args`（`[{name, value}]`）、`result`（文字列か `null`）、`last`（下を参照） |
| `alloc`、`free` | `+ alloc Handle 0x… (64 B)`、`- free  Handle 0x… (64 B)` | `kind`（`Handle` か `World`）、`address`、`bytes` |
| `checkout` | `⇄ CheckoutLayerPixels #0` | `kind`、`id`（チェックアウトの番号） |

チェックアウトの `kind` は次のどれかです。

- `DeclareLayer`：`SMART_PRE_RENDER` 中の `checkout_layer`
- `CheckoutLayerPixels`、`CheckinLayerPixels`：`SMART_RENDER` 中
- `CheckoutParam`、`CheckinParam`：`PF_CHECKOUT_PARAM`、`PF_CHECKIN_PARAM`

**まとめ（折りたたみ）。**
同じ深さで同じスイート関数への呼び出しが続くと、引数が違っても1つの項目にまとめます。
ピクセルごとのコールバックでトレースが埋まらないようにするためです。
`repeat` は呼び出しの数で、`text` の末尾に `  ×<repeat>` が付きます。
`args` と `result` は最初の呼び出しのものです。
`last` は最後の呼び出しの `{args, result}` で、最初と違う場合だけ入り、それ以外は `null` です。
間の呼び出しは数えますが、その引数は残しません。

引数を記録するのは `--level args`（既定）のときだけです。
`calls` では `args` が空になり、`commands` では `call` の項目自体がありません。

### 診断オブジェクト

`{"title": "...", "sections": [{"heading": "...", "items": [...]}]}`。

診断が項目の実行場所（`entry`、`inside`、`after`）から始まるときは、その最初のセクションの `heading` が `null` です。
ステップの診断にはこのセクションがなく、`what it caused` から始まります。

各要素は次の2つの形のどちらかです。

- `{"key", "value", "tone", "note"?}`：ラベル付きの値。`tone` は値の判定で、`note` があればその理由です（`"live Handle, 64 B, allocated at #24 in PF_Cmd_RENDER"`、`"null pointer"` など）。
- `{"text", "tone"}`：ラベルのない行。

| `tone` | 意味 |
| ------ | ---- |
| `bad` | バグ。解放後の使用、レンダリングから漏れたワールド、エラーコード |
| `warn` | 怪しい。null ポインター、解放されないメモリ、まだ確保されていない値 |
| `good` | 確認済みで問題なし。生存中のメモリを指す引数、`PF_Err_NONE`、解放されたメモリ |
| `plain` | 情報 |
| `dim` | 前後の項目などの文脈 |

問題だけが欲しいプログラムは、`tone` が `bad` か `warn` の要素を集めれば足ります。

### フラグオブジェクト

`{"bits": 33555456, "names": ["PIX_INDEPENDENT", "DEEP_COLOR_AWARE"]}`。
このバージョンの aexlo が名前を知らないビットは、`names` に16進の文字列で入ります。

## 6. 終了コードとエラー

| 終了コード | 条件 |
| ---------- | ---- |
| 0 | すべてのコマンドが成功した |
| 1 | 失敗したコマンドがあった（一致しない `focus` を含む）、プラグインを読み込めなかった、またはオプションかコマンドが不正だった |
| シグナルによる終了 | プラグインがクラッシュした（下を参照） |

JSON モードの stdout には JSON の行しか出ません。
それ以外はすべて stderr に出ます。

- プラグインが `printf` や `std::cout` で出力したもの。Unix では、セッション中はファイル記述子 1 を stderr に向けています。
- ログと、`<plugin>` がクレートのときの cargo のビルド出力。
- 失敗時の `error: ...` と、最後の `error: N command(s) failed`。

JSON を出力する前に見つかる間違いもあります。
不正なオプションや、`--exec` か `--script` ファイル内の知らないコマンドです。
このときは stderr に `error: ...` が出るだけです。
stdout が空で終了コードが 1 なら、stderr を読んでください。

Windows では、CLI はプラグインの後始末と出力のフラッシュが済んだ時点でプロセスを終了します。
その時点で読み込まれたままの DLL（プラグイン自身のスレッドが参照を持っていると残ります）には、プロセス終了時の detach を実行しません。
スレッドがなくなった後の detach は、停止やクラッシュを起こすためです。
コマンド、setdown、アンロードの最中のクラッシュは、これまでどおりクラッシュとして扱います。
`aexlo` ライブラリーを組み込んだプログラムの終了方法はそのプログラムが決めます。通常どおり終了すれば、この detach が実行されます。

プラグインがクラッシュすると、プロセスごと終了します。終了コードはシグナルによるものです。
それまでに出力したイベントはそのまま有効です。
実行中だったコマンドの `command` 行は出ず、最後の `report` もありません。
（TUI は端末を元に戻し、クラッシュした `PF_Cmd_*` と直前のスイート呼び出しを表示しますが、バッチでは表示しません。）

## 7. 使用例

### シェルと jq

```bash
# 失敗したコマンド
aexlo inspect MyGlow --format json --exec "render; render smart; render legacy" \
  | jq -c 'select(.event=="command" and .ok==false) | {line, error}'

# エラーを返したすべての PF_Cmd_*（成功したコマンドの中のものも含む）
aexlo inspect MyGlow --format json --exec "render" \
  | jq -c 'select(.event=="report") | .errors[]'

# リーク：strict モードの指摘。GLOBAL_SETDOWN の時点で残っていたものも含む
aexlo inspect MyGlow --strict --format json --exec "render; render" \
  | jq 'select(.event=="report") | .findings'

# 診断が見つけた問題だけ
aexlo inspect MyGlow --format json --exec "render legacy; focus ◀ PF_Cmd_RENDER" \
  | jq -c 'select(.event=="focus") | .diagnosis.sections[].items[] | select(.tone=="bad" or .tone=="warn")'
```

### プログラムからセッションを操作する

```python
import json, subprocess

p = subprocess.Popen(
    ["aexlo", "inspect", "build/MyGlow.plugin", "--format", "json", "--script", "-"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

def send(line):
    p.stdin.write(line + "\n")
    p.stdin.flush()
    return json.loads(p.stdout.readline())

hello = json.loads(p.stdout.readline())
if hello["event"] == "error":
    raise SystemExit(hello["error"])

r = send("render")
failed = [c for c in r.get("commands", []) if c["error"]]
if not r["ok"] or failed:
    # ステップを診断する。どの PF_Cmd_* を引き起こし、どこで失敗したか
    print(send(f"focus {r['step']}")["diagnosis"])
    # 失敗したコマンドそのものを診断する
    print(send("focus ◀ PF_Cmd_RENDER")["diagnosis"])

held = send("report")["memory"]
final = send("quit")              # 最後の report が返る
print(final["findings"])          # GLOBAL_SETDOWN で判定したリークも含む
p.wait()
```

送った1行につき、必ず1行が返ります。
`quit` には最後のレポートが返り、`;` で複数のコマンドを書いた行にはコマンドの数だけ行が返ります。

### バグを追い詰める手順

1. `render`（または問題のあるコマンド）を送り、その `command` イベントを読む。
2. `ok` が `false` か、`commands[].error` のどれかが入っていれば、その `step` を `focus` する。続けて、失敗したコマンドを `focus ◀ PF_Cmd_<NAME>` で診断する。
3. 各診断から `bad` と `warn` の要素を集める。要素にアドレスが出てきたら、`focus + alloc Handle 0x…`（または `World`）でその確保を診断できる。一生と、それを引数に取った呼び出しがわかる。
4. 1つだけ変えて（`set`、`size`、`depth`、`render smart`、`strict on`）、1 に戻る。次のレポートをそれ以降だけにしたいなら、先に `clear` する。
5. リークを調べるなら `strict alloc` を有効にし（または `--strict` で起動し）、最後のレポートの `findings` を読む。
