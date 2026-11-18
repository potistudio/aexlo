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
`SDK_Noise` のような拡張子なしの名前は、プラットフォームの拡張子を付けて探します。

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

| | `--exec "<cmds>"` | `--script <file>` | `--script -` |
| - | - | - | - |
| コマンドの取得元 | 引数（`;` 区切り） | ファイル（1行1コマンド、`;` も可） | 標準入力（1行1コマンド、`;` も可） |
| `#` で始まる行 | — | コメントとして無視 | コメントとして無視 |
| パースするタイミング | プラグインの読み込み前に全部 | プラグインの読み込み前に全部 | 1行届くたび |
| 知らないコマンドがあった場合 | 読み込み前に全体が止まる（終了コード 1） | 読み込み前に全体が止まる（終了コード 1） | その行だけ失敗（`ok: false`）。セッションは続く |
| 終了するタイミング | 最後のコマンドの後 | 最後のコマンドの後 | `quit` を受け取るか、stdin が閉じたとき |

`--script -` は、1行に答えてから次の行を読みます。
そのため、プログラムは `render` を送って結果を読み、それを見てから `focus #29` や `set Radius 10` を送るか決められます。
どれも、読み込んだ同じプラグインに対して実行されます。
ほかの2つのモードは、手順を最後まで続けて実行します。

どのモードでも、最後に分析（`report`）をもう一度出力します。

## 2. オプション

| オプション | 意味 | 既定値 |
| ---------- | ---- | ------ |
| `-i`, `--input <png>` | 入力レイヤー | グラデーション |
| `--size <W>x<H>` | 既定の入力と出力のサイズ | 640x360 |
| `--depth <8\|16\|32>` | プロジェクトのビット深度（チャンネルあたり） | 8 |
| `--level <args\|calls\|commands>` | 記録する内容。`args` はスイート呼び出しを引数付きで記録、`calls` は引数なしで記録、`commands` は `PF_Cmd_*` だけを記録 | `args` |
| `--strict` | 最初のコマンドから strict チェック（ポイズン、ガードバンド、確保とチェックアウトの追跡）をすべて有効にする | 無効 |
| `--serial` | iterate コールバックをプラグインのスレッドで実行し、その中のスイート呼び出しも記録する | 無効 |
| `--exec <cmds>` | バッチ。`;` 区切りのコマンドを実行する | — |
| `--script <file\|->` | ファイルからのバッチ。`-` なら stdin からのセッション | — |
| `--format <human\|json>` | バッチの出力形式。テキストか [JSON の行](#4-イベント) | `human` |
| `--trace` | バッチで、レポートにトレース全体も含める | 無効 |

`--format` と `--trace` には `--exec` か `--script` が必要です。

## 3. コマンド

TUI のプロンプトと同じコマンドです（先頭の `:` は省略できます）。
プラグインのコマンドはプラグインのスレッドで実行され、「ステップ」の下に記録されます。
ステップとは、送ったコマンド行のことです。

| コマンド | 動作 |
| -------- | ---- |
| `about` | `PF_Cmd_ABOUT`。メッセージはプラグインのテキスト |
| `render [auto\|smart\|legacy\|gpu]` | 指定した経路でフレームをレンダリングする（`auto` は GPU、smart、legacy の順に選ぶ） |
| `set <param> <value>` | パラメーターを設定する。`<param>` はインデックス、`#インデックス`、名前のどれか。値には空白を含められる（`set Center 10, 20`） |
| `changed <param>` | `PF_Cmd_USER_CHANGED_PARAM`（プラグインが求めれば `UPDATE_PARAMS_UI` も） |
| `updateui` | `PF_Cmd_UPDATE_PARAMS_UI` |
| `dialog` | `PF_Cmd_DO_DIALOG`（Options ボタン） |
| `time <frame> [fps]` | フレームを移動する（fps の既定は 30） |
| `size <w> <h>` | 以降のレンダリングの出力サイズ |
| `input <png\|black\|white\|gradient> [w h]` | 入力レイヤーを差し替える（ビット深度は維持） |
| `depth <8\|16\|32>` | 入力（プロジェクト）のビット深度を変換する |
| `reload` | プラグインを破棄して読み込み直す。クレートなら先に再ビルドする |
| `level <commands\|calls\|args>` | 記録する内容を変える |
| `strict <on\|off\|poison,guard,alloc,checkout>` | strict チェックを変える。指摘はレポートに出る |
| `serial <on\|off>` | `--serial` と同じ |
| `save <path>` | 出力フレームを書き出す（`.png`、`.exr`） |
| `focus <text\|#seq>` | トレースの1項目を診断する。`<text>` を含む最新の項目（大文字小文字は区別しない）か、番号 `#seq` の項目 |
| `report` | その時点の分析を出力する（バッチのみ） |
| `clear` | トレースと集計を消す。項目の番号は 1 からやり直しになる |
| `help` | この一覧を出力する |
| `quit` | セッションを終える（バッチではコマンドの読み込みをやめる） |
| `mem ...`、`find ...` | TUI の表示を変えるだけなので、バッチでは飛ばす |

## 4. イベント

`--format json` では、stdout の各行が1つの JSON オブジェクトです。
`event` フィールドが行の種類を表します。
stdout を1行ずつ読み、`event` で処理を分けてください。
順序は必ず次のとおりです。

```text
loaded
command | focus | report | help      （送ったコマンド1つにつき1行）
report                              （最後）
```

プラグインが見つからない、または読み込めない場合は、`error` の1行だけになります。

時間はマイクロ秒（`*_us`）です。
アドレスは16進の文字列（`"0x1238…"`）で、`focus` や `mem` にそのまま渡せる形です。

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

| フィールド | 型 | |
| ---------- | -- | - |
| `line` | string | 送ったコマンド |
| `ok` | bool | 成功したか |
| `message` | string | `ok` のとき。出力された内容（`rendered 16x8 (Smart)`、`#1 = 7`、ABOUT のテキストなど） |
| `error` | string | `ok` でないとき。失敗の理由 |
| `elapsed_us` | number | `ok` のとき。かかった時間 |
| `step` | string \| null | このステップの項目番号（`"#10"` など）。`focus` に渡すと、ステップが引き起こしたことを診断できる |
| `commands` | array | ステップが送った `PF_Cmd_*`（いちばん外側のものだけ）。各要素は `{command, elapsed_us, error}` で、`error` は[エラーオブジェクト](#エラーオブジェクト)か `null` |
| `suite_calls` | number | ステップ中のスイート呼び出しの数 |
| `allocations`、`frees` | number | ステップ中に確保・解放したハンドルとワールドの数 |
| `skipped` | string | `mem` と `find` のときだけ、上のフィールドの代わりに付く |

パースできなかったコマンド（`--script -` のみ）は、`line`、`ok: false`、`error` だけになります。

`ok: true` でも、エラーを返した `PF_Cmd_*` を含むことがあります。
たとえば `SMART_PRE_RENDER` に失敗したプラグインが、別の経路にフォールバックした場合です。
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

| フィールド | 型 | |
| ---------- | -- | - |
| `line`、`query` | string | コマンドと、`focus` の後ろのテキストまたは `#seq` |
| `ok` | bool | 一致する項目がなければ `false` で、`error` に理由が入る。失敗したコマンドとして数える |
| `entry` | object | 一致した[項目](#項目オブジェクト) |
| `diagnosis` | object | `{title, sections}`。[診断オブジェクト](#診断オブジェクト)を参照 |

診断の内容は、項目の種類によって変わります。

- **スイート呼び出し：** 引数と戻り値。アドレスは確保の履歴と照らし合わせて判定します（生存中、解放済み、まだ確保されていない）。呼び出しの結果ホストが確保したメモリとその解放状況、実行中だったコマンド、その関数が呼ばれた回数も示します。
- **`PF_Cmd_*`（開始・終了）：** かかった時間、エラーとその手がかり、中で実行されたこと、確保してまだ保持しているもの、エラー直前の呼び出し。
- **確保・解放：** そのアドレスの一生、現在生存しているか、それを引数に取った呼び出し。
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
     {"text":"never freed: still held (Memory tab shows its bytes)","tone":"warn"}]},
   ...]}}
