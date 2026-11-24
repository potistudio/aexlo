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

- After Effects の外で After Effects プラグインを読み込み、GPU、smart、legacy の各経路、8・16・32 bpc でレンダリングします。
- エフェクトをテキストレイヤーやシェイプレイヤーに置けるので、グリフやパスのデータを使うエフェクト（Discotext、Extrude）がそれを読めます。マスクも渡せます。
- エフェクトの周囲のプロジェクト（コンポジション、レイヤー、トランスフォーム、レイヤー時間）を渡せます。
- パラメーターにキーフレームを設定できます。プラグインがどの API で読んでも、同じ時刻なら同じ値です。
- コマンド、スイート呼び出し、プラグインが求めたが aexlo が対応していない機能を観測できます。
- ワークフローに合わせて、SDK のコマンド、スイート、コールバックを選択的に上書き、または再実装できます。

## 開発状況

> [!WARNING]
> このプロジェクトは**現在、活発に開発中**です。
> エラーなくレンダリングできても、同じ画素になるとは限りません。ホストの多くは After Effects と比較していません。
> 最新の進捗は [master ブランチ](https://github.com/potistudio/aexlo/tree/master)で確認できます。

## クイックスタート

### 必要な環境

- Rust 1.99.0 以上
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

### レイヤー、パス、キーフレーム

> [!NOTE]
> この節の API は 0.2.0 より新しいものです。[Unreleased](CHANGELOG.md) を参照してください。

プロジェクトを渡さない場合、エフェクトはコンポジションと同じ大きさのレイヤー 1 枚の上にあります。`set_text_layer` でそのレイヤーをテキストレイヤーにし、`set_mask_paths` でマスクを渡せます（時間で動くマスクは `set_mask_tracks`）。
コンポジション全体は `set_project` で渡します。エフェクトにシェイプレイヤーのパスを読ませるにはこれを使います。

```rust,no_run
use aexlo::{
    Comp, Depth8, Host, Keyframe, Layer, LayerContent, MaskPath, MaskVertex, ParamValue, PixelDepthKind, Project,
    ProjectLayer, ShapeFill, ShapeGroup, ShapeItem, ShapePath, Time, TimePoint, Track,
};

fn main() -> aexlo::Result<()> {
    let comp = Comp::new("Comp 1", 640, 360, TimePoint::whole_seconds(10));
    let triangle = MaskPath {
        vertices: vec![
            MaskVertex::corner(260.0, 60.0),
            MaskVertex::corner(370.0, 250.0),
            MaskVertex::corner(150.0, 250.0),
        ],
        closed: true,
        ..Default::default()
    };
    let group = ShapeGroup::new(
        "Triangle",
        vec![
            ShapeItem::Path(ShapePath::new("Path 1", triangle)),
            ShapeItem::Fill(ShapeFill::new([1.0; 4])),
        ],
    );
    let content = LayerContent::Shape(vec![ShapeItem::Group(group)]);
    let layer = ProjectLayer::new(1, "Shape Layer 1", content, &comp);
    // 画素は、After Effects が描くのに相当するものをホストが渡します。
    let input = layer
        .rasterize(PixelDepthKind::U8, TimePoint::ZERO, [1.0; 4])
        .expect("a shape layer");

    let mut fx = Host::get().loader("Extrude.aex").input(input).load()?;
    fx.set_project(Some(Project::new(comp, vec![layer], 1)?));

    // Extrude の「Extrusion Length Back」。
    let length = Track::keyed(vec![
        Keyframe::linear(TimePoint::frame(0, 30), ParamValue::Float(0.0)),
        Keyframe::linear(TimePoint::frame(30, 30), ParamValue::Float(200.0)),
    ])
    .expect("two different times");
    fx.set_param_track(27, length)?;

    fx.set_time(Time::frame(15, 30)); // 100
    fx.render_frame()?;
    let output: Layer<Depth8> = fx.read_output()?;
    Ok(())
}
```

プラグインからプロジェクトは読み取り専用です。プロジェクトを渡した場合、`set_time` はエフェクトのレイヤーの時間で、コンポジションの時間は `start + time * stretch` です。
キーフレームは線形か停止です。その他の制限は [AEGP](#aegp) にまとめています。

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
aexlo render --preset dot_glow -o dot_glow.png
aexlo check                        # CI で実行するコマンド（.github/workflows/aexlo-check.yml）
```

`aexlo help` でコマンドの一覧を、`aexlo help <command>`（または
`aexlo <command> --help`）でそのオプションを表示します。

各バリアントはワーカープロセスで実行されるため、プラグインがクラッシュしても、その実行だけが失敗します。
Rust プラグインでは `aexlo-test` を開発用依存関係に追加すると、フレームのアサーションを伴うプロセス内の `#[aexlo_test::test]` を利用できます。

レンダリングせずにプラグインの宣言内容を見るには `info` を使います。

```bash
aexlo info MyGlow             # PiPL、ABOUT テキスト、フラグ、パラメーター
aexlo info MyGlow --pipl      # PiPL だけを、プラグインを読み込まずに表示
```

プラグインの動作を調べるには、ターミナル UI のプロンプトからコマンドを実行します。

```bash
aexlo inspect build/MyGlow.plugin            # TUI。コマンド一覧は `:help`
aexlo inspect MyGlow --exec "render; set Radius 50; render smart"
```

`input text`、`input shape`、`mask`、`key` で、テキストレイヤー、シェイプレイヤー、マスク、キーフレームをエフェクトに渡せます。

```bash
aexlo inspect Extrude --exec "input shape ring; render smart; save ring.png"
aexlo inspect Discotext --exec "input text HELLO; render smart; save text.png"
```

送ったコマンドと、プラグインが呼び出したホストのスイート関数をすべてトレースし、関数ごとに集計します。
パラメーター、`PF_InData` / `PF_OutData` の状態、プラグインが保持するハンドルとワールドの16進ダンプ（直前のコマンドで変化したバイトを強調）、出力フレームも表示します。
トレースの行にフォーカスすると（Enter または `:focus <text|#seq>`）、その行を診断します。呼び出しなら引数と戻り値、実行中のコマンド、確保した・引数で渡されたメモリとそれがまだ保持されているか、失敗したコマンドならエラーとその直前の呼び出しを表示します。
`--exec` は同じ分析をテキストで出力します。クレートのディレクトリを渡した場合、`:reload` で再ビルドし、パラメーターの値を引き継ぎます。

エージェントやスクリプトから使う場合は、`--json` で1行に1つの JSON オブジェクトを出力します。
まず `loaded`、コマンドごとに `command`（結果、発行した `PF_Cmd_*` とそのエラー、`focus` に渡せる `#seq`）、`focus` は診断をセクションに分けて、最後に `report` を出力します。
プラグイン自身の出力は stderr に回るため、stdout はそのままパースできます。
`--script -` は stdin からコマンドを読み、1つ答えてから次を読むので、読み込んだプラグインをパイプ越しに操作できます。`report` でその時点の分析を出力し、`clear` でそれまでのトレースを消し、`quit` で終了します。

```bash
aexlo inspect MyGlow --json --exec "render; focus checkout"
aexlo inspect MyGlow --json --script -    # stdin に1行1コマンド
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
全シナリオを通るプラグインは[互換プラグイン](docs/compatible-plugins.ja.md)に一覧があります。

## 実装の進捗

⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣶⣀ 89% (33/37)

> ○ 実装済み / △ 一部実装 / × 未実装

| CB Suites          | Effect Suites                    | Adv Effect Suites | その他            |
| ------------------ | -------------------------------- | ----------------- | ----------------- |
| ○ ANSI             | ○ AE App                         | ○ AE Adv App      | ○ Cache On Load   |
| ○ Batch Sampling   | ○ AngleParam                     | ○ AE Adv Item     | △ Channel         |
| ○ Color            | ○ ColorParam                     | ○ AE Adv Time     | ○ GPU Device      |
| ○ Color16          | △ Effect Custom UI Overlay Theme |                   | ○ Plugin Helper   |
| ○ ColorFloat       | △ Effect Custom UI               |                   | ○ Plugin Helper 2 |
| ○ Fill Matte       | ○ Effect UI                      |                   | × PF Utility      |
| ○ Handle           | ○ Param Utils                    |                   |                   |
| ○ Iterate8         | ○ Path Data                      |                   |                   |
| ○ Iterate16        | ○ Path Query                     |                   |                   |
| ○ IterateFloat     | ○ PointParam                     |                   |                   |
| ○ Pixel Data       | ○ Effect Sequence Data           |                   |                   |
| ○ Pixel Format     |                                  |                   |                   |
| ○ Sampling8        |                                  |                   |                   |
| ○ Sampling16       |                                  |                   |                   |
| ○ SamplingFloat    |                                  |                   |                   |
| ○ World            |                                  |                   |                   |
| ○ World Transform  |                                  |                   |                   |

△ Effect Custom UI / Overlay Theme：テーマの値は返します。Drawbot は `PF_Event_DRAW` のあいだ描画します。CLI は `PF_Cmd_EVENT` を送らず、埋め込み側が送ります。

○ Batch Sampling：After Effects と同じく、スイートの関数はすべて NULL です（SDK では After Effects 7.0 で廃止）。画素単位のサンプラーは使えます。
○ Param Utils：状態は、指定された範囲の値、レイヤーの画素、その範囲のマスク、任意データを対象にします。任意データは、ホストが値を受け取るとき（セットアップ後と、プラグインが値を変えるコマンドの後）にプラグインが flatten した内容で比べます。`AUTOMATIC_WIDE_TIME_INPUT` のエフェクトでは、それまでのチェックアウトが届いた時刻まで範囲を広げます。レイヤーの画像は 1 枚なので、埋め込み側が別フレーム用の画像を設定すると変化として扱います。
○ World Transform：After Effects 2026 と同じ合成をします。全モードを 8・16・32 bpc で測った probe のトレースと照合し、32 bpc は完全一致、8・16 bpc は数階調以内です。例外は、ディザ合成の乱数パターン（規則は After Effects と同じで、乱数は aexlo 独自）と、両方のレイヤーが半透明な部分の色相・彩度・輝度です。`composite_rect` は After Effects と同じく描画モードを無視し、どのワールドも 8 bpc として扱います。
× PF Utility：AEGP Utility とは別のスイートです。取得はできますが、関数はすべてエラーを返すスタブです。
△ Channel：レイヤーは通常の 2D 画像であり、補助チャンネルはないものとして応答します。
未実装のコールバックは、成功を装わずエラーを返します。
旧 Gaussian カーネル生成は近似値を使用し、After Effects との係数比較は未実施です。

### AEGP

⣿⣿⣶⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ 14% (5/36)

実装済み 5、一部実装 22、未実装 9 です。

| Project        | Streams           | Pixels                 | その他                 |
| -------------- | ----------------- | ---------------------- | ---------------------- |
| △ Layer        | △ Stream          | ○ World                | △ Color Settings       |
| △ Comp         | △ Dynamic Stream  | ○ Memory               | △ Register             |
| △ Item         | △ Keyframe        | ○ Iterate              | △ Utility              |
| △ Light        | ○ Persistent Data | △ Render               | △ Camera               |
| △ Effect       | △ Mask            | △ Render Options       | × QueryXform           |
| △ PF Interface | △ Mask Outline    | △ Layer Render Options | △ Math                 |
| △ Proj         | △ Text Document   | × Composite            | △ Compute Cache        |
| × Footage      | ○ Text Layer      | × Canvas               | △ Hash                 |
| × Collection   |                   | × Sound Data           | × Render Async Manager |
| × Marker       |                   |                        |                        |

△ Layer / Comp / Item / Light / Effect / PF Interface：`set_project` で渡されたプロジェクトを読み取り専用で返します。渡されなければ、エフェクトのレイヤーと、入力を表す仮想フッテージアイテムを持つコンポジション 1 つです。
Effect Suite が扱うエフェクトはホスト中の 1 つと、ロード中のプラグインが申告した識別情報です。システムにインストールされた全エフェクトは列挙しません。
△ Stream / Dynamic Stream：トランスフォーム、エフェクトのパラメーター、ソーステキスト、シェイプの内容（パス、グループ、塗り、パスの結合）を、指定された時刻で返します。
レイヤーマスクのアウトライン、不透明度、ぼかし、拡張もストリームです（キーフレームなし）。線、パラメトリックシェイプ、エクスプレッションはストリームにしていません。プラグインはパラメーターの非表示・無効化と、キーフレームのないパラメーター（レイヤーと任意データを除く）への値の設定ができます。ほかの書き込みは拒否します。
△ Keyframe：読み取りのみで、線形か停止です。ベジェのイージングと空間接線はありません。別の時刻でチェックアウトしたレイヤーの画素は、現在時刻のものです。
△ Mask：埋め込み側が `set_mask_paths` か `set_mask_tracks` でエフェクトのレイヤーに付けたマスクを読み取ります。マスクは変更できず、ほかのレイヤーにはマスクがありません。
△ Mask Outline / Text Document：レイヤーマスクのアウトライン、シェイプのパス、ソーステキストの文字列を読めます。書き込みはできません。
△ Camera：v2 のみです。プロジェクトにカメラとライトのレイヤーはないため、カメラは「なし」と答え、既定カメラの距離を返します。
△ Math：`AEGP_MatrixDecompose4` の角度の規約は After Effects と照合していません。
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
