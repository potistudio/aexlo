# aexlo ツールキット — 仕様書

[English](toolkit.md) | 日本語

状態：実装済み（M1–M7） · 対象：aexlo 0.1

## 1. 目的

aexlo は、すでに実際の After Effects プラグインを AE の外で実行するホストです。この仕様では、そのランタイムを**プラグイン作者向けの開発ツールキット**に発展させます。「この条件でこのプラグインをレンダリングする」という宣言的な記述を 1 つ用意し、**テスト**、**ベンチマーク**、**プレビュー**に共通して使います。さらに、After Effects 自体では提供できないホスト側のチェックも加えます。

現状では各機能が存在していますが、入力形式が共通化されていません。

| 用途 | 既存の機能 | 不足している点 |
| --- | --- | --- |
| テスト | `tests/e2e`、`#[aexlo::preview]`、`render_one` によるクラッシュ分離 | ピクセルのインデックス処理が手書き、golden がない、分離は e2e のみ |
| ベンチマーク | `aexlo-bench`、`aexlo bench` | 環境変数とフラグで設定方法が異なる、ベースライン比較がない、8 ビットのみ |
| プレビュー | `aexlo dev`、`dev --bin`、`preview --web`、`studio` | 「入力、パラメーター、サイズ、時間」の指定方法が 4 通りある |

### 目標

- Rust **または** C++ のプラグイン作者がマニフェストを記述し、同じプリセットに対して `aexlo test`、`aexlo bench`、`aexlo dev` / `preview`、`aexlo check` を実行できること。
- プラグインがクラッシュしても、1 つのプリセットだけが失敗し、実行全体は続くこと。
- ホスト側の計測機能（strict モード）により、AE では隠れるバグを報告すること。範囲外の書き込み、未書き込みの出力ピクセル、ハンドルのリーク、不均衡なチェックアウトが対象です。
- 8、16、32 bpc をすべて正式に扱うこと。

### 対象外

- AE アプリケーション自体のエミュレーション（プロジェクト、`.ffx` プリセット、UI パネル）。aexlo は引き続きプラグインインスタンスのホストです。
- 既定での AE とのピクセル単位の完全一致。一致検証は AE 由来の golden（§8.4）を使って明示的に有効化します。
- Linux 用プラグイン（AE プラグインの対象は Windows と macOS です）。

## 2. 用語

| 用語 | 意味 |
| --- | --- |
| **manifest（マニフェスト）** | `aexlo.toml`：読み込むプラグインと、そのプリセット。 |
| **preset（プリセット）** | 名前付きの実行条件。入力、サイズ、時間、パラメーター、レンダリング経路、深度、期待結果を含みます。 |
| **variant（バリアント）** | プリセットのマトリクスの軸を展開した、具体的な 1 つの組み合わせ。 |
| **run（実行）** | バリアントを 1 回実行すること。`Frame`、`Trace`、`Timing` を生成します。 |
| **golden（参照画像）** | バリアントの比較対象として保存した参照フレーム。 |
| **check（チェック）** | 実行結果に対して検証する、ホスト側の不変条件（§7）。 |
| **baseline（ベースライン）** | 後続の実行との比較用に保存したベンチマークの計測値。 |

## 3. アーキテクチャ

```text
                    aexlo.toml（プリセット）
          ┌──────────────┬──────────────┬──────────────┐
          ▼              ▼              ▼              ▼
     aexlo test     aexlo bench    aexlo dev/preview  aexlo check
          └──────────────┴──────┬───────┴──────────────┘
                                ▼
                         aexlo-harness
             preset · run（in-process | worker）· frame · checks · golden
                                ▼
                    aexlo（ホスト）+ Observer / Strict フック
```

### 3.1 クレート

| クレート | 役割 | 状態 |
| --- | --- | --- |
| `aexlo` | ホスト。観測と設定の API（§5）だけを追加し、ツールのロジックは持ちません。 | 既存 |
| `aexlo-macros` | `#[aexlo::preview]` と新しい `#[aexlo::test]`。`aexlo-harness` のパスに展開します。 | 既存 |
| `aexlo-harness` | `preset`（マニフェストのモデル、`inherits`、マトリクスの展開、適用）、`run`（プロセス内とワーカー）、`frame`、`check`、`golden`、`bench` の計測ループ。 | 新規 |
| `aexlo-test` | Rust プラグインクレートに追加する唯一の開発用依存関係。マクロと `Frame` のアサーションを再エクスポートします。 | 新規 |
| `aexlo-bench` | criterion のベンチマーク。`aexlo_harness::bench` 上の薄い層になります。 | 既存 |
| `aexlo-cli` | `aexlo` バイナリ。`test`、`bench`、`dev`、`preview`、`check`、`presets`、`worker` と、既存の `render` / `about` / `params` / `view`。 | 既存 |

依存の向きは厳密に下方向です。`cli → harness → aexlo`、`aexlo-test → harness`、`aexlo-bench → harness` となり、`harness` より下の層はマニフェストを扱いません。