```

### `report`

それまでに記録したすべての分析です。
`report` を送ったときと、最後に1回出力されます。

| フィールド | 型 | |
| ---------- | -- | - |
| `failed` | number | それまでに失敗したコマンドの数 |
| `loaded` | bool | プラグインが読み込まれているか。`false`（`reload` の失敗）のときは、`plugin` 以下のフィールドがない |
| `trace` | object | `{entries, dropped}`。`--trace` 付きなら、全[項目](#項目オブジェクト)の配列 `lines` も入る。トレースは最新 200,000 項目まで保持し、それより古いものは `dropped` に数える |
| `commands` | array | `PF_Cmd_*` ごとの集計（合計時間の長い順）。`{command, count, errors, max_us, total_us}` |
| `suite_calls` | array | スイート関数ごとの集計（呼ばれた回数の多い順）。`{suite, function, count, commands}`。`commands` は呼び出し元の `PF_Cmd_*` |
| `errors` | array | エラーを返したすべての `PF_Cmd_*`。`{seq, command, error}` |
| `plugin` | object | `path`、`smart_render`、`gpu`、`custom_ui`、`options_button`、`pixel_formats`、`out_flags`、`out_flags2`（[フラグオブジェクト](#フラグオブジェクト)） |
| `state` | object | `global_data`、`sequence_data`（アドレスか `null`）。`time` `{value, step, scale}`。`input` と `output` `{width, height, bpc}`。`last_out_flags`、`last_out_flags2`、`last_message`（直前のコマンドが `PF_OutData` に残した値）。`output_mean`（RGBA、0..1）。`unwritten_pixels`（`strict poison` のとき。それ以外は `null`）。`parallel_iterate`。`strict` `{poison, guard, alloc, checkout}` |
| `params` | array | `{index, name, kind, value, detail, hidden, disabled}`。`detail` は範囲かポップアップの選択肢 |
| `memory` | array | 生存中のハンドルとワールド。`{kind, address, bytes, allocated_in, role, head}`。`role` は `global_data`、`sequence_data`、`null` のどれか。`head` は先頭 64 バイトの16進 |
| `findings` | string の配列 | strict モードの指摘（古い順）。ガードバンドの上書き（範囲外書き込み）、ハンドルやワールドのリーク、チェックアウトの違反 |

リークを探すには `memory` を見ます。
`role` が `null` で `allocated_in` がレンダリングのコマンドなら、そのレンダリングが終わった後も残っています。

### `help`

`{"event":"help","text":"..."}`。コマンド一覧のテキストです。

### `error`

`{"event":"error","error":"no plugin at nope.dylib"}`。
プラグインが見つからない、ビルドできない、または読み込めませんでした。
ほかには何も出力されず、終了コードは 1 です。

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
| `repeat` | この項目にまとめられた、連続する同じ呼び出しの数 |
| `text` | TUI やテキスト出力での表示 |
| `type` | 下の種類のどれか。種類ごとに追加のフィールドがある |

| `type` | 追加のフィールド |
| ------ | ---------------- |
| `step` | `line`：送ったコマンド |
| `begin` | `command` |
| `end` | `command`、`elapsed_us`、`error`（エラーオブジェクトか `null`） |
| `call` | `suite`、`function`、`args`（`[{name, value}]`）、`result`（文字列か `null`）、`last`（まとめた最後の呼び出しの `{args, result}`。最初と違う場合だけ入り、それ以外は `null`） |
| `alloc`、`free` | `kind`（`Handle`、`World` など）、`address`、`bytes` |
| `checkout` | `kind`、`id` |

引数を記録するのは `--level args`（既定）のときだけです。
それより下のレベルでは、`args` は空になります。

### 診断オブジェクト

`{"title": "...", "sections": [{"heading": "...", "items": [...]}]}`。
最初のセクションが項目の実行場所（`entry`、`inside`、`after`）を表すときは、その `heading` は `null` です。
各要素は次の2つの形のどちらかです。

- `{"key", "value", "tone", "note"?}`：ラベル付きの値。`note` は分析がその値をどう判断したかの説明です（`"live Handle, 64 B, allocated at #24 in PF_Cmd_RENDER"`、`"null pointer"` など）。
- `{"text", "tone"}`：ラベルのない行。

`tone` が判定結果を表します。

| `tone` | 意味 |
| ------ | ---- |
| `bad` | バグ。解放後の使用、レンダリングから漏れたワールド、エラーコード |
| `warn` | 怪しい。null ポインター、保持されたままのメモリ、まだ確保されていない値 |
| `good` | 確認済みで問題なし。生存中の確保、`PF_Err_NONE` |
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

JSON モードの stdout には JSON の行しか出ません。
それ以外はすべて stderr に出ます。

- プラグインが `printf` や `std::cout` で出力したもの。Unix では、セッション中はファイル記述子 1 を stderr に向けています。
- ログと、`<plugin>` がクレートのときの cargo のビルド出力。
- 最後の `error: N command(s) failed`。

JSON を出力する前に見つかる間違いもあります。
不正なオプションや、`--exec` か `--script` ファイル内の知らないコマンドです。
このときは stderr に `error: ...` が出るだけです。
stdout が空で終了コードが 1 なら、stderr を読んでください。

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

# 2回レンダリングした後もプラグインが保持しているもの
aexlo inspect MyGlow --format json --exec "render; render" \
  | jq 'select(.event=="report") | .memory'

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

leaks = send("report")["memory"]
send("quit")                      # 最後の report が返る
p.wait()
```

コマンド1つにつき、必ず1行が返ります。
そのため `send` は、1行書くたびに1行読めば足ります。
例外は1行に `;` で複数のコマンドを書いた場合で、そのときはコマンドの数だけ行が返ります。

### バグを追い詰める手順

1. `render`（または問題のあるコマンド）を送り、その `command` イベントを読む。
2. `ok` が `false` か、`commands[].error` のどれかが入っていれば、その `step` を `focus` する。続けて、失敗したコマンドを `focus ◀ PF_Cmd_<NAME>` で診断する。
3. 各診断から `bad` と `warn` の要素を集める。要素にアドレスが出てきたら、`focus alloc Handle 0x…`（または `alloc World 0x…`）でその確保自体を診断できる。一生と、それを引数に取った呼び出しがわかる。`focus 0x…` だけだと、そのアドレスに触れた最新の項目が種類を問わず選ばれる。
4. 1つだけ変えて（`set`、`size`、`depth`、`render smart`、`strict on`）、1 に戻る。次のレポートをそれ以降だけにしたいなら、先に `clear` する。
