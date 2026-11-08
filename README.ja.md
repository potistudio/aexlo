# aexlo — After Effects プラグインランタイム

[English](README.md) | 日本語

初めてこのリポジトリを使う場合は、[はじめに](docs/getting-started.ja.md)を読んでから、[サンプル](examples/README.ja.md)、GUI、[検証用 playground](playground/README.ja.md)を試してください。

> After Effects の外で After Effects プラグイン（.aex）を読み込み、実行し、レンダリングします。

## 概要

**aexlo** は、After Effects のプラグインランタイムをエミュレートする Rust クレートです。
After Effects 自体がプラグインに提供するインターフェースである AE Plugin SDK を再実装することで、`.aex` プラグインを読み込み、レンダリングします。

既定では、UI やアプリケーションのコールバックなど、ホストに依存する機能にはヘッドレスホストが応答します。
これらの機能をどう上書きし、独自のアプリケーションにどう組み込むかを自由に制御できます。
`AppHost` を実装し、プラグインの読み込み前に `Host::install` で設定してください。

## 背景

**Adobe After Effects** は、世界で最も広く使われている動画編集ソフトであり、強力なプラグインが数多く開発されています。
しかし、これらのプラグインはいずれも After Effects 内でのみ動作するように設計されています。

**aexlo** は After Effects のプラグインランタイムをエミュレートし、AE プラグインを After Effects の外で実行できるようにします。Wine が Linux 上で Windows アプリケーションを実行する仕組みに似ています。

## 想定ユーザー

- **プラグイン開発者**：After Effects を起動せずにプラグインをテストしたい方。
- **ソフトウェア開発者**：自身のソフトウェアを After Effects プラグインに対応させたい方。
- **アーティスト**：画像処理のワークフローに After Effects プラグインを組み込みたい方。

## 機能

- After Effects の外で After Effects プラグインを読み込み、レンダリングします。
- ワークフローに合わせて、SDK のコマンド、スイート、コールバックを選択的に上書き、または再実装できます。

## 開発状況