### 3.2 コアとの境界の原則

`aexlo` は**仕組み**（イベント、割り当て方針、深度別 I/O）を公開し、ハーネスは**運用上の方針**（失敗の判定、許容誤差、ファイル）を実装します。
`crates/aexlo/src/preview.rs`（ビューアーのロック、PNG 書き込み、`AEXLO_PREVIEW`）はこの原則に反するため、`aexlo_harness::preview` へ移動します（§13）。

## 4. マニフェスト：`aexlo.toml`

`--manifest <path>` で指定します。指定がなければ、現在のディレクトリから親方向へ探索して、最も近い `aexlo.toml` を使います。マニフェスト内の相対パスは、マニフェストのディレクトリを基準に解決します。

### 4.1 例

```toml
[plugin]
# `artifact` または `crate` のいずれかを指定。
artifact = { macos = "build/MyGlow.plugin", windows = "build/MyGlow.aex" }
# crate = "."            # `aexlo dev --bin` と同様に cargo build で cdylib をビルド
# entry = "EffectMain"   # プロセス内で使用するエントリーシンボル（Rust プラグインのみ）

[defaults]
size   = [1920, 1080]
depth  = [8, 16, 32]
render = ["smart", "gpu"]
checks = ["finite", "coverage", "deterministic"]

[[preset]]
name   = "base"
hidden = true
input  = { generate = "gradient" }

[[preset]]
name     = "dot_glow"
inherits = "base"
input    = { generate = "dot", at = [320, 240], radius = 2 }
size     = [640, 480]
time     = { frame = 12, fps = 24 }
params   = { Radius = 100.0, Mode = "Screen", Center = [320, 240] }
golden   = { tolerance = { max_abs = 0.004 } }

[[preset]]
name     = "radius_sweep"
inherits = "dot_glow"
params   = { Radius = { sweep = [10, 100, 1000] } }
golden   = false
bench    = { samples = 30, warmup = 5 }
```

### 4.2 `[plugin]`

| キー | 型 | 意味 |
| --- | --- | --- |
| `artifact` | string \| `{ macos, windows }` | ビルド済みのプラグイン。単独の文字列は現在のプラットフォームに適用します。 |
| `crate` | string | クレートのディレクトリ。読み込み前に `cargo build` でビルドします。 |
| `entry` | string、既定は `"EffectMain"` | プロセス内で実行する際のエントリーシンボル（§6.1）。 |

`artifact` / `crate` のいずれか一方だけが必要です。

### 4.3 プリセットのフィールド

各フィールドは `[defaults]` またはプリセットに記述できます。

| キー | 型 | 既定値 | 意味 |
| --- | --- | --- | --- |
| `name` | string、`[a-z0-9_]+` | 必須（プリセットのみ） | 一意の ID。 |
| `hidden` | bool | `false` | `inherits` の基底。実行対象にはなりません。 |
| `inherits` | string \| [string] | なし | §4.4。 |
| `input` | 入力仕様 | `{ generate = "gradient" }` | §4.5。 |
| `size` | [w, h] | 入力サイズ | レンダリングサイズ（`set_render_size`）。`input.fit = "none"` でなければ入力をこのサイズに拡大縮小します。 |
| `time` | `{ frame, fps }` | `{ frame = 0, fps = 24 }` | §4.7。 |
| `params` | table | なし | §4.6。 |
| `layers` | table：パラメーター → 入力仕様 | なし | レイヤーパラメーター（`set_layer_param`）。 |
| `render` | mode \| [mode] | `"auto"` | `auto`（`render_frame`）、`legacy`、`smart`、`gpu`。マトリクスの軸になります。 |
| `depth` | 8 \| 16 \| 32 \| [..] | `8` | マトリクスの軸になります。 |
| `iterate` | `"parallel"` \| `"serial"` | `"parallel"` | `set_parallel_iterate`。 |
| `checks` | [check id] | §7 の既定値 | マージせず置き換えます。`"+id"` / `"-id"` で継承した一覧を調整します。 |
| `strict` | bool \| [strict feature] | `false` | §5.3。 |
| `golden` | golden 仕様 \| `false` | `{}`（有効） | §8。 |
| `bench` | `{ samples, warmup }` \| `false` | `{ samples = 30, warmup = 5 }` | §10。 |
| `timeout` | 秒 | `30` | 実行ごと（§6.3）。 |
| `plugin` | `[plugin]` テーブル | `[plugin]` | このプリセットで別のプラグインを実行します（マージせず置き換え）。1 つのマニフェストで複数のプラグイン、たとえばフィクスチャのマトリクスを扱えます。 |

### 4.4 継承

CMakePresets と同じ意味論に従います。

