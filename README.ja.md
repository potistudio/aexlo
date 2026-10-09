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
> 最新の進捗は [master ブランチ](https://github.com/potistudio/aexlo-rs/tree/master)で確認できます。

## クイックスタート

### 必要な環境

- `rust-toolchain.toml` で指定された Rust ツールチェーン（現在は 1.99.0）
- Windows x64 または macOS arm64

> [!NOTE]
> After Effects プラグインは Windows と macOS のみを対象としているため、現在 Linux はサポートしていません。

### ビルド

```bash
cargo build
```

## ツールキット

`aexlo.toml` に「この条件でこのプラグインをレンダリングする」という設定をプリセットとして一度記述すれば、`aexlo` CLI（`cargo install --path crates/cli`）でテスト、ベンチマーク、プレビューを実行できます。
詳細は[ツールキット仕様書](docs/toolkit.ja.md)を参照してください。

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
Rust プラグインでは `aexlo-test` を開発用依存関係に追加すると、フレームのアサーションを伴うプロセス内の `#[aexlo::test]` を利用できます。

## 実装の進捗

⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣶⣀ 91% (32/35)

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
| ○ Pixel Data       |                                  |                   |                   |
| ○ Pixel Format     |                                  |                   |                   |
| ○ Sampling8        |                                  |                   |                   |
| ○ Sampling16       |                                  |                   |                   |
| ○ SamplingFloat    |                                  |                   |                   |
| ○ World            |                                  |                   |                   |
| ○ World Transform  |                                  |                   |                   |

△ Effect Custom UI / Overlay Theme：aexlo は `PF_Cmd_EVENT` を送信しないため、描画コンテキストがありません。テーマの値は提供しますが、描画呼び出しでは何も描画しません。

△ Channel：レイヤーは通常の 2D 画像であり、補助チャンネルはないものとして応答します。

## ライセンス

このプロジェクトは [MIT License](LICENSE) に基づいて公開されています。
