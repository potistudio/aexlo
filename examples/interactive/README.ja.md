# 対話型 playground

[English](README.md) | 日本語

aexlo を通じて既存の After Effects プラグインを読み込み、パラメーターを編集しながら出力をプレビューできるデスクトップアプリケーションです。UI は eframe / egui を使っています。初めてこのリポジトリを使う場合は、[はじめに](../../docs/getting-started.ja.md)で minimal と sdk_noise を先に試してください。

## 実行

リポジトリのルートで実行します。

```sh
cargo run -p interactive --release
# 拡張子を除いた名前で同梱のプラグインを選択します（ファイルパスではありません）。
cargo run -p interactive --release -- SDK_Noise
```

アプリケーションは `fixtures/plugins/macos/` または `fixtures/plugins/windows/` 内のプラグインを一覧表示します。既定の選択は SDK_Noise です。プラグインが見つからない場合、UI にエラーが表示されます。実行には、GUI と GPU にアクセスできるデスクトップ環境が必要です。

## 最初の操作

1. 左側の `Plugin` が SDK_Noise に設定されていることを確認します。
2. `Parameters` の `Noise variation` を変更し、右側のプレビューを確認します。
3. `Auto-render (animate)` をオフにし、`Render once` をクリックして 1 フレームずつ処理します。
4. ステータスで、読み込み・レンダリングのエラー、FPS、画像サイズ、smart または legacy のレンダリング経路を確認します。

UI には、数値や真偽値など、対応している型のパラメーターが表示されます。任意のプラグインのすべてのパラメーターが表示されるとは限りません。実装の入口は [src/main.rs](src/main.rs) です。

複数のレイヤーとファイルの書き出しには [studio](../studio/README.ja.md) を試してください。ホスト API の応答を調べるには[検証用 playground](../../playground/README.ja.md) を使ってください。