- プリセット自身のキーが継承したキーを上書きします。テーブル（`params`、`layers`、`input`、`golden`、`bench`、`time`）はキーごとにマージし、配列とスカラーは置き換えます。
- 複数の親がある場合は、**先に指定した親が優先**されます。
- `[defaults]` はすべての継承チェーンの暗黙のルートです。
- 循環、存在しない親、別のマニフェスト内のプリセットからの継承はエラーです。

### 4.5 入力仕様

```toml
input = { path = "plates/street.png" }                        # PNG（8/16）または EXR（32）
input = { generate = "gradient" }
input = { generate = "solid", color = [0.2, 0.4, 0.8, 1.0] }
input = { generate = "checker", cell = 32 }
input = { generate = "dot", at = [x, y], radius = 2 }
input = { generate = "noise", seed = 7 }
```

`fit = "stretch" | "none"`（既定は `stretch`）。生成入力はバリアントの深度で直接生成し、ファイル入力はその深度に変換します。ジェネレーターの色は深度にかかわらず `[0, 1]` に正規化します。

### 4.6 パラメーター

キーには宣言されたパラメーター名（大文字小文字を区別せず、現在の `aexlo-bench` と同じ照合）または `"#24"` のようなインデックスを指定します。複数のパラメーターに一致する名前はエラーとなり、候補のインデックスを一覧表示します。

値は TOML の型をそのまま使い、パラメーターの `ParamKind` に応じて解釈します。

| 種類 | TOML の値 |
| --- | --- |
| Float / Fixed / Angle | 数値 |
| Slider | 整数 |
| Checkbox | bool |
| Popup | 整数（AE と同じく 1 始まり）または選択肢のラベル |
| Point | `[x, y]`（ピクセル） |
| Point3D | `[x, y, z]` |
| Color | `[r, g, b]` または `[r, g, b, a]`（正規化） |
| Path | マスク ID（整数） |

`{ sweep = [v1, v2, ...] }` を指定すると、そのパラメーターがマトリクスの軸になります。パラメーター設定後、ハーネスは `aexlo render --set` と同様に `PF_Cmd_UPDATE_PARAMS_UI` を一度送信します。

### 4.7 時間

`fps` は数値または有理数を表す文字列（`"30000/1001"`）です。ハーネスは、`scale/step = fps` を既約分数の整数比（`24` → `24/1`、`"30000/1001"` → `30000/1001`）として、`set_time(frame * step, step, scale)` を呼び出します。

### 4.8 マトリクスの展開とバリアント ID

軸は `depth`、`render`、スイープ対象の各パラメーターです。バリアントの集合は、それらの直積になります。バリアント ID は次の形式です。

```text
表示用：     dot_glow[depth=16,render=gpu,Radius=100]
ファイル用： dot_glow.d16.gpu.radius-100
```

一覧の値が 1 つだけの軸は ID から省略します。プラグインが実行できないバリアントは、失敗ではなく**スキップ**します。対象は、`supports_gpu()` が false の場合の `gpu`、`PF_OutFlag2_SUPPORTS_SMART_RENDER` がない場合の `smart`、プラグインが宣言していない深度（16 では `PF_OutFlag_DEEP_COLOR_AWARE` がない場合、32 では `supported_pixel_formats()` に含まれない場合）です。

## 5. コアへの追加（`aexlo`）

### 5.1 パラメーターの検索

```rust
impl PluginInstance {
    /// 宣言された名前が大文字小文字を区別せず一致するインデックス。
    pub fn param_indices(&self, name: &str) -> Vec<usize>;
}
```

`aexlo-bench` の `resolve_param_index` と、インデックスのみを受け付ける CLI の `--set` は、この API を使うように移行します。

### 5.2 ビット深度別の I/O

```rust
impl PluginInstance {
    pub fn set_input_layer<D: PixelDepth>(&mut self, input: Layer<D>);
    pub fn output_depth(&self) -> PixelDepthKind;
    pub fn read_output<D: PixelDepth>(&self) -> Result<Layer<D>>;
}
```

- AE のプロジェクト深度と同様に、出力深度は入力深度に従います。
- `set_input_layer` は、既存の `Layer<Depth8>` を使うコードとのソース互換性を維持します。
- `read_output::<D>` は `D` が出力深度と異なる場合にエラーを返します。変換はハーネスの役割です。
- レイヤーパラメーターにも同じジェネリックなシグネチャを追加します。

### 5.3 strict モード

```rust
#[derive(Clone, Copy, Default)]
pub struct Strict {
    /// レンダリングのたびに出力 world を番兵値で埋める。
    pub poison_output: bool,
    /// ホストが割り当てた各 world の周囲にガード領域の行・バイトを設け、破棄時に検証する。
    pub guard_bands: bool,
    /// ハンドルと world の割り当て・破棄を数える。
    pub track_allocations: bool,
    /// smart render でのレイヤーとパラメーターのチェックアウト・チェックインを記録する。
    pub track_checkouts: bool,
}

impl Strict { pub fn all() -> Self; }

impl PluginInstance {
    pub fn set_strict(&mut self, strict: Strict);
    pub fn strict_report(&self) -> StrictReport;   // 前回の呼び出し以降の違反
}
```

