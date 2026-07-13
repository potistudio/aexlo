# aexlo コードベース分析レポート — API設計 & パフォーマンス

作成日: 2026-07-14 / 対象ブランチ: `feat/live-preview` (575793f)

## 全体像

aexlo は After Effects プラグイン (.aex / .plugin) を AE なしでロード・実行するホストエミュレーター。
ワークスペース構成は `aexlo` (コア) / `wrapper` (Pixel・Layer の安全ラッパー) / `aexlo-macros`
(`#[aexlo::preview]`) / `cli` / `benches` / `demos` / `tests/e2e`。

全体としての設計はかなり筋がええ。特に:

- スイートを **stateless vtable + プロセス共有 static** にまとめた設計 ([suites/mod.rs](aexlo/src/suites/mod.rs)) は所有権モデルが明快
- `PF_NewWorld` のリーク / 回収の対 ([world.rs:136-141](aexlo/src/suites/world.rs#L136-L141), [world.rs:50-59](aexlo/src/suites/world.rs#L44-L59)) はコメント含めて誠実
- `iterate8` の rayon 行並列化 ([iterate.rs:136](aexlo/src/suites/iterate.rs#L136)) はエイリアシングにも配慮されとる
- GPU パスの CPU フォールバック連鎖 (`render_frame`) も実戦的

そのうえで、API 設計とパフォーマンスの両面で直すべき点がいくつかある。重要度順に並べたで。

---

## 1. API設計の課題

### 1.1 【重大】パラメーターインデックス体系が 3 種類混在しとる

同じ `PluginInstance` の公開メソッド間で「index が何を指すか」が食い違っとる:

| メソッド | index の意味 |
|---|---|
| [`set_param`](aexlo/src/instance.rs#L821) | `internal = index + 1`。ただし doc は「index 0 は入力レイヤー」と言うとる (実際の internal 0 がレイヤーなので矛盾) |
| [`get_param`](aexlo/src/instance.rs#L907) | `internal = index + 1` (ユーザー index 0 = 最初の実パラメーター) |
| [`param_values`](aexlo/src/instance.rs#L899) | `(1..len)` を回して `get_param(i)` を呼ぶ → **internal 2 以降しか読まん** |
| [`param_by_index`](aexlo/src/instance.rs#L987) | 生の internal index (0 = レイヤー) |

これの帰結として実バグが 2 つ出とる:

1. **`param_values` が最初の実パラメーター (ユーザー index 0 = internal 1) を絶対に返さん。**
   `(1..len)` 開始 + `get_param` 内の `+1` で二重シフトや。
2. **CLI の `aexlo params` で名前と値が 1 個ズレる。**
   [main.rs:123](cli/src/main.rs#L123) は値を `param_values()` (internal `i+1` の値) から、
   名前を `param_by_index(i)` (internal `i` の名前) から取っとるので、
   「前のパラメーターの名前 + 次のパラメーターの値」が並ぶ。そのまま `--set i=v` すると
   表示された名前とは別のパラメーターが書き換わる。

**改善案:** 公開 API のインデックス空間を 1 つに決めるんや。推奨は
「**公開 index = 実パラメーターの 0 始まり。レイヤーは API から見えない**」で統一:

- `param_values` は `(0..self.params.len() - 1)` で `get_param` を回す
- `param_by_index` を廃止するか、同じ +1 変換を入れた `param_def(index)` に置き換える
- `set_param` の doc コメント「index 0 is the input layer」を削除
- 変換は 1 箇所のプライベートヘルパー (`fn internal_index(user: usize) -> usize`) に集約

### 1.2 【重大疑い】`FIX_SLIDER` の値変換が Q31 になっとる

[`set_param`](aexlo/src/instance.rs#L846) の `ParamValue::Fixed` は
[`f32_to_q31`](aexlo/src/utils.rs#L4) で書き込んどるが、AE SDK の
`PF_FixedSliderDef.value` は `PF_Fixed` = **16.16 固定小数点**のはずや。
Q31 は [-1.0, 1.0) にクランプするから:

- 1.0 以上のスライダー値が設定できん
- 0.5 を書くと `0x4000_0000` になり、プラグイン側は 16.16 として読むので **16384.0** に見える

`get_param` も対で `q31_to_f32` を使っとるから aexlo 内ではラウンドトリップして気づけん。
Angle / Point は正しく `f32_to_fixed16` を使っとるだけに、Fixed だけ浮いとる。

**改善案:** `ParamValue::Fixed` の読み書きを `f32_to_fixed16` / `fixed16_to_f32` に変更し、
実プラグイン (SDK サンプルの FIX_SLIDER 持ち) で e2e 検証を足す。

### 1.3 フレームサイズが 1920×1080 にハードコード

- [instance.rs:62-63](aexlo/src/instance.rs#L62-L63) の `WIDTH`/`HEIGHT` と
  [core/constants.rs](aexlo/src/core/constants.rs) の `DEFAULT_WIDTH`/`DEFAULT_HEIGHT` が**二重定義**
- `set_input` で任意サイズの入力を差し替えられるのに、出力レイヤー・`in_data.width/height`・
  smart render のチェックアウト矩形 ([smart_render.rs:36-55](aexlo/src/host/smart_render.rs#L36-L55) は
  定数を直接参照) は 1080p のまま → 入力サイズ ≠ 1080p だと CPU smart render パスの矩形報告が嘘になる
- `render_gpu` だけは `in_data.width` を出力に合わせて直しとる ([instance.rs:580-581](aexlo/src/instance.rs#L580-L581)) が、CPU パスは直しとらん

**改善案:**
- `PluginInstance::try_load` にビルダーを足す (`PluginInstance::builder().size(w, h).load(path)?`) か、
  `set_output_size(w, h)` を公開して worlds / in_data / checkout 矩形を一括更新
- `checkout_layer_stub` は `effect_ref` を受け取っとるんやから、定数やなくてインスタンスの実レイヤー寸法を返すべき
- 定数の二重定義は `core::constants` に一本化

### 1.4 ライフサイクルの片道切符 (Drop で teardown せん)

`PluginInstance` に `Drop` 実装がなく、`SEQUENCE_SETDOWN` / `GLOBAL_SETDOWN` が**どこからも呼ばれとらん**。
`gpu_device_setdown` も public メソッドとして存在するだけで自動では呼ばれん。

- プラグインが確保した global/sequence handle がリークする
- teardown で後始末する行儀のよいプラグイン (ライセンスチェック解放、スレッドプール停止など) が壊れる
- `raw_library` (dlopen) がインスタンスより先に drop されると entry_point がダング linkする危険は
  フィールド順で回避しとるようやが、明示されとらん

**改善案:** `impl Drop for PluginInstance` で
`SEQUENCE_SETDOWN → GLOBAL_SETDOWN → gpu_device_setdown` をベストエフォート送信
(エラーは `log::warn` に落とす)。フィールドの drop 順依存 (`entry_point` が `raw_library` より先) もコメントで明示。

### 1.5 公開 API の表面積が広すぎ & 抽象漏れ

- [`in_data`](aexlo/src/instance.rs#L302) と [`pica`](aexlo/src/instance.rs#L299) が **pub フィールド**。
  `after_effects_sys` の生構造体がそのまま公開 API に漏れとるので、sys クレートの
  バージョン更新 = 破壊的変更になる。ユーザーが `in_data.effect_ref` を触ったら即壊れる
- [`add_instance_param`](aexlo/src/instance.rs#L970) / [`clear_instance_params`](aexlo/src/instance.rs#L1008) は
  AGENTS.md で「ホストから呼ぶな」と明言されとるのに `pub`。suites ブリッジ専用なら `pub(crate)` にすべき
- `params()` が `&[PF_ParamDef]` (生 sys 型) を返す
- `lib.rs` の `pub use wrapper::*` は wildcard 再エクスポートで、wrapper に何か足すたび
  aexlo の公開 API が暗黙に増える

**改善案:** sys 型を返す/受けるものは `pub(crate)` に降格。診断用に必要なら
`ParamInfo { name, type_name, value }` みたいな安全な読み取り専用ビューを公開する。
wildcard は明示列挙に変える。

### 1.6 プレビュー / ビューアのプロセス管理がコアに同居

[instance.rs:127-280](aexlo/src/instance.rs#L127-L280) の
`preview_requested` / `preview_mode` / `viewer_lock` / `ensure_live_viewer` / `open_in_viewer` は
プラグインホストの機能やなくて **dev ツーリング**や。コアクレートが:

- 環境変数 (`AEXLO_PREVIEW`, `AEXLO_BIN`, `AEXLO_DISABLE_GPU`) を読む隠れ挙動を持ち
- 子プロセスを spawn し (`kill -0`, ビューア起動)
- ロックファイルを管理する

さらに [`pid_alive`](aexlo/src/instance.rs#L160-L177) は **Windows で無条件 `true`** を返すので、
ロックファイルが残った Windows 環境では `viewer_is_running` が永遠に true → live ビューアが二度と起動せん。
ロックファイル方式自体も、pid 再利用や `std::fs::write` の非アトミック性で race がある。

**改善案:** これらを `aexlo-preview` (または cli 内) に切り出す。`#[aexlo::preview]` マクロの
生成コードもそっちを参照するようにする。Windows は `OpenProcess` 相当を使うか、
ロックを「開きっぱなしのファイルハンドル + advisory lock」(`fs2` など) に変える。

### 1.7 プロセスグローバル state が複数インスタンスで衝突する

- [`PLUGIN_PATH`](aexlo/src/host/utility.rs#L10): `load()` のたびに上書きされる単一グローバル。
  プラグインを 2 個ロードすると、両方の `get_platform_data` が**最後にロードした方のパス**を返す
- [`GPU_WORLD_PTRS`](aexlo/src/gpu.rs#L34): world ポインタ (アドレス) をキーにしたグローバル集合。
  アドレス再利用で誤判定する余地があり、コメントにも書かれとる通り `effect_ref` が来ん
  callback の制約による苦肉の策やが、少なくともインスタンス drop 時の掃除は
  `unregister_all_worlds` 頼み

**改善案:** `PLUGIN_PATH` はインスタンスのフィールドにして、`get_platform_data` は
`effect_ref` 経由でインスタンスから引く (この callback は `effect_ref` を受け取れるはず)。
GPU world 集合は「登録時に世代カウンターを付ける」か、docs に multi-instance 制約を明記する。

### 1.8 `call_plugin` 中のエイリアシング (健全性リスク)

[`call_plugin`](aexlo/src/instance.rs#L1331) は `&mut self` を保持したまま
`self as *mut _` を `effect_ref` に入れてプラグインへ渡し、callback 側
([checkout_output](aexlo/src/host/smart_render.rs#L138) など) が
`get_instance_ptr(effect_ref).as_mut()` で **同じインスタンスの `&mut` を再生成**する。
Stacked Borrows 的には `&mut self` 生存中の再借用で UB 相当や。実害は出にくいが、
Miri やこの先のコンパイラー最適化に対して脆い。

**改善案:** 少なくとも「プラグイン呼び出し中に触られうるフィールド」
(`world`, `input_world`, `params`, `smart_render_data`, `gpu_context` あたり) を
`UnsafeCell` ベースの内部構造にまとめ、callback からは raw pointer 経由でのみアクセスする設計に寄せる。
短期的には `checkout_layer_pixels_stub` の [`expect`](aexlo/src/host/smart_render.rs#L97-L100)
(プラグイン呼び出し中の panic → FFI 境界越え unwind で UB) を
`PF_Err_BAD_CALLBACK_PARAM` 返却に変えるのが先決や。

### 1.9 エラー設計: 文脈が失われる

- [`AexloError::Unexpected(String)`](aexlo/src/core/error.rs#L48) が万能受け皿になっとって、
  GPU 失敗・PNG 失敗・spawn 失敗が全部同じ variant
- `PluginExecutionFailed { code }` に**どのコマンドで失敗したか**が入っとらん。
  `render_frame` は GPU → smart → legacy と 3 段フォールバックするから、
  最終エラーだけ見ても何が起きたか分からん
- 一方で `write_output_rgba` は `Layer` の `Result<(), String>` を文字列連結で包んどる
  ([instance.rs:728-732](aexlo/src/instance.rs#L728-L732))。`LayerError` は
  `std::error::Error` 実装済みなんやから `#[from]` で受けるべき

**改善案:** `PluginExecutionFailed { command: &'static str, code: i64 }` に拡張、
`Gpu(String)` / `Io { context, source }` あたりの variant を分離、
`LayerError` を `AexloError` に `#[from]` 追加。

### 1.10 iterate suite のスタブが「成功」を返す

[`iterate_origin_stub` / `iterate_lut_stub` / `iterate_generic_stub`](aexlo/src/suites/iterate.rs#L184-L255)
は warn ログだけ出して `PF_Err_NONE` を返す。これを使うプラグインは
**エラーにならずに黙って何もレンダリングされへん**。デバッグ地獄のもとや。

**改善案:** 未実装スタブは `PF_Err_UNRECOGNIZED_PARAM_TYPE` 系のエラーを返すか、
最低限 `iterate_origin` / `iterate_generic` は実装する (`iterate_generic` は
rayon で `0..iterationsL` を分割するだけなので実装コストが低い)。

### 1.11 `Layer::as_sys` の provenance 問題 (wrapper)

[layer.rs:194](wrapper/src/layer.rs#L194) は `&mut self` を取りながら
`self.pixels.as_ptr() as *mut PF_Pixel` と **const ポインタを mut にキャスト**しとる。
プラグインはこの `data` に書き込むんやから、`as_mut_ptr()` 由来にせんと
provenance 的に書き込みが未定義になる。1 文字直すだけの話やが健全性に効く。

あと `Layer::from_raw` が `Layer<D>` の関連関数なのに常に `Layer<Depth8>` を返す
シグネチャなのも型パラメーターの意味が迷子や (`impl Layer<Depth8>` ブロックへ移すべき)。

---

## 2. パフォーマンスの課題

### 2.1 【最重要】診断文字列が feature 無効でも毎回構築される

`DiagnosticBuilder` の `emit()` は `diagnostics` feature 無効時 no-op やが、
**`add_arg(format!(...))` の引数評価は無効化されとらん**。しかも
[smart_render.rs](aexlo/src/host/smart_render.rs#L58-L67) と
[world.rs](aexlo/src/suites/world.rs#L61-L65) は `#[cfg]` なしで無条件に呼んどる。

つまり release ビルドでも、**フレームごとに呼ばれる** `checkout_layer` /
`checkout_layer_pixels` / `checkout_output` / `PF_NewWorld` / `PF_GetPixelFormat` の中で
`format!("{:#x}", ...)` × 数個 + `Vec<(Cow, String)>` のヒープ確保が毎回走る。
`PF_GetPixelFormat` はプラグインによっては 1 フレームに何十回も呼ばれるやつや。

**改善案:** 呼び出し側を全部 `#[cfg(feature = "diagnostics")]` で括る…のは漏れやすいので、
`diag!` マクロを 1 個作って феature 無効時は完全に消えるようにするのが本命:

```rust
macro_rules! diag {
    ($name:expr, { $($k:expr => $v:expr),* $(,)? } $(, result: $r:expr)?) => {
        #[cfg(feature = "diagnostics")]
        { /* DiagnosticBuilder 構築 */ }
    };
}
```

ついでに `print_colored` の `println!` 連打 (呼び出しごとに stdout ロック 5 回以上) も、
1 個の `String` に組んでから一発 `println!` にすると diagnostics 有効時も速くなる。

### 2.2 ホットパスの logging

- [`call_plugin`](aexlo/src/instance.rs#L1335-L1373) はコマンド 1 回につき `log::info!` を **3 発**
  (実行前・実行後・成功)。`format!("{:?}", command).blue()` は logger 無効なら遅延評価されるとはいえ、
  info レベルでフレームごとに 3 行はレンダーループのログを埋め尽くす
- [`host_new_handle_impl`](aexlo/src/suites/handle.rs#L33) は**関数入口で無条件 `log::info!`**。
  handle 確保はプラグインのフレーム内で頻発する操作や

**改善案:** レンダー系コマンド (`Render`, `SmartRender`, `SmartPreRender`, `SmartRenderGpu`) と
handle suite のログは `debug!`/`trace!` に落とす。成功ログは 1 発に統合。

### 2.3 GPU パス: フレームごとの大型アロケーション + 転送過多

1080p の場合、`render_gpu` 1 回あたり:

| 箇所 | サイズ | 頻度 |
|---|---|---|
| [`pack_layer_to_bgra_f32`](aexlo/src/instance.rs#L640) の staging `Vec<f32>` | ~33 MB | 毎フレーム |
| [readback staging](aexlo/src/instance.rs#L629) `vec![0f32; ...]` | ~33 MB | 毎フレーム |
| 入力アップロード ([instance.rs:608-613](aexlo/src/instance.rs#L608-L613)) | ~33 MB 転送 | **入力が変わらんでも毎フレーム** |

watch / live-preview のようにパラメーターだけ変えて回すユースケースでは、
入力レイヤーは不変なのに毎回 pack + upload しとる。

さらに `pack` はチャンネル単位の `push` ×4/pixel、`unpack` はスカラーの clamp+round ループで、
830 万チャンネル分を 1 スレッドで舐める。

**改善案 (効果順):**
1. staging バッファ 2 本を `PluginInstance` のフィールドとして保持し再利用 (アロケーション消滅)
2. 入力レイヤーに dirty フラグ (`set_input` で立てる) を持たせ、変更時のみ pack + upload
3. pack/unpack を `rayon` の `par_chunks_exact` で並列化、または少なくとも
   `chunks_exact_mut(4)` への一括書き込みに変えてオートベクトル化を効かせる

### 2.4 CUDA バックエンドの同期過多

[gpu.rs](aexlo/src/gpu.rs#L328-L386) は `alloc` / `write_buffer` / `read_buffer` の
**各操作直後に `stream.synchronize()`**、さらに `wait_for_completion` では
stream + context の二段同期。安全側に倒した設計なのは分かるが、
1 フレームに full-device sync が 4〜5 回入ると GPU パイプラインがぶつ切りになる。

**改善案:** upload → render → readback を全部同一 stream に流しとるんやから、
中間の sync は原理上不要 (stream 内は順序保証される)。プラグインが別 stream に
launch する対策が必要なのは render 前後だけなので、`write_buffer` 後の sync は
`ensure_buffer` 直後 (初回のみ) に限定し、定常状態は「readback 後の 1 回」まで減らせる。

### 2.5 CPU ピクセル変換のループ

- [`write_rgba_bytes`](wrapper/src/layer.rs#L214-L220) は `buffer[offset + n]` の
  インデックスアクセス。長さ検証済みでも境界チェックが残る形や。
  `buffer.chunks_exact_mut(4).zip(self.pixels.iter())` に書き換えるとチェックが消えて
  オートベクトル化も効きやすい
- [`save_preview`](aexlo/src/instance.rs#L743) は呼ぶたび ~8 MB の `Vec` を確保。
  live プレビューで毎フレーム呼ばれる前提の API なんやから、こっちもバッファ再利用の余地あり
  (`render → save` のループなら `write_output_rgba` + 呼び出し側バッファで既に回避可能ではある)

### 2.6 handle suite: 1 handle につき 2 回の malloc

[`host_new_handle_impl`](aexlo/src/suites/handle.rs#L97-L117) はデータブロックと
handle スロット (ポインタ 1 個分) を**別々に alloc** しとる。AE プラグインは
sequence data のたびに handle を作るから、細かい 8 byte alloc が積もる。
ヘッダー領域 (magic + size で 16 byte 使用中) に handle スロットも同居させれば
1 alloc に統合できる (handle 自体が「データブロック内の固定オフセットを指すポインタ」になる)。

優先度は低いが、`MAX_REASONABLE_SIZE` (1GB) のマジックナンバーが
handle.rs 内に 3 回コピペされとるのも const 1 個にまとめたい。

### 2.7 依存の重複: PNG エンコーダーが 2 系統

- コア (`aexlo`) は `save_preview` で **mtpng**
- CLI と watch は `image::save_buffer` / `image::open`

PNG エンコード実装が 2 つバイナリに入り、コンパイル時間とバイナリサイズを両方食っとる。
`image` は入力デコードに必要やから、出力も `image` の PNG エンコーダーに寄せるか、
逆に速度重視で mtpng に統一して `image` は decode 機能だけ有効化する
(`default-features = false, features = ["png"]`) のがええ。

### 2.8 その他 (小粒)

- [`pid_alive`](aexlo/src/instance.rs#L160): 生存確認のたびに `kill` を **子プロセスとして spawn**。
  live プレビューの保存ごとに呼ばれる。unix なら `libc::kill(pid, 0)` 一発 (すでに FFI だらけの
  クレートで libc を避ける意味は薄い)
- [`iterate_8_sys`](aexlo/src/suites/iterate.rs#L152-L157): ピクセルごとに
  `current_x * pixel_size` の乗算でアドレス計算。行頭ポインタからの逐次 `add(pixel_size)` に
  変えると素直やが、ここはコンパイラーが強度低減してくれる可能性が高いので測ってから
- `params_ptr_cache` の遅延再構築 ([instance.rs:1337-1340](aexlo/src/instance.rs#L1337-L1340)) は
  ええ設計や。ただし `params` の要素を `&mut` で触る他のメソッド (`update_param_ui` など) は
  Vec 再確保を起こさんから dirty 不要、という前提がコードに書かれとらんので一言コメント推奨

---

## 3. 改善ロードマップ (推奨順)

| 優先度 | 項目 | 種別 | 工数感 |
|---|---|---|---|
| P0 | 1.1 パラメーターインデックス統一 (`param_values` バグ + CLI 名前ズレ修正) | バグ | 小 |
| P0 | 1.2 `FIX_SLIDER` の 16.16 変換修正 + e2e テスト | バグ | 小 |
| P0 | 2.1 診断の eager format 排除 (`diag!` マクロ化) | 性能 | 小〜中 |
| P1 | 1.4 `Drop` で SETDOWN 送信 | API | 小 |
| P1 | 1.8 `checkout_layer_pixels` の `expect` → エラー返却 (FFI unwind 防止) | 健全性 | 極小 |
| P1 | 1.11 `as_sys` の `as_mut_ptr` 化 | 健全性 | 極小 |
| P1 | 2.2 ホットパス logging のレベル調整 | 性能 | 極小 |
| P1 | 2.3 GPU staging 再利用 + 入力 dirty フラグ | 性能 | 中 |
| P2 | 1.3 フレームサイズ設定 API (ビルダー) + 定数一本化 | API | 中 |
| P2 | 1.5 sys 型の公開停止 (`in_data`/`pica`/`params` の隠蔽) | API | 中 (破壊的) |
| P2 | 1.6 プレビュー/ビューアの dev-tool クレート分離 + Windows `pid_alive` 修正 | API | 中 |
| P2 | 2.4 CUDA 同期削減 | 性能 | 小 |
| P3 | 1.7 `PLUGIN_PATH` のインスタンス化 | API | 小 |
| P3 | 1.9 エラー型の文脈強化 | API | 小 |
| P3 | 1.10 iterate スタブのエラー化 or 実装 | API | 小〜中 |
| P3 | 2.5〜2.7 変換ループ最適化・handle 統合 alloc・PNG 依存統一 | 性能 | 小 |

**計測の勧め:** `benches` クレートが既にあるんやから、P0/P1 の性能修正は
`render_matrix` ベンチで before/after を取ってから入れるとええ。特に 2.1 は
diagnostics 無効ビルドでも数 % 単位で効く可能性がある (world/checkout 系 callback の頻度次第)。

---

## 4. 対応状況 (2026-07-14 更新)

| 項目 | 状態 | コミット |
|---|---|---|
| 1.1 インデックス統一 | ✅ 修正済み (公開 index = 内部 index に統一。0 = レイヤー) | c08acab |
| 1.2 FIX_SLIDER 16.16 | ✅ 修正済み + 回帰テスト | c08acab |
| 2.1 診断の eager format | ✅ `diag!` マクロ化、feature 無効時ゼロコスト | 8a26b81 |
| 1.4 Drop で SETDOWN | ✅ GPU → SEQUENCE → GLOBAL をベストエフォート送信 | e5d5733 |
| 1.8 callback の panic / `expect` | ✅ エラー返却化 (unwind 防止) | 8a26b81 |
| 1.11 `as_sys` provenance | ✅ `as_mut_ptr` 化 | e5d5733 |
| 2.2 ホットパス logging | ✅ debug/trace 降格 | e5d5733 |
| 2.3 GPU staging | ✅ バッファ再利用 + 入力 dirty フラグ | 0405ab8 |
| 1.3 フレームサイズ API | ✅ `set_render_size` + 定数一本化 + checkout 実寸法 | 6b2b76a |
| 1.5 sys 型の公開停止 | ✅ `param_name` 追加、in_data/pica ほか非公開化 | 47c31d9 |
| 1.6 プレビュー分離 | ✅ `preview` モジュール化 + Windows `pid_alive` 修正 | e8a3ac0 |
| 1.7 PLUGIN_PATH | ✅ インスタンス保持化 | bbc08e7 |
| 1.9 エラー文脈 | ✅ 失敗コマンド名を付与、`LayerError` を `#[from]` | bbc08e7 |
| 1.10 iterate スタブ | ✅ `iterate_generic` 実装、残り 3 種はエラー返却 | aabaebb |
| 2.4 CUDA 同期 | ⚠️ 冗長な二重 sync のみ削除 (upload/readback の削減は CUDA 実機での検証待ち) | 285aa3b |
| 2.5 変換ループ | ✅ `write_rgba_bytes` chunks 化 | e5d5733 |
| 2.7 PNG 依存 | ✅ CLI render を mtpng に統一 | 285aa3b |
| 2.6 handle 統合 alloc | ⏳ 未対応 (優先度低) | — |
| 分析中に発見した追加バグ | ✅ `checkout_layer` の結果未書き込み / `copy` の O(n²)+OOB / AngleParam スタブ | 8a26b81 |

---

*Generated by Claude (Fable 5) — 対象コミット 575793f、対応状況は 285aa3b 時点*
