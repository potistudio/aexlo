# 互換プラグイン

[English](compatible-plugins.md) | 日本語

[`aexlo compat`](compat.ja.md) が不足なく動かせるプラグインの一覧です。
該当するシナリオがすべて `ok`、またはプラグインが宣言していないための `n/a`
なので、[カバレッジ](compat.ja.md#対応率coverage)は 100% になります。
aexlo は各プラグインを読み込み、宣言しているすべてのレンダーパスと深度で実行し、
入力・サイズ・時間を変え、すべてのパラメーターを宣言された値に設定し、
差が出る場合はテキストレイヤーでもレンダリングしています。
aexlo が提供していないスイート、ホスト関数、呼び出しを求めたものはありません。

## 一覧

macOS arm64、aexlo v0.3.0、2026-11-23 に
`fixtures/plugins/macos` の 34 プラグインを対象に測定した結果です。

| プラグイン | シナリオ | `ok` | `n/a` | 深度 | スマートレンダー | GPU |
| ---------- | -------: | ---: | ----: | ---- | ---------------- | --- |
| AOD_ChannelRemap           | 108 | 105 |  3 | 8, 16, 32 | あり | なし |
| AOD_ColorAjust             |  39 |  36 |  3 | 8, 16, 32 | あり | なし |
| AOD_ColorChange            | 153 | 150 |  3 | 8, 16, 32 | あり | なし |
| AOD_ColorComposite         | 1065 | 1062 |  3 | 8, 16, 32 | あり | なし |
| AOD_ColorConvert           |  45 |  42 |  3 | 8, 16, 32 | あり | なし |
| AOD_ColorQuantize          |  68 |  65 |  3 | 8, 16, 32 | あり | なし |
| AOD_ColorSelect            |  91 |  88 |  3 | 8, 16, 32 | あり | なし |
| AOD_ContourGenerate        |  37 |  34 |  3 | 8, 16, 32 | あり | なし |
| AOD_DifferentialGenerate   |  41 |  38 |  3 | 8, 16, 32 | あり | なし |
| AOD_DistanceGenerate       |  42 |  39 |  3 | 8, 16, 32 | あり | なし |
| AOD_FFT                    |  35 |  32 |  3 | 8, 16, 32 | あり | なし |
| AOD_FourierFilter          | 103 | 100 |  3 | 8, 16, 32 | あり | なし |
| AOD_IFFT                   |  35 |  32 |  3 | 8, 16, 32 | あり | なし |
| AOD_ImageCalculate         |  86 |  83 |  3 | 8, 16, 32 | あり | なし |
| AOD_LineRepaint            |  33 |  30 |  3 | 8, 16, 32 | あり | なし |
| AOD_NormalGenerate         |  51 |  48 |  3 | 8, 16, 32 | あり | なし |
| AOD_RegionColorize         |  42 |  39 |  3 | 8, 16, 32 | あり | なし |
| AOD_SingularValueDecompose |  29 |  26 |  3 | 8, 16, 32 | あり | なし |
| AOD_VoronoiGenerate        |  89 |  86 |  3 | 8, 16, 32 | あり | なし |
| BitonicPixelSorter         |  71 |  71 |  0 | 8, 16, 32 | あり | あり |
| Circle-Repeater            |  56 |  53 |  3 | 8, 16, 32 | あり | なし |
| Cosmic                     | 112 | 112 |  0 | 8, 16, 32 | あり | あり |
| DisplacerPro               | 106 | 101 |  5 | 8, 32 | あり | なし |
| Falloff                    | 149 | 149 |  0 | 8, 16, 32 | あり | あり |
| RXXR                       | 767 | 764 |  3 | 8, 16, 32 | あり | なし |
| SDK_Noise                  |  23 |  20 |  3 | 8, 16, 32 | あり | なし |
| TextGrad                   | 535 | 525 | 10 | 8, 32 | あり | なし |

合計 4,011 シナリオ（3,930 `ok`、81 `n/a`）です。

## まだ一覧にないプラグイン

フィクスチャディレクトリにある残り 7 プラグインは、次のいずれかに当たります。
いずれも aexlo 側の不足か、インストールされていないフィクスチャによるもので、
プラグイン自体が誤っていると分かっているわけではありません。

| プラグイン | カバレッジ | `compat` の報告 |
| ---------- | ---------: | ---------------- |
| DeepGlow2           |  99.5% | `error` ×2、SmartRender がエラー 516 で拒否 |
| Furikake            |  99.2% | `error` ×5、Render がエラー 516 で拒否 |
| RainyDay            |  99.1% | 読み込み時に `PF Effect Sequence Data Suite` v1 が不足 |
| AnimatedNoise       |  97.4% | `PF_Cmd_SMART_RENDER` で `crash` ×1 |
| FillColor           |  96.0% | `PF_Cmd_SMART_RENDER` で `crash` ×1 |
| AOD_MobiusTransform |  19.6% | `PF_Cmd_SMART_RENDER` と `PF_Cmd_RENDER` で `crash` ×37 |
| AutoFill2           |       — | 読み込めない: バンドルにバイナリがない |

不足を 1 つずつ埋める手順は [compat-workflow.ja.md](compat-workflow.ja.md) にあります。

## 一覧の作り直し

フィクスチャは各マシンにあり、リポジトリには含まれないため、この一覧は
1 回の実行のスナップショットです。自分の環境で作るには同じコマンドを
`--json` 付きで実行します。`coverage` が `1` のプラグインが互換です。

```bash
cargo build -p aexlo-cli
target/debug/aexlo compat fixtures/plugins/macos --json > compat.json
```

変更の前後で一覧が増えたか減ったかは、ベースラインを渡すと分かります。
[compat.ja.md](compat.ja.md#5-ベースライン) を参照してください。