`set_parallel_iterate` と同様に、インスタンスごとに設定します。すべて無効なら、ホストの割り当て経路とコストは変わりません。`Host::try_load_with(path, strict)` は `GLOBAL_SETUP` より前に strict モードを有効にするため、セットアップ中の割り当ても追跡します。`PluginInstance::finish(self) -> StrictReport` はプラグインの終了処理を行い、解放されなかったものを報告します。レンダリングコマンド中に割り当てた world は、そのコマンドの終了までに破棄する必要があります。それ以外は `GLOBAL_SETDOWN` までに破棄します。

| 機能 | 仕組み | 検出対象 |
| --- | --- | --- |
| `poison_output` | 8/16 bpc：バイトパターン `0xCD`。32 bpc：シグナリング NaN のペイロード。 | プラグインが一度も書き込まなかった出力ピクセル。 |
| `guard_bands` | 上下の追加行と rowbytes の末尾領域をパターンで埋めます。 | world の範囲外への書き込み（rowbytes の誤用）。検証するのは上下の行だけです。末尾領域は `rowbytes` 内にあり、プラグインがクリアしても構いません。`rowbytes == width * bpp` を前提とするコードで、見た目に誤ったレンダリングが生じるように設けています。 |
| `track_allocations` | Handle Suite / World Suite / `new_world` の経路にカウンターを設置します。 | `SEQUENCE_SETDOWN` / `GLOBAL_SETDOWN` 時のリーク。 |
| `track_checkouts` | `PreRender` での宣言と `SmartRender` のチェックアウトを記録します。 | PreRender で宣言されていないチェックアウト、チェックアウトされていないレイヤーのチェックイン、チェックインされていないレイヤーパラメーター（`PF_CHECKOUT_PARAM`）。SDK 上、smart-render のレイヤーピクセルのチェックインは任意なので、未実施でも報告しません。PreRender でチェックアウトしたレイヤーパラメーターは、続く SmartRender でチェックインしても構いません。 |

### 5.4 Observer

```rust
pub trait Observer: Send + Sync {
    fn command(&self, _event: &CommandEvent) {}      // コマンド、開始/終了、所要時間、PF_Err
    fn suite_call(&self, _event: &SuiteCallEvent) {} // スイート、バージョン、関数、所要時間
    fn allocation(&self, _event: &AllocationEvent) {}
    fn checkout(&self, _event: &CheckoutEvent) {}
}

pub enum ObserveLevel { Commands, Calls }

impl PluginInstance {
    pub fn set_observer(&mut self, observer: Option<Arc<dyn Observer>>, level: ObserveLevel);
}
```

- `Commands` はコマンドごとのイベントのみを追加します。ベンチマーク中も有効にできる程度の低コストです。
- `Calls` はスイート関数の呼び出しごとのイベントを追加します。コストが高いため明示的な有効化が必要で、ベンチマークでは使いません。
- イベントは `effect_ref` から到達できるインスタンスに帰属させます。到達できるインスタンスがない呼び出し（ANSI コールバック、iterate のワーカースレッドからの呼び出し）は、コマンドのディスパッチ中に設定したスレッドローカルを通じて帰属させ、iterate プールにも伝播します。
- 既存の `diagnostics` 機能は、ログを出力する `Observer` の実装の 1 つになります。

## 6. ハーネスの実行

### 6.1 モード

| モード | 方法 | 用途 | トレードオフ |
| --- | --- | --- | --- |
| `in-process` | `Host::from_entry_raw(entry)` | Rust プラグイン：`#[aexlo::test]`、`aexlo dev` | デバッガーと `dbg!` が使えます。クラッシュするとプロセスが終了します。 |
| `worker` | `aexlo worker` サブプロセスでプラグインを `dlopen` | 任意のプラグイン：`aexlo test/check/bench/preview` の既定 | クラッシュを分離できます。プロセス内のブレークポイントは使えません。 |

CLI の既定は `worker` です。`--isolate preset`（既定）はプリセットの全バリアントで 1 つのワーカーを再利用し、`--isolate variant` はバリアントごとにワーカーを起動します。`--isolate none` は CLI のプロセス内で実行します（デバッグ専用）。

### 6.2 ワーカーのプロトコル

`aexlo worker`（非表示のサブコマンド）は、標準入力から 1 行につき 1 つの JSON リクエストを読み、標準出力に 1 行につき 1 つの JSON レスポンスを書きます。ログは標準エラーに出力し、実行レポートに取り込みます。

```json
→ {"id":1,"op":"load","artifact":"/abs/MyGlow.plugin"}
← {"id":1,"ok":true,"info":{"params":[...],"smart":true,"gpu":false,"formats":[...]}}
→ {"id":2,"op":"run","variant":{...resolved variant...},"frame_out":"/tmp/aexlo-.../2.raw","observe":"commands","strict":["poison_output"]}
← {"id":2,"ok":true,"frame":{"path":"...","w":640,"h":480,"depth":16},"timing":{...},"trace":{...},"strict":{...}}
```

