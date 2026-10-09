# サンプル

[English](README.md) | 日本語

まずは [aexlo をはじめる](../docs/getting-started.ja.md)を参照してください。すべてのコマンドはリポジトリのルートで実行します。基本的なサンプルは同梱の SDK_Noise フィクスチャを使うため、After Effects は必要ありません。

| サンプル | 内容 | コマンド |
| --- | --- | --- |
| [minimal](minimal/src/main.rs) | Host を通じて既存のプラグインを読み込み、ABOUT を取得する | `cargo run -p minimal` |
| [sdk_noise](sdk_noise/src/main.rs) | PNG 入力 → レンダリング → PNG 出力 | `cargo run -p sdk_noise` |
| [interactive](interactive/README.ja.md) | GUI でプラグインを選択し、パラメーターを編集する | `cargo run -p interactive --release` |
| [studio](studio/README.ja.md) | レイヤー、エフェクト、時間を組み合わせる | `cargo run -p studio --release` |

minimal と sdk_noise は `-- --help` で引数の説明を表示できます。sdk_noise は既定で `target/sdk-noise.png` に書き出します。自分のアプリケーションに API を組み込む際は、まずこの 2 つのサンプルを読んでください。

[playground](../playground/README.ja.md) はホストの動作を検証するツールで、[demos](../demos/) にはプラグイン側の実装があります。既存のプラグインをホストするアプリケーションを作る場合は、このサンプル集から始めてください。
