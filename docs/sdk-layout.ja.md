# SDK レイアウトチェック — バインディングと SDK ヘッダーの比較

[English](sdk-layout.md) | 日本語

aexlo と `playground/probe` は、After Effects SDK の同じ Rust バインディング（`after-effects-sys`。`after_effects::sys` として再エクスポート）を使っています。
このバインディングのフィールドのオフセットが間違っていると、両方で同じように間違うので、probe と aexlo のトレースを比べても差は出ません。
特に問題になるのは、プラグインが書き込み aexlo が読み取る構造体です（`PF_OutData` など）。

`after-effects-sys` はレイアウトテストなしで生成されており、aexlo はその生成済みバインディングを使います。
そのため、本物のヘッダーと比べるものはほかにありません。
`aexlo_sdk_layout` クレート（`tests/sdk_layout`）が、2つに分けてこれを比べます。

1. `snapshot` バイナリが、SDK ヘッダーに対して C++ プログラムをコンパイルします。
   このプログラムは、各構造体の `sizeof` と `alignof`、各フィールドの `offsetof`・サイズ・アラインメントを出力します。
   バイナリはそれを `fixtures/sdk-layout/<os>-<arch>.json` に書き出します。
   SDK と C++ コンパイラが必要なので、メンテナーが実行して結果をコミットします。
   スナップショットに入るのは数値と名前だけで、ヘッダーの本文は入りません。
2. テストが、実行中のターゲットのスナップショットと `after_effects_sys` を比べます。
   比べるのは、各構造体の `size_of` と `align_of`、各フィールドの `offset_of!`・サイズ・アラインメントです。
   SDK がいらないので CI でも実行します。スナップショットのないターゲットでは ignored として報告されます。

```bash
cargo test -p aexlo_sdk_layout                         # 比較（スナップショットがあるターゲット）
cargo run -p aexlo_sdk_layout --bin snapshot           # 再生成（SDK が必要）
AE_SDK_DIR="/path/to/AE SDK" cargo run -p aexlo_sdk_layout --bin snapshot
```