フレームは、セッションごとの一時ディレクトリにある raw ファイルとして受け渡し、パイプには流しません。`variant` ペイロードは完全に解決済み（`inherits` や名前による参照を含まない）なので、ワーカーにマニフェストの処理は不要です。`tests/e2e` の `render_one` はこれに置き換えます。

### 6.3 結果の分類

各実行は、次のいずれか 1 つの結果で終了します。

| 結果 | 原因 |
| --- | --- |
| `pass` | レンダリングされ、すべてのチェックと golden の比較に合格。 |
| `fail` | レンダリングされたものの、チェックまたは golden の比較に不合格。 |
| `error` | プラグインが `PF_Err` を返した、または読み込みに失敗。 |
| `crash` | ワーカーが終了。レポートにシグナルまたは例外コードと、トレースの最後のコマンドを記載。 |
| `timeout` | `timeout` を超過。ハーネスはまず `AppHost` の中断経路でキャンセルを要求し、2 秒後にワーカーを強制終了。 |
| `skipped` | プラグインがそのバリアントをサポートしていない（§4.8）。 |

`crash` または `timeout` の後は、次のバリアントに新しいワーカーを用意します。

### 6.4 実行結果のデータ

```rust
pub struct Run {
    pub variant: VariantId,
    pub outcome: Outcome,
    pub frame: Option<Frame>,
    pub timing: Timing,         // コマンドごとの実経過時間（Observer の Commands レベル）
    pub trace: Trace,           // コマンド列、PF_Err の値、収集したログ
    pub strict: StrictReport,
    pub checks: Vec<CheckResult>,
    pub golden: Option<Comparison>,
}

pub struct Frame { /* 幅、高さ、深度、ピクセル */ }
impl Frame {
    pub fn pixel(&self, x: u32, y: u32) -> [f32; 4];   // 正規化済み。すべての深度に対応
    pub fn compare(&self, other: &Frame, tol: &Tolerance) -> Comparison;
    pub fn save(&self, path: &Path) -> Result<()>;      // PNG 8/16、EXR 32
}
```

## 7. チェック

チェックは実行結果（場合によっては 2 回目の実行結果も）を受け取り、合格、メッセージ付きの不合格、または適用対象外を返します。

| ID | 既定 | 追加コスト | 不合格となる条件 |
| --- | --- | --- | --- |
| `finite` | 有効 | なし | 出力に NaN/Inf が含まれる（32 bpc のみ）。 |
| `coverage` | 有効 | なし | 出力矩形内に番兵値のピクセルが残る。`strict.poison_output` も有効になります。 |
| `deterministic` | 有効 | 追加レンダリング 1 回 | 同じバリアントの 2 回のレンダリング結果が異なる。 |
| `iterate-parallel` | 無効 | 追加レンダリング 1 回 | 並列と直列の iterate 結果が許容誤差を超えて異なる（スレッド安全性）。 |
| `depth-consistency` | 無効 | 深度ごと | 同じバリアントの 8/16/32 ビット出力が `max_abs = 2/255` を超えて異なる。2 つ以上の深度が必要。 |
| `bounds` | strict 時 | なし | ガード領域が上書きされた。 |
| `allocations` | strict 時 | なし | ハンドルまたは world がスコープ終了後も残る。 |
| `checkouts` | strict 時 | なし | 未宣言、または不均衡な smart-render のチェックアウト。 |
| `flags` | strict 時 | なし | 宣言した out_flags と動作が矛盾する。たとえば 16 bpc 対応を宣言しているのに 16 bpc でエラーになる。 |

`strict = true` はすべての strict 機能と対応するチェックを有効にします。一覧を指定すると一部だけを有効にします。

## 8. golden（参照画像）

### 8.1 保存

既定のパスは、マニフェストの隣にある `golden/<variant file-safe id>.<ext>` です。ID には `render` と `iterate` を**含めません**（すべてのレンダリング経路が同じ参照画像と一致する必要があり、それ自体がチェックとなります）。上書きする場合は次のように指定します。

```toml
golden = { path = "golden/custom.png", split_by = ["render", "platform"] }
```

形式は 8 bpc → PNG8、16 bpc → PNG16、32 bpc → EXR（`image` クレートの `exr` 機能）です。

### 8.2 許容誤差

すべての値は `[0, 1]` に正規化するため、同じ許容誤差をすべての深度に適用できます。

| キー | 既定値 | 意味 |
| --- | --- | --- |
| `max_abs` | `1/255` | チャンネルごとに許容する最大の差。 |
| `max_bad` | `0` | `max_abs` を超えてもよいピクセルの割合。 |
| `min_psnr` | なし | 任意指定の PSNR の下限（dB）。 |

