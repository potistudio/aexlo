# playground — aexlo のホスト動作検証

[English](README.md) | 日本語

## 最初の実行

この playground は、ホスト API の動作を計測します。画像処理アプリケーションを作りたい場合は、[はじめに](../docs/getting-started.ja.md)と[サンプル](../examples/README.ja.md)から始めてください。

以下をリポジトリのルートで実行します。After Effects は必要ありません。

```sh
cargo run -p playground -- run --in-process
cargo run -p playground -- report target/probe/trace-aexlo.jsonl
```

`--in-process` はプローブを直接実行します。動的ライブラリの読み込みも含めるには、`cargo run -p playground -- run` を使ってください。トレースは `target/probe/trace-aexlo.jsonl`、プレビューは `target/probe/preview-aexlo.png` に保存されます。次の実行で上書きされるため、比較する前にトレースを別名で保存してください。レポートの `unavailable` は、プローブが取得できなかったスイートを示します。

以下の After Effects 用 `package` の手順は Windows 向けです。インストール可能な macOS バンドルのパッケージ化には、まだ対応していません。

この仕組みは、**aexlo が実際の After Effects ホストと同じように動作するか**という問いに、正確に答えるためのものです。

考え方は、*プラグインを計測器とし、ホストを変数とする*ことです。
`playground/probe` は、PiPL も含めて Rust で実装された、実際に読み込める AE エフェクトプラグインです。**関数、スイート、変数を 1 つずつ**検証します。各チェックはホストのサービスに固定入力を渡し、正確な出力を JSONL トレースの `fact` として記録します。たとえば `sin(0.5)` の最下位ビット、`sprintf` の各変換、rowbytes のレイアウト方針、`blend` の丸め、iterate の呼び出し回数などです。
同じバイナリを実際の AE と aexlo に読み込み、2 つのトレースの差分を取ります。fact は設計上決定的なので、差が出た場合はエミュレーションの動作差であり、シナリオ由来のノイズではありません。ヘッドレスの aexlo と GUI の AE セッションでは、コマンドの順序、タイミング、UI のやり取りなど、プラグインの駆動方法が大きく異なります。これらはすべてコンテキストとしてのみ記録し、既定の比較対象から除外します。

```text
┌───────────────┐         ┌─────────────────┐         trace-ae.jsonl ───┐
│ After Effects │ ──────► │                 │ ───────►                  │  playground diff
├───────────────┤         │  AexloProbe.aex │                           ├─────────────────►
│     aexlo     │ ──────► │    プローブ     │ ───────►                  │  動作差の一覧
└───────────────┘ 読込    └─────────────────┘ 記録    trace-aexlo.jsonl
```

## クイックスタート

```bash
# 1. aexlo 上でプローブをトレース（ビルド、1 フレームのレンダリング、レポート表示）
cargo run -p playground -- run
#    SmartRender 版（後述）は 8、16、32 bpc でレンダリングします。
cargo run -p playground -- run --variant smart

# 2. 同じプローブを実際の After Effects でトレース（Windows）
cargo run -p playground -- ae
#    → target/probe/trace-ae.jsonl

# 3. 比較
cargo run -p playground -- diff target/probe/trace-ae.jsonl target/probe/trace-aexlo.jsonl
```