> [!WARNING]
> このプロジェクトは**現在、活発に開発中**です。
> 最新の進捗は [master ブランチ](https://github.com/potistudio/aexlo/tree/master)で確認できます。

## クイックスタート

### 必要な環境

- `rust-toolchain.toml` で指定された Rust ツールチェーン（現在は 1.99.0）
- Windows x64 または macOS arm64

> [!NOTE]
> After Effects プラグインは Windows と macOS のみを対象としているため、現在 Linux はサポートしていません。

### ライブラリとして使う

```bash
cargo add aexlo
```

```rust,no_run
use aexlo::{Depth8, Host, Layer, ParamValue, Time};

fn main() -> aexlo::Result<()> {
    // Windows では `.aex` ファイル、macOS では `.plugin` バンドルを渡します。
    let mut fx = Host::get().load("MyEffect.aex")?;
    println!("{}", fx.about()?);

    fx.set_input_layer(Layer::<Depth8>::black(1920, 1080));
    fx.set_param(1, ParamValue::Float(0.5))?;
    fx.set_time(Time::frame(0, 30));
    fx.render_frame()?;
    let output: Layer<Depth8> = fx.read_output()?;
    Ok(())
}
```

プラグインはネイティブコードとして、呼び出し元と同じプロセス内で動きます。信頼モデル、プロセス全体で共有される状態、スレッドの扱いは [docs.rs](https://docs.rs/aexlo) のクレートドキュメントを参照してください。

### リポジトリからビルドする

```bash
cargo build
```

## ツールキット

`aexlo.toml` に「この条件でこのプラグインをレンダリングする」という設定をプリセットとして一度記述すれば、`aexlo` CLI でテスト、ベンチマーク、プレビューを実行できます。
詳細は[ツールキット仕様書](docs/toolkit.ja.md)を参照してください。

CLI とツールキットのクレートは crates.io には公開していないため、CLI はリポジトリからインストールします。

```bash
cargo install --git https://github.com/potistudio/aexlo aexlo-cli
```

```toml
[plugin]
artifact = { macos = "build/MyGlow.plugin", windows = "build/MyGlow.aex" }

[defaults]
depth = [8, 16, 32]

[[preset]]
name   = "dot_glow"
input  = { generate = "dot", at = [320, 240], radius = 2 }
size   = [640, 480]
params = { Radius = 100.0, Mode = "Screen" }
```

```bash
aexlo presets                      # プリセットから展開されるバリアントを一覧表示
aexlo test --bless                 # 参照画像を保存。以降は `aexlo test` で比較
aexlo test --strict                # ガード領域、リーク、チェックアウトの追跡も実施
aexlo test --fuzz 100              # パラメーターをランダム化し、失敗ケースをプリセットとして出力
aexlo bench --save-baseline main   # 以降は aexlo bench --baseline main で比較
aexlo preview build/MyGlow.plugin --preset dot_glow
aexlo check                        # CI で実行するコマンド（.github/workflows/aexlo-check.yml）
```

各バリアントはワーカープロセスで実行されるため、プラグインがクラッシュしても、その実行だけが失敗します。
Rust プラグインでは `aexlo-test` を開発用依存関係に追加すると、フレームのアサーションを伴うプロセス内の `#[aexlo_test::test]` を利用できます。

## 実装の進捗

⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣶⣀ 89% (32/36)

> ○ 実装済み / △ 一部実装 / × 未実装

| CB Suites          | Effect Suites                    | Adv Effect Suites | その他            |
| ------------------ | -------------------------------- | ----------------- | ----------------- |
| ○ ANSI             | ○ AE App                         | ○ AE Adv App      | ○ Cache On Load   |
| ○ Batch Sampling   | ○ AngleParam                     | ○ AE Adv Item     | △ Channel         |
| ○ Color            | ○ ColorParam                     | ○ AE Adv Time     | ○ GPU Device      |
| ○ Color16          | △ Effect Custom UI Overlay Theme |                   | ○ Plugin Helper   |
| ○ ColorFloat       | △ Effect Custom UI               |                   | ○ Plugin Helper 2 |
| ○ Fill Matte       | ○ Effect UI                      |                   |                   |
| ○ Handle           | ○ Param Utils                    |                   |                   |
| ○ Iterate8         | ○ Path Data                      |                   |                   |
| ○ Iterate16        | ○ Path Query                     |                   |                   |
| ○ IterateFloat     | ○ PointParam                     |                   |                   |
| ○ Pixel Data       | × Effect Sequence Data           |                   |                   |
| ○ Pixel Format     |                                  |                   |                   |
| ○ Sampling8        |                                  |                   |                   |
| ○ Sampling16       |                                  |                   |                   |
| ○ SamplingFloat    |                                  |                   |                   |
| ○ World            |                                  |                   |                   |
| ○ World Transform  |                                  |                   |                   |

△ Effect Custom UI / Overlay Theme：テーマの値は返します。Drawbot は `PF_Event_DRAW` のあいだ描画します。CLI は `PF_Cmd_EVENT` を送らず、埋め込み側が送ります。

△ Channel：レイヤーは通常の 2D 画像であり、補助チャンネルはないものとして応答します。

### AEGP

⣿⣶⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ 11% (4/36)

| Project        | Streams           | Pixels                 | その他                 |
| -------------- | ----------------- | ---------------------- | ---------------------- |
| △ Layer        | △ Stream          | ○ World                | △ Color Settings       |
| △ Comp         | △ Dynamic Stream  | ○ Memory               | △ Register             |
| △ Item         | △ Keyframe        | ○ Iterate              | △ Utility              |
| △ Light        | ○ Persistent Data | △ Render               | × Camera               |
| △ Effect       | × Mask            | △ Render Options       | × QueryXform           |
| △ PF Interface | × Mask Outline    | △ Layer Render Options | × Math                 |
| × Proj         | × Text Document   | × Composite            | × Compute Cache        |
| × Footage      | × Text Layer      | × Canvas               | × Hash                 |
| × Collection   |                   | × Sound Data           | × Render Async Manager |
| × Marker       |                   |                        |                        |

△ Layer / Comp / Item / Light / Effect / PF Interface / Stream / Dynamic Stream / Keyframe：コンポジションそのものである固定の 1 レイヤーを返し、キーフレームはありません。
△ Render / Render Options / Layer Render Options：チェックアウトはエフェクト自身の入力を返します。
△ Color Settings：作業用スペースは sRGB で、カラーマネジメントはしません。
△ Register：アイドルフックは記録しますが、呼びません。
△ Utility：`AEGP_ExecuteScript` はスクリプトを実行しません。

### Drawbot

⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿ 100% (6/6)

| ○ Draw | ○ Supplier | ○ Surface | ○ Path |
| ------ | ---------- | --------- | ------ |
| ○ Pen  | ○ Image    |           |        |

文字は OS のサンセリフを使います。見つからないとき、`DrawString` は何も描きません。

## ライセンス

このプロジェクトは [MIT License](LICENSE) に基づいて公開されています。