### 8.3 bless と失敗時の生成物

- `aexlo test --bless [filter]` は、合格した（クラッシュしていない）バリアントの golden を作成または上書きし、変更したファイルを表示します。
- golden が存在しない場合は、`--bless` を指定しなければ `fail` です。CI では暗黙に golden を作成しません。
- 不一致の場合、ハーネスは `target/aexlo/<variant>/{actual,expected,diff}.png` を保存します。`diff` は許容誤差に合わせてスケールしたヒートマップです。

### 8.4 AE 由来の golden

`golden = { source = "ae", path = "golden/ae/dot_glow.png" }` は、実際の After Effects でレンダリングした参照画像を示します。これらは `--bless` で上書きせず、不合格の場合は性能や動作の回帰とは別に、**一致検証（parity）**の失敗として報告します。これは `playground` の考え方（AE を正解とする）を、ユーザーのプラグインにも拡張するものです。

## 9. パラメーターのファジング

`aexlo test --fuzz <n> [--seed s] [preset]` は、各パラメーターの宣言範囲（`param_slider_range`、`param_choices`、チェックボックス、フレーム内のポイント）から値を選び、`n` 個のバリアントをレンダリングします。適用するのは `finite`、`coverage`、`bounds`、クラッシュ、タイムアウトのみで、golden は使いません。失敗したケースは、シードとともにそのまま貼り付けられる `[[preset]]` ブロックとして出力します。

## 10. ベンチマーク

### 10.1 手順

バリアントごとに一度読み込み、時間を計測しない `warmup` 回のレンダリングを行ってから、`samples` 回のレンダリングを計測します。中央値、最小値、平均値、p95、メガピクセル/秒を報告します。ベンチマークは常に直列に、1 つのワーカーで、`ObserveLevel::Commands` を使い、strict を無効にして実行します。

### 10.2 フェーズ別の内訳

Observer のコマンドイベントから、`PRE_RENDER`、`SMART_RENDER` / `RENDER`、GPU ディスパッチ、**ホストのオーバーヘッド**（実経過時間からプラグインのエントリーポイント内の時間を引いたもの。入力変換、world のセットアップ、読み戻し）を算出します。この内訳により、「プラグインが遅くなった」のか「aexlo が遅くなった」のかを区別できます。

### 10.3 ベースライン

```sh
aexlo bench --save-baseline main
aexlo bench --baseline main --threshold 5%
```

マニフェストの隣にある `.aexlo/baselines/<name>.json` に保存します（コミットするかどうかはプロジェクトの方針に従います）。各記録にはマシンの識別情報（OS、CPU モデル、GPU、aexlo のバージョン）が含まれ、異なる識別情報間で比較すると警告します。

中央値と最小値の**両方**がベースラインよりも閾値を超えて遅い場合、そのバリアントを**性能低下（regression）**と判定します。最小値も条件に含めることで、一時的なノイズを除外します。性能低下があれば非ゼロで終了します。

### 10.4 互換性

`AEXLO_BENCH_*` の変数は `cargo bench -p aexlo-bench` で引き続き使えます。これらは暗黙のプリセットを生成するため、現在の `aexlo bench` と `aexlo-bench` と同様に、両方のフロントエンドが同じ計測ループ（`aexlo_harness::bench`）を共有します。

## 11. プレビュー

`aexlo dev`（ソースの監視）と `aexlo preview`（ビルド済みプラグインの監視）は、`crates/cli/src/viewer.rs` のブラウザービューアーを共有します。次の機能を追加します。

- **プリセット選択**：選択したバリアントを `test` と同じ条件でレンダリングします。
- パラメーターのコントロール（既存）：プリセットの値で初期化します。
- golden との**比較**モード：ワイプ、横並び、差分ヒートマップ。深度間、レンダリング経路間の比較も可能です。
- `time.frame` を操作するタイムスクラバー。
- 直前のレンダリングのフェーズ別時間と strict レポート。
- **「Save as preset」**：`toml_edit` を使って書式を維持しながら、現在の状態を `aexlo.toml` に追記します。目視で見つけた問題のあるフレームを、ワンクリックで回帰テストにできます。

`studio` には、後のマイルストーンで「Export layer effect as preset」を追加します。

## 12. CLI

```text
aexlo presets   [filter]                 展開したバリアントを一覧表示
aexlo test      [filter] [--bless] [--fuzz n] [--seed s]
aexlo bench     [filter] [--save-baseline n | --baseline n --threshold p]
aexlo check     [filter]                 CI 用：test + strict + baseline（設定されていれば）
aexlo dev       [--preset name] ...      既存のフラグを維持
aexlo preview   <plugin> [--preset name] ...
aexlo render    <plugin> [--preset name] ...   既存のフラグがプリセットを上書き
aexlo worker                              非表示。§6.2
```

`aexlo test --save-frames <dir>` は、golden の有無にかかわらず、すべてのバリアントのフレームを `<dir>/<file-safe id>.<png|exr>` にも保存します。