- [1. 何を調べるか](#1-何を調べるか)
- [2. 不一致](#2-不一致)
- [3. スナップショットの再生成](#3-スナップショットの再生成)
- [4. 制限](#4-制限)

## 1. 何を調べるか

構造体の一覧は手書きではなく、ソースから作ります。
生成器は、`crates/aexlo/src` と `playground/probe/src` にある識別子のうち、バインディングの構造体・共用体・型エイリアスの名前になっているものをすべて集めます。
そこから、フィールドの型、関数ポインターのシグネチャ、エイリアスをたどります。
そのため、スイートの関数テーブルからは、その関数が受け取るすべての構造体がたどられます。
`PF_ParamDef` からは `PF_ParamDefUnion` のすべてのメンバーがたどられます。
ソースが名前を出さない構造体は、生成器の `EXTRA_ROOTS` に加えます。

各構造体のフィールドはすべて Rust のパスで並び、C++ プログラムは同じ名前の SDK フィールドを測ります。
bindgen による名前の変更は元に戻します。
`type_` は C では `type` です。
匿名メンバーのフィールド（`__bindgen_anon_1.x`）は C のフィールド `x` です。
名前付きメンバーが持つ匿名型（`PF_ParamDef__bindgen_ty_1`）は、そのメンバーの型として測ります。
今のバインディングには匿名メンバーがないので、`__bindgen_anon_1.x` の経路は試していません。
バインディングで中身がない構造体（C では宣言だけのもの）は、理由とともに `skipped` に入ります。

生成器は、バインディングの中に読めないフィールドや型エイリアスを見つけると止まります。
そのため、bindgen の出力の書き方が変わっても、調べる範囲が黙って狭まることはありません。

テストはスナップショットにあるすべての構造体を調べます。Rust 側は、同じ一覧から `build.rs` が生成します。
ほかに2つのテストが、スナップショットを最新に保ちます。

- `snapshot_covers_every_struct_the_sources_use` は、スナップショットにない構造体をソースが使い始めると失敗します。
  そうした構造体につながる名前は、スナップショットの `unchecked` に並んでいます。
  このテストはコメントも含めてすべての識別子を読むので、そうした構造体の名前をコメントに書いただけでも失敗します。
- `snapshot_is_of_the_locked_bindings` は、測ったときと違うバージョンの `after-effects-sys` が `Cargo.lock` にあると失敗します。

今スナップショットがあるのは Windows x64 だけです。
ほかのターゲットでは、3つのテストは理由付きで ignored として報告されます。
`--ignored` を付けると失敗し、何も比べずに成功することはありません。

## 2. 不一致

テストは、すべての不一致を種類ごとにまとめて、1回の失敗で報告します。

| 種類 | 意味 |
| ---- | ---- |
| structs the SDK headers do not declare | ヘッダーにない型がバインディングにある |
| structs the SDK only declares | ヘッダーは型を宣言しているが、定義していない |
| named differently in the SDK headers | ヘッダーでは、構造体かフィールドが古い名前になっている。レイアウトはその名前で比べる |
| `size_of` / `align_of` differs | 構造体全体 |
| field offsets / sizes / alignments differ | フィールド1つ |
| Rust fields the SDK struct does not have | ヘッダーにないフィールド名 |
| SDK fields the Rust struct does not have | アラインメントの詰め物では説明できず、どの Rust フィールドにも属さない SDK のバイト |

最後の種類は数値から見つけます。
各フィールドは、前のフィールドの終わりを自分のアラインメントに切り上げた位置より後ろには始まりません。
構造体は、最後のフィールドの終わりを構造体のアラインメントに切り上げた位置より後ろには終わりません。
どちらかの位置より後ろのバイトは、バインディングが名前を持たないフィールドのものです。

理由が分かっていて害のない不一致は、`tests/sdk_layout/src/allowlist.rs` に、不一致1つにつき1エントリ、理由とともに加えます。
どの不一致にも一致しないエントリがあるとテストは失敗するので、一覧が古くなることはありません。
代わりに比較を緩めてはいけません。

### 分かっている不一致

`after-effects-sys` 0.4.0 のバインディングは SDK 25.6 から生成されています。
Windows のスナップショットは 23.4 の SDK（`PF_AE_PLUG_IN_SUBVERS` 28）で測りました。
古いヘッダーでは、3つの名前が違います。
生成器はそれぞれを古い名前で測り（`RENAMED`）、オフセットとサイズは同じです。

| Rust | SDK 23.4 |
| ---- | -------- |
| `PF_ParamDef.name_do_not_use_directly` | `PF_ParamDef.name` |
| `PF_ANSICallbacksBlock` | `PF_ANSICallbacks` |
| `PF_ANSICallbacksBlock.unused_longA` | `PF_ANSICallbacks.ansi_procs` |

オフセット・サイズ・アラインメントが違うものはありません。

SDK 25.x で加わった構造体やフィールドは、23.4 のヘッダーにはないものとして報告されます。
今はそうしたものはありません。
出てきたら、新しいヘッダーで測るか、allowlist にエントリを加えます。

## 3. スナップショットの再生成

`after-effects-sys` を上げたとき、カバレッジのテストに求められたとき、新しい SDK ヘッダーを使うときに再生成します。

1. `AE_SDK_DIR` が指す場所に SDK を置きます。`Headers/` と `Util/` があるフォルダーです。
   既定はワークスペースの `refs/AE SDK` で、git は無視します。
2. `AESDK_ROOT` を外します。
   設定されていると、`after-effects-sys` は aexlo が使う生成済みのものではなく、自分でバインディングを生成するので、生成器は止まります。
3. `cargo run -p aexlo_sdk_layout --bin snapshot` を実行します。
   C++ コンパイラは、`crates/aexlo/build.rs` と同じく `cc` クレートで探します（Windows では MSVC）。
4. `cargo test -p aexlo_sdk_layout` を実行し、スナップショットを、それが必要になった変更と一緒にコミットします。

プログラムは、`after-effects-sys` の `wrapper.hpp` と同じ SDK ヘッダーを、同じ順番で include します。
Windows では、`after-effects-sys` と同じく `_WINDOWS` を定義します。
あわせて、`<windows.h>` がフィールド名をマクロに変えないように `NOMINMAX` も定義します。
また Windows では `Mach-O_prefix.h` を include しません。
`wrapper.hpp` はどの OS でもこれを include しますが、23.4 の SDK では `__MACH__` を定義するだけで、Windows のプラグインは include しないからです。

`wrapper.hpp` は、After Effects SDK にない `PrSDKStringSuite` を、Premiere SDK からコピーして自分で定義しています。
プログラムはこれを定義しません。
そのため、ソースが `PrSDKStringSuite` を使い始めると、再生成したスナップショットはこれを「SDK ヘッダーが宣言していない構造体」として報告します。
この場合は allowlist にエントリを加えます。新しい After Effects のヘッダーで測っても解決しません。

## 4. 制限

- ビットフィールドにはバイト単位のオフセットがありません。
  そのフィールドは `bitfield` として並び、その構造体ではバイトの抜けを調べません。
  今の Windows のバインディングにビットフィールドはありません。
- 共用体では、バインディングにない SDK メンバーが共用体より小さいと、抜けが出ないので見つかりません。
- バインディングにない SDK フィールドが、Rust のレイアウトにもともとある詰め物に収まる場合も見つかりません。
  たとえば SDK の `{ int a; int b; double c; }` と Rust の `{ a: i32, c: f64 }` は、どちらも `c` を 8 に置きます。
  `#pragma pack` の下では、フィールド本来のアラインメントで位置を見積もるので、さらに見逃しやすくなります。
- 生成器は Windows の MSVC でしか実行していません。
  macOS のスナップショットには Mac での実行が必要です。
  コードはビルド先のターゲットに合わせて、バインディングのファイル、コンパイラのフラグ、スナップショットの名前を選びますが、その経路は試していません。
