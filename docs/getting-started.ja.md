# aexlo をはじめる

[English](getting-started.md) | 日本語

aexlo は、既存の After Effects プラグインを読み込み、After Effects の外で画像を処理する Rust 製のホストです。まずは同梱の SDK_Noise を試してから、自分のプラグインを使ってみましょう。以下のコマンドはリポジトリのルートで実行してください。

## 1. プラグインを読み込む

- rustup を使って Rust と Cargo をインストールしてください。使用するツールチェーンはリポジトリの `rust-toolchain.toml` で指定されています（現在は 1.99.0）。
- 使用するプラットフォーム向けにビルドされたプラグインを用意してください。主な対象は macOS ARM64 と Windows x64 です。Linux はサポートしていません。
- 同梱のサンプルを実行する際に After Effects は必要ありません。Adobe のホストとの動作比較には After Effects が必要です。

```sh
rustup show
cargo run -p minimal
```

初回は依存関係をダウンロードし、サンプルをビルドします。成功すると `Loading ...SDK_Noise...` に続いて、プラグインの ABOUT メッセージが表示されます。

## 2. 画像をレンダリングする

```sh
cargo run -p sdk_noise
```

同梱の `input.png` を読み込み、`target/sdk-noise.png` に書き出します。最後の `Saved ...` メッセージには出力サイズが含まれます。[ソースコード](../examples/sdk_noise/src/main.rs)を、プラグインの読み込み、入力レイヤーの設定、レンダリング、PNG の保存という順に読んでみてください。

自分のプラグインや画像を使う場合は、`--` で Cargo の引数とアプリケーションの引数を区切ります。空白を含むパスは引用符で囲んでください。

```sh
# macOS の例。Windows では .aex のパスを指定してください。
cargo run -p minimal -- "/path/to/MyEffect.plugin"
cargo run -p sdk_noise -- "/path/to/MyEffect.plugin" "input.png" "target/my-effect.png"
cargo run -p sdk_noise -- --help
```

明示的に指定した相対パスは、現在の作業ディレクトリを基準に解決されます。既定のフィクスチャのパスは、作業ディレクトリにかかわらずリポジトリを基準に解決されます。サードパーティーのプラグインでは、追加のライブラリやライセンスが必要な場合があります。

## 3. GUI でパラメーターを試す

```sh
cargo run -p interactive --release
```

左側の `Plugin` で同梱のプラグインを選択します。`Parameters` の値を変え、右側のプレビューを確認してください。`Auto-render (animate)` をオフにして `Render once` をクリックすると、1 フレームずつ確認できます。ステータスでエラーと選択中のレンダリング経路を確認してください。詳細は [interactive ガイド](../examples/interactive/README.ja.md)を参照してください。

複数のレイヤーを扱うには、`cargo run -p studio --release` でコンポジターを開きます。プロジェクトの保存とフレームの書き出しは [studio ガイド](../examples/studio/README.ja.md)で説明しています。

## 4. playground でホストの動作を調べる

```sh
cargo run -p playground -- run --in-process
cargo run -p playground -- report target/probe/trace-aexlo.jsonl
```

playground のプローブは、API 呼び出しに対するホストの応答を計測します。トレースは `target/probe/trace-aexlo.jsonl` に、画像は `target/probe/preview-aexlo.png` に保存されます。レポートの `unavailable` は、プローブが取得できなかったスイートを示します。報告された測定結果（fact）やスイートの結果を確認してください。終了コードだけでは API の対応範囲はわかりません。

動的ライブラリの読み込みも検証するには、`cargo run -p playground -- run` を使ってください。[playground ガイド](../playground/README.ja.md)では、After Effects との比較方法を説明しています。

## 5. プリセットで検証を繰り返す

```sh
cargo run -p aexlo-cli -- presets --manifest fixtures/aexlo.toml
cargo run -p aexlo-cli -- test --manifest fixtures/aexlo.toml
```

`fixtures/aexlo.toml` には、入力、サイズ、ビット深度、パラメーター、チェックが定義されています。`passthrough` プリセットはノイズを無効にし、結果を用意された参照画像（golden）と比較します。`noisy` プリセットはランダムなノイズを使うため、golden との比較や決定性などの一部のチェックを無効にしています。これらの設定の理由は、マニフェストのコメントを参照してください。

`--bless` は参照画像を更新します。最初の実行では既存の参照画像を使い、期待する出力が変わったことを確認してから更新してください。マニフェストの設定と開発サイクルは[ツールキット仕様書](toolkit.ja.md)を参照してください。

## リポジトリの構成

| ディレクトリ | 役割 | 調べるタイミング |
| --- | --- | --- |
| `examples/` | ホストを利用するアプリケーション | まずはここから：minimal → sdk_noise → interactive |
| `fixtures/` | 既存のプラグインと検証設定 | サンプルの入力を確認したいとき |
| `crates/aexlo/` | ホストの実装と公開 API | 自分のアプリケーションにホストを組み込みたいとき |
| `crates/cli/` | 検証・プレビュー用 CLI | 同じ条件で実行を繰り返したいとき |
| `demos/` | ソースからビルドするデモプラグイン | プラグイン側のコードを調べたいとき |
| `playground/` | ホスト API のプローブと実行ツール | 互換性や未対応 API を調べたいとき |

## トラブルシューティング

- **プラグインが見つからない：** パスとプラットフォームごとの拡張子を確認してください。macOS では `.plugin` バンドル全体、Windows では `.aex` ファイルを指定します。フィクスチャが Git LFS のポインターの場合は、`git lfs pull` で実体を取得してください。
- **ビルドに失敗する：** `rustup show` で選択中のツールチェーンを確認し、最初のコンパイラーエラーを読んでください。デモプラグインのビルドには、プラットフォームに対応したリンカーも必要です。
- **画像を開けない：** 入力パスを確認し、PNG を使ってください。このサンプルの画像デコーダーは PNG のみを有効にしています。
- **プラグインは読み込めるがレンダリングに失敗する：** まず SDK_Noise で同じ手順を試し、次に対象プラグインのエラーを確認してください。ホストがプラグインに必要なすべての機能を提供しているとは限りません。
- **GUI が起動しない：** GPU にアクセスできるデスクトップ環境を使ってください。minimal、sdk_noise、CLI はターミナルから実行できます。