共通フラグは `--manifest <path>`、`--depth 8,16`、`--render smart`（マトリクスを絞る）、`--isolate preset|variant|none`、`--jobs <n>`（test/check の並列ワーカー数。bench は常に 1）、`--strict`、`--format human|json`、`--junit <path>` です。

`filter` はバリアントの表示用 ID に部分文字列で一致させます。`*` を含む場合は glob として扱います。

終了コードは次のとおりです。

| コード | 意味 | 該当するケース |
| --- | --- | --- |
| `0` | すべてのバリアントが合格、またはスキップされた。 | |
| `1` | プラグインを実行し、不合格と判定した。 | `fail`、`error`、`crash`、`timeout`（§6.3）、ベンチマークの性能低下（§10.3）。 |
| `2` | マニフェストまたはコマンドラインが不正。 | TOML の構文、未知のキー・プリセット・親、`inherits` の循環、曖昧なパラメーター名、不正なフラグ値。 |
| `3` | **ハーネスのエラー**：実行が判定に到達しなかった。 | ワーカーを起動できない、または見つからない。一時ディレクトリや出力ディレクトリに書き込めない。ワーカーの応答が不正（aexlo のバグ）。`crate` モードで `cargo build` に失敗。ベースラインまたは golden ファイルが存在するものの、読み込みやデコードができない。 |

この分類により CI が対処を判断できます。`1` はプラグインの修正が必要で、`3` は環境または aexlo 自体の問題なので、再試行で成功する可能性があります。ビルド失敗が `1` ではなく `3` なのは、バリアントが判定されていないためです。判定済みの失敗とハーネスのエラーが両方ある場合、終了コードは `3` です。

## 13. Rust API

```rust
// Cargo.toml: [dev-dependencies] aexlo-test = "0.1"

#[aexlo::test(depth = [8, 16, 32], render = [smart, gpu], preset = "dot_glow")]
fn glow_is_symmetric(fx: &mut aexlo::PluginInstance) -> aexlo_test::Result {
    fx.set_param_named("Radius", 100.0)?;
    let frame = aexlo_test::render(fx)?;
    frame.assert_symmetric_about((320, 240), 0..20, 4.0 / 255.0)?;
    frame.assert_golden("dot_glow")?;
    Ok(())
}
```

- プロセス内で実行します。バリアントごとに 1 つの `#[test]` に展開し、`glow_is_symmetric__d16_gpu` のように命名します。未対応のバリアントは「skipped」と表示して合格にします。
- `preset = "..."` は、テスト時に最も近い `aexlo.toml` から読み取った、そのプリセットの解決済みの状態を初期状態とします。
- `#[aexlo::preview]` と異なり、テストに `#[ignore]` は付けません。
- `Frame` のアサーションは `assert_golden`、`assert_close`、`assert_pixel`、`assert_region`、`assert_symmetric_about`、`assert_opaque`、`assert_finite` で、すべて正規化された許容誤差を使います。失敗時は §8.3 と同じ生成物を保存します。
- `aexlo_test::check(fx, &["coverage", "deterministic"])` は、プロセス内のインスタンスに対して §7 のチェックを実行します。

## 14. 移行

| 既存の機能 | 移行先 |
| --- | --- |
| `aexlo::{save_preview, preview_mode, ...}` | `aexlo_harness::preview`。`#[aexlo::preview]` はこのパスに展開します。ユーザーは `aexlo-test` を開発用依存関係に追加します。 |
| `tests/e2e/src/bin/render_one.rs` | `aexlo worker` |
| `aexlo-bench` のパラメーター・解像度・入力のヘルパー | `aexlo_harness::preset` + `bench` |
| CLI の `--set <index>=<value>` | `param_indices` を通じて名前も受け付けます。 |
| `tests/e2e` の手書きのフィクスチャ | フィクスチャごとにマニフェストを採用できます。必須ではありません。 |

## 15. マイルストーン

各マイルストーンは、macOS arm64 と、フィクスチャが存在する場合は Windows x64 で受け入れ基準を満たした時点で完了とします。

