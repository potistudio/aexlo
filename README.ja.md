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

プラグインの動作を調べるには、ターミナル UI のプロンプトからコマンドを実行します。

```bash
aexlo inspect build/MyGlow.plugin            # TUI。コマンド一覧は `:help`
aexlo inspect MyGlow --exec "render; set Radius 50; render smart"
```

送ったコマンドと、プラグインが呼び出したホストのスイート関数をすべてトレースし、関数ごとに集計します。
パラメーター、`PF_InData` / `PF_OutData` の状態、プラグインが保持するハンドルとワールドの16進ダンプ（直前のコマンドで変化したバイトを強調）、出力フレームも表示します。
トレースの行にフォーカスすると（Enter または `:focus <text|#seq>`）、その行を診断します。呼び出しなら引数と戻り値、実行中のコマンド、確保した・引数で渡されたメモリとそれがまだ保持されているか、失敗したコマンドならエラーとその直前の呼び出しを表示します。
`--exec` は同じ分析をテキストで出力します。クレートのディレクトリを渡した場合、`:reload` で再ビルドし、パラメーターの値を引き継ぎます。

エージェントやスクリプトから使う場合は、`--format json` で1行に1つの JSON オブジェクトを出力します。
まず `loaded`、コマンドごとに `command`（結果、発行した `PF_Cmd_*` とそのエラー、`focus` に渡せる `#seq`）、`focus` は診断をセクションに分けて、最後に `report` を出力します。
プラグイン自身の出力は stderr に回るため、stdout はそのままパースできます。
`--script -` は stdin からコマンドを読み、1つ答えてから次を読むので、読み込んだプラグインをパイプ越しに操作できます。`report` でその時点の分析を出力し、`clear` でそれまでのトレースを消し、`quit` で終了します。

```bash
aexlo inspect MyGlow --format json --exec "render; focus checkout"
aexlo inspect MyGlow --format json --script -    # stdin に1行1コマンド
```

オプション、コマンド、JSON のフィールドはすべて [docs/inspect.ja.md](docs/inspect.ja.md) で説明しています。

自分で書いていないプラグインを aexlo がどこまで動かせるかは、`aexlo compat` で調べます。
各プラグインを読み込み、すべてのレンダーパスと深度、別の入力・サイズ・時間、各パラメーターを別の値にした状態で、シナリオごとに1つのワーカーで実行します。
シナリオごとに `ok`、`partial`、`n/a`、`error`、`crash`、`timeout` を報告し、プラグインが求めたのに aexlo が応えなかったスイート、スタブ、呼び出しを、多い順に一覧にします。
ベースラインを保存しておけば、aexlo の変更で悪化したシナリオが分かります。

```bash
aexlo compat fixtures/plugins/macos --save-baseline compat.json
aexlo compat fixtures/plugins/macos --baseline compat.json   # 悪化があれば終了コード 1
```

詳しくは [docs/compat.ja.md](docs/compat.ja.md) を参照してください。

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
旧 Gaussian カーネル生成は近似値を使用し、After Effects との係数比較は未実施です。旧 Blend／Convolve は現在 8 ビット画像に対応します。

### AEGP

⣿⣿⣶⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ 14% (5/36)

| Project        | Streams           | Pixels                 | その他                 |
| -------------- | ----------------- | ---------------------- | ---------------------- |
| △ Layer        | △ Stream          | ○ World                | △ Color Settings       |
| △ Comp         | △ Dynamic Stream  | ○ Memory               | △ Register             |
| △ Item         | △ Keyframe        | ○ Iterate              | △ Utility              |
| △ Light        | ○ Persistent Data | △ Render               | × Camera               |
| △ Effect       | × Mask            | △ Render Options       | × QueryXform           |
| △ PF Interface | △ Mask Outline    | △ Layer Render Options | △ Math                 |
| △ Proj         | △ Text Document   | × Composite            | △ Compute Cache        |
| × Footage      | ○ Text Layer      | × Canvas               | △ Hash                 |
| × Collection   |                   | × Sound Data           | × Render Async Manager |
| × Marker       |                   |                        |                        |

△ Layer / Comp / Item / Light / Effect / PF Interface / Stream / Dynamic Stream / Keyframe：`set_project` で渡されたプロジェクト、渡されなければエフェクトのレイヤー 1 枚のコンポジションを読み取り専用で返します。
キーフレームは線形か停止のみで、ベジェのイージング、空間接線、エクスプレッションはありません。checkout で別の時刻を指定しても、レイヤーの画素は変わりません。
△ Mask Outline / Text Document：シェイプのパスとソーステキストの文字列を読めます。書き込みはできず、レイヤーマスクはストリームにしていません（Path Query で読みます）。
シェイプレイヤーのストリームはパス、グループ、塗り、パスの結合で、線とパラメトリックシェイプはありません。
△ Math：`AEGP_MatrixDecompose4` の角度の規約は After Effects と照合していません。
このレイヤーにはホスト中のエフェクトが 1 つあり、入力画像を表す仮想フッテージアイテムが対応します。
Effect Suite はロード中のプラグインが申告した識別情報を返し、システムにインストールされた全エフェクトは列挙しません。
△ Proj：未保存の仮想プロジェクトを 1 つ公開します。編集、保存、ルートフォルダー、時間表示設定は未実装です。
△ Render / Render Options / Layer Render Options：チェックアウトはエフェクト自身の入力を返します。
△ Color Settings：作業用スペースは sRGB で、カラーマネジメントはしません。
△ Register：アイドルフックは記録しますが、呼びません。
△ Utility：スクリプトエンジンはありません（`AEGP_IsScriptingAvailable` はそう答え、`AEGP_ExecuteScript` はエラーを返します）。取り消し履歴とパレットの UI もありません。macOS の `AEGP_GetPluginPlatformRef` はバンドルを返しません。
△ Compute Cache：値はクラス登録の解除またはプラグイン終了まで保持し、メモリー不足時の退避は未実装です。
△ Hash：決定的なホスト内ハッシュを提供しますが、After Effects と同じ値は保証しません。

### Drawbot

⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿ 100% (6/6)

| ○ Draw | ○ Supplier | ○ Surface | ○ Path |
| ------ | ---------- | --------- | ------ |
| ○ Pen  | ○ Image    |           |        |

文字は OS のサンセリフを使います。見つからないとき、`DrawString` は何も描きません。

## ライセンス

このプロジェクトは [MIT License](LICENSE) に基づいて公開されています。