手順 2 を手作業で行う場合は、`cargo run -p playground -- package` を実行します。`playground/dist/AexloProbe.aex` と `playground/dist/AexloProbeSmart.aex` が出力されるので、どちらかを `C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore\` などのプラグインフォルダーにコピーします。レイヤーに "Effect > aexlo > Aexlo Probe"（または "Aexlo Probe Smart"）を適用し、フレームをレンダリングします。トレースは `%TEMP%\aexlo-probe\` に保存されます。正確なパスはエフェクトの About ダイアログに表示されます。

`diff` は動作に差がある場合に非ゼロで終了します。実際の AE から取得した参照トレースをリポジトリに追加すれば、CI の合否判定にも使えます。

## コマンド

| コマンド | 動作 |
| --- | --- |
| `run [--release] [--in-process] [--variant legacy\|smart] [--trace <file>] [--input <png>]` | プローブをビルドして aexlo 上で読み込み、すべてのパラメーターを既定値から変更して `fixtures/input.png` をレンダリングします。`target/probe/trace-aexlo.jsonl` とプレビュー PNG を保存し、レポートを表示します。`--variant smart` を指定すると SmartRender 版のエフェクトを 8、16、32 bpc で 1 回ずつレンダリングし、`trace-aexlo-smart.jsonl` と `preview-aexlo-smart.png` を保存します。`--in-process` は `dlopen` を使わずにエントリーポイントを実行するため、プローブ内のブレークポイントを使えます。ただし legacy 版のビルドだけが対象です。 |
| `report <trace.jsonl>` | 1 つのトレースを読みやすく要約します（ホスト識別情報、コマンド・エラー表、スイートの利用可否、コールバックの結果、world のレイアウト、パラメーター値）。 |
| `diff <a> <b> [--all]` | 2 つのトレースをキーごとに比較します。既定では fact、スイートの利用可否、コールバックの有無を比較し、`--all` ではコンテキスト（コマンド回数、レンダリングシナリオ、タイミング）も比較します。 |
| `package [--debug] [--to <dir>]` | 2 つの版をリリースビルドし、`playground/dist/AexloProbe.aex` と `playground/dist/AexloProbeSmart.aex` に出力します。 |
| `pipl [--release] [--variant legacy\|smart]` | ビルドした DLL から PiPL リソースを再解析し、実際の AE が受け入れることを事前確認します（Windows）。out flags はフラグ名で表示します。 |
| `ae [--ae <AfterFX.exe>] [--trace <file>] [--frames <n>] [--input <png>] [--plugin-dir <dir>] [--timeout <secs>] [--no-build] [--variant legacy\|smart]` | プローブをパッケージ化して After Effects にインストールし、`AfterFX.exe -r` で適用とレンダリングを行い、トレースを `target/probe/trace-ae.jsonl` に保存します（Windows）。`--variant smart` を指定すると、SmartRender 版のエフェクトをプロジェクトのビット深度 8、16、32 bpc で順にレンダリングし、`trace-ae-smart.jsonl` に保存します。詳しくは[実際の After Effects で記録する](#実際の-after-effects-で記録する)を参照してください。 |

## 実際の After Effects で記録する

`playground ae` は、手作業の手順を GUI を使わずに実行します。

1. リリースビルドしたプローブを `<After Effects>\Support Files\Plug-ins\aexlo\AexloProbe.aex` にコピーします。同じ内容のファイルがすでにある場合はコピーしません。`--plugin-dir` で別のフォルダーを指定できます。
2. [`harness/ae/trace.jsx`](harness/ae/trace.jsx) から `target/probe/ae-run/trace.jsx` を書き出し、`AfterFX.exe -r` に渡します。スクリプトは `fixtures/input.png`（または `--input`）を読み込み、プローブを適用し、`run` が aexlo 上で設定するのと同じパラメーター値を設定します。続いて `saveFrameToPng` で `--frames` 枚のフレームをレンダリングし、追加したものをすべて削除します。結果は `target/probe/ae-run/result.json` に書き出されます。
3. トレースを保存し、レポートを表示します。

使用する After Effects は、`--ae`、環境変数 `AEXLO_AE_EXE`、起動中のインスタンス、`C:\Program Files\Adobe\Adobe After Effects <year>` のうち最も新しいもの、の順に決まります。ベータ版は明示的に指定した場合だけ使います。

After Effects がすでに起動しているかどうかで、動作が変わります。

| After Effects | 動作 | トレース |
| --- | --- | --- |
| 起動していない | 新しいインスタンスを、`AEXLO_PROBE_TRACE` を設定した状態で `AfterFX.exe -r` から起動します。スクリプトは保存せずに終了させます。 | プラグインの読み込みから `GLOBAL_SETDOWN` までの、1 回分のセッションです。 |
| 起動している | `AfterFX.exe -r` がスクリプトを起動中のインスタンスに渡します。スクリプトは開いているプロジェクトの中で作業し、元の状態に戻します。 | そのインスタンスの `%TEMP%\aexlo-probe\trace-*-pid<pid>.jsonl` をコピーしたものです。それまでのセッション全体が含まれます。 |

After Effects はプラグインを起動時にしか読み込みません。起動中のインスタンスが読み込んだプローブがビルドより古い場合、`ae` は After Effects を閉じるように求めて止まります。閉じてからもう一度実行すると、新しいインスタンスを起動します。

`--timeout` の秒数（既定は 600 秒）以内に結果が届かない場合は、After Effects のウィンドウでダイアログが待っている可能性があります。クラッシュからの復元など、起動時のダイアログは抑制されません。

## fact：検証の単位

`probe/src/checks.rs` は GLOBAL_SETUP 時に実行されます。各チェックはパニックから保護されているため、1 つのホストサービスが壊れていても、残りの計測を続けられます。すべての fact は `固定入力 → 正確な出力` です。

- **変数**：`appl_id`、仕様バージョン、品質、in_flags など、`PF_InData` から直接読み取る静的なホスト識別情報。
- **ANSI コールバック**：17 個すべての数学関数を f64 の全精度で計測します（libm の最下位ビットの差も有効な測定結果です）。`sprintf` の書式マトリクス（幅、精度、配置、ゼロ埋め、`%d/%u/%x/%f/%e/%g/%s/%c/%%`）と `strcpy` の動作も検証します。
- **色のコールバック**：固定のピクセルに対する `RGBtoHLS` / `HLStoRGB`。
- **ホストのハンドル**：作成・ロック・書き込み・読み取り・アンロック・リサイズ・破棄のサイクル。各段階の報告サイズと、リサイズで内容が保持されるかを検証します。
- **world とピクセル処理**：*ホストを通じて* world を割り当てます（`utils.new_world`、利用できない場合は World Suite 2）。rowbytes とレイアウトの方針、割り当て時のクリア、`fill`（全体・部分矩形、境界を含むか）、`copy`、比率 0.5 の `blend`（丸め方向）、`premultiply` の丸め、`iterate` の呼び出し回数と結果のハッシュを検証します。
- **カーネル**：`gaussian_kernel` の直径と正規化された重み。
- **スイート関数**：利用可否の一覧より一段深く調べます。同じ固定入力を `PF ANSI Suite`、`PF Handle Suite`、`PF World Suite v2`（ピクセル形式ごとの割り当てと `PF_GetPixelFormat` の応答）、`PF Iterate8 Suite v2` に渡します。

トレースには fact に加えて**コンテキスト**も記録されます。スイートの利用可否（約 45 スイート × 各バージョン）と `PF_UtilCallbacks` の有無は決定的なので、どちらも既定の比較対象です。一方、コマンドの流れ、`PF_InData` のスナップショット、レンダリング時のパラメーターと world のハッシュは参考情報としてのみ記録し、既定の差分から除外します。

プローブは、パラメーターに応じた決定的なテストパターン（浮動小数点スライダー、チェックボックス、ポップアップ、色、角度、ポイント）もレンダリングするため、AE 内の画面でもパラメーターの受け渡しを確認できます。

## SmartRender 版

out flags は PiPL と一致している必要があるため、SmartRender 経路は実行時の切り替えではなく、2 つ目のエフェクトで計測します。`probe` の `smart` フィーチャーを有効にすると、"Aexlo Probe Smart"（マッチ名 "AEXLO Probe Smart"、ファイル名 `AexloProbeSmart.aex`）がビルドされます。このエフェクトは `PF_OutFlag2_SUPPORTS_SMART_RENDER`、`PF_OutFlag_DEEP_COLOR_AWARE`、`PF_OutFlag2_FLOAT_COLOR_AWARE`、`PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` を宣言します。legacy 版と同じ GLOBAL_SETUP のチェックをすべて実行し、同じテストパターンを 8、16、32 bpc でレンダリングします。ハーネスはこの版を `<target>/probe-smart/` にビルドするため、2 つのビルドが互いを上書きすることはありません。2 つのエフェクトは AE に並べてインストールできます。

`probe/src/smart.rs` は、プローブ自身が組み立てた要求に対するホストの応答を fact として記録します。fact はビット深度ごとに 1 組（`smart.8.*`、`smart.16.*`、`smart.32.*`）になります。`cargo run -p playground -- ae --variant smart` を実行すると、1 回のセッションでプロジェクトのビット深度を 8、16、32 bpc に切り替えながらレンダリングし、3 組すべてを 1 つのトレースに記録します。手動で行う場合は、`package` で作成した `AexloProbeSmart.aex` をインストールし、各深度でフレームをレンダリングしてください。

- 10x9 ピクセル以上のどのレイヤーにも収まる固定矩形、空の矩形、パラメーター番号 99 での `checkout_layer`。
- レイヤーの各辺から 4 ピクセルはみ出す `checkout_layer`。レイヤーサイズが打ち消されるように、結果はレイヤーの `max_result_rect` からの差で記録します。
- 固定矩形でチェックアウトした world のレイアウトと、その ID に対する `checkin_layer_pixels`。
- 固定バイト列を渡した `GuidMixInPtr` と、`pre_render_data` が SMART_RENDER まで壊れずに届くかどうか。
- チェックアウトした入力と出力の world に対する `PF_GetPixelFormat` の応答と world flags。

プローブは、SDK が動作を定めている呼び出しだけを行います。同じ ID の 2 回目のチェックイン、プリレンダーでチェックアウトしていない ID のピクセル取得、`GuidMixInPtr` への null バッファーの受け渡しは行いません。ホストがこれらで異常終了する可能性があるためです。ピクセルの書き込みとハッシュの計算は、`PF_GetPixelFormat` が既知のピクセル形式を返し、かつ各行が `rowbytes` に収まる場合だけ行います。

fact の値がそろうのは、エフェクトを 10x9 ピクセル以上の通常のフッテージレイヤーか平面レイヤーに適用し、レイヤーを広げる処理がない場合だけです。

シナリオに依存する値はすべてコンテキストとして扱い、既定の差分から除外します。対象は `PF_PreRenderExtra` の入力（出力要求、フィールド、チャンネルマスク、ビット深度、GPU フレームワーク）、チェックアウト結果の全体（`par`、`solid`、`ref_width`/`ref_height`）、プローブが返す `PF_PreRenderOutput`、レンダリング時の入力と出力の world のレイアウトとハッシュです。`report` ではこれらを `smart/` のキーで表示します。

GPU：GPU のあるホストは `PF_Cmd_GPU_DEVICE_SETUP` を送ります。プローブはフレームワーク、デバイス番号、GPU Device Suite が返すデバイス数とデバイス情報を、`gpu/` のコンテキストとして記録します。デバイスのポインターは null かどうかだけを記録し、ポインター先は読み取りません。プリレンダーでは `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE` を設定しないため、すべてのフレームは CPU でレンダリングされます。プローブは GPU ではレンダリングしません。

## トレースの保存先

1. `AEXLO_PROBE_TRACE`：ファイルの完全なパス（ハーネスが設定）。
2. `AEXLO_PROBE_DIR`：ディレクトリを指定し、ファイル名は自動生成。
3. どちらも指定しなければ `%TEMP%/aexlo-probe/trace-<time>-pid<pid>.jsonl`（SmartRender 版は `trace-smart-<time>-pid<pid>.jsonl`）。

ハーネスは、`CARGO_TARGET_DIR` が設定されていればそこに、設定されていなければ `target/` にビルドします。どちらの場合も、トレースとプレビューは `<workspace>/target/probe/` に保存されます。

各トレースの先頭には `trace_schema` が記録されます。この値は、イベントや fact の名前が変わるたびに変わります。`diff` はこの値も比較するため、スキーマの異なるトレースは fact の欠落ではなく差分として表示されます。

## 構成

```text
playground/
  probe/     Rust cdylib → .aex。build.rs が `pipl` クレートを使って
             PiPL リソースを埋め込みます。--in-process 用の rlib にもなります。
  harness/   `playground` CLI：run / report / diff / package / pipl。
  dist/      パッケージ化した AexloProbe.aex と AexloProbeSmart.aex
             （Git 管理対象外のビルド出力）。
```

## 補足

- プローブは aexlo と同じ `after-effects 0.4.0` に依存していますが、高水準のラッパーではなく、再エクスポートされた生の `sys` API を意図的に使っています。プローブはホストを計測するため、間にラッパーを挟むとラッパーを計測することになってしまいます。同じクレートバージョンを使うことで、ABI の両側で構造体のレイアウトを一致させています。
- legacy 版は `PF_Cmd_RENDER` を計測します。SmartRender 版は `PF_Cmd_SMART_PRE_RENDER`、`PF_Cmd_SMART_RENDER`、GPU デバイスのセットアップを計測しますが、GPU でのレンダリング（`PF_Cmd_SMART_RENDER_GPU`）は対象外です。
- macOS のパッケージ化（`.plugin` バンドルと rsrc PiPL）はまだ実装していません。プローブ自体はプラットフォームに依存しません。