| # | 範囲 | 受け入れ基準 |
| --- | --- | --- |
| M1 | コア：`param_indices`、深度を汎用化した I/O（16、次に 32）、`Commands` レベルの `Observer`。 | ディープカラーを宣言する同梱のフィクスチャを 16 bpc でレンダリングし、`Layer<Depth16>` として読み戻せる。コマンドの時間を観測できる。 |
| M2 | ハーネス：マニフェスト、`inherits`、マトリクス、ワーカープロトコル、結果の分類。`aexlo presets`、`aexlo render --preset`。 | 一方がクラッシュする 2 つのフィクスチャを扱うマニフェストで、`aexlo test` が 1 件の `crash` を報告し、残りのバリアントも実行する。`render_one` を削除。 |
| M3 | テスト：golden、`--bless`、`finite` / `coverage` / `deterministic`、JSON と JUnit 出力、`#[aexlo::test]` を備えた `aexlo-test`。 | `demos/noise` にマニフェストと `#[aexlo::test]` があり、ハッシュ定数を変えると両方が不合格になり、差分の生成物が保存される。 |
| M4 | strict：ガード領域、割り当てとチェックアウトの追跡、`bounds` / `allocations` / `checkouts` / `flags`、`iterate-parallel`、`depth-consistency`、ファジング。 | world の 1 行先に書き込むテストプラグインが `bounds` に不合格となり、ハンドルをリークするものが `allocations` に不合格となる。 |
| M5 | プリセットのベンチマーク：フェーズ別の内訳、ベースライン、性能低下の終了コード。`aexlo-bench` の移行。 | フィクスチャのレンダリングを意図的に閾値以上遅くすると、`aexlo bench --baseline` が 1 で終了する。 |
| M6 | ビューアー：プリセット選択、比較モード、タイムスクラバー、「Save as preset」。 | 保存したプリセットを再利用できる。ビューアーに表示され、`aexlo test` でも実行できる。 |
| M7 | `aexlo check`、CI テンプレート（GitHub Actions、macOS + Windows）、AE 由来の golden。 | このリポジトリのフィクスチャでテンプレートが成功する。 |

## 16. 未解決の問い

1. **iterate スレッドでの Observer の帰属。** スレッドローカルを rayon プールへ伝播すると、`Commands` レベルのトレースで許容できるコストを超える可能性があります。その場合、スイート呼び出しの帰属はディスパッチ元のスレッドに限定できます。
2. **GPU の許容誤差。** GPU と CPU の経路や、異なる GPU 同士で、ビット単位の一致はほとんどありません。`render = gpu` に緩い既定の `max_abs` を与えるべきか、それとも常に `split_by = ["render"]` を使うべきでしょうか。
3. **マニフェスト内のマスク。** `set_mask_paths` はありますが、マスクをテキストで表す形式（頂点、接線、モード、ぼかし）は、それ自体が独立したフォーマットです。studio の書き出し（M6 以降）により、実際にどのような記述が使われるかわかるまで延期します。
4. **32 bpc の番兵値。** シグナリング NaN は明確な目印ですが、`finite` にも引っかかります。`coverage` を先に実行して該当ピクセルを処理済みとし、二重に報告しないようにする必要があります。

## 17. 実装上の補足

上記の草案で未確定だった点について、M1–M7 の実装中に次のように決定しました。

- **レイヤーパラメーター**は任意の深度を受け付けます（`set_layer_param::<D>`）。`clear_layer_param` は深度を指定せずに関連付けを解除し、`layer_param` は `AnyLayer` を返します。
- **16 bpc の白は 32768**（`PF_MAX_CHAN16`）であり、`u16::MAX` ではありません。PNG16 ファイルはこの値に合わせて、正確に相互変換します。
- **`Calls` レベルの Observer** は、ディスパッチ元のスレッドのスイート呼び出しのみを報告します（未解決の問い 1）。iterate のワーカースレッドからの呼び出しは帰属させません。
- **strict モード**は、SDK に明記された箇所ではその規定に従います。smart-render のレイヤーピクセルのチェックインは任意（厳密には必要ではない）なので、チェックインを必須とするのはレイヤーパラメーター（`PF_CHECKOUT_PARAM`）のみです。PreRender でチェックアウトしたものは、続く SmartRender でチェックインしても構いません。ガード領域は world の上下を検証します。行の末尾領域は `rowbytes` 内にあり、書き込んでも構いません。レンダリングコマンド中に割り当てた world はそのコマンドの終了までに、ハンドルは `GLOBAL_SETDOWN` までに破棄する必要があります。
- **`-id` による除外は `--strict` でも維持**します。プリセットの明示的な除外が、一括指定のフラグより優先されます（フィクスチャでは SDK_Noise を `allocations` の対象外にしています。配布された状態では、毎フレーム pre-render データをリークするためです）。
- **`aexlo check`** は `--baseline <name>` でベースラインを指定します。指定がなければ、`.aexlo/baselines/main.json` が存在する場合に `main` を使います。
- **`aexlo bench`** は、マニフェストがなくてもプラグインのパスを受け付けます（フラグで設定するフロントエンド）。最初の引数がプラグインを指していれば、そのプラグインを選択します。
- **ビューアー**は `--preset` / `--manifest` でプリセットを読み込みます。`--no-open` はブラウザーを開かないようにします。「Save as preset」は、`inherits`、スイープ・編集したパラメーター、時間を書き込みます。
- **テストプラグイン** `tests/misbehave` は、要求に応じてクラッシュ、ハング、エラー、範囲外の書き込み、リーク、ピクセルの未書き込み、ランダム化を行います。各マイルストーンの受け入れテストは、実際の `aexlo` バイナリを通じてこのプラグインを実行します（`crates/cli/tests/`）。
