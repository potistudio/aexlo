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
| AnimatedNoise              |  42 |  39 |  3 | 8, 16, 32 | あり | なし |
| BitonicPixelSorter         |  71 |  71 |  0 | 8, 16, 32 | あり | あり |
| Circle-Repeater            |  56 |  53 |  3 | 8, 16, 32 | あり | なし |
| Cosmic                     | 111 | 111 |  0 | 8, 16, 32 | あり | あり |
| DeepGlow2                  | 442 | 432 | 10 | 8, 32 | あり | なし |
| DisplacerPro               | 106 | 101 |  5 | 8, 32 | あり | なし |
| Falloff                    | 148 | 148 |  0 | 8, 16, 32 | あり | あり |
| FillColor                  |  28 |  25 |  3 | 8, 16, 32 | あり | なし |
| Furikake                   | 610 | 604 |  6 | 8, 16, 32 | あり | なし |
| RXXR                       | 767 | 764 |  3 | 8, 16, 32 | あり | なし |
| RainyDay                   | 115 | 115 |  0 | 8, 16, 32 | あり | あり |
| SDK_Noise                  |  23 |  20 |  3 | 8, 16, 32 | あり | なし |
| TextGrad                   | 535 | 525 | 10 | 8, 32 | あり | なし |

合計 5,246 シナリオ（5,143 `ok`、103 `n/a`）です。

## まだ一覧にないプラグイン

フィクスチャディレクトリにある残り 2 プラグインは、次のいずれかに当たります。
いずれも aexlo 側の不足か、インストールされていないフィクスチャによるもので、
プラグイン自体が誤っていると分かっているわけではありません。

| プラグイン | カバレッジ | `compat` の報告 |
| ---------- | ---------: | ---------------- |
| AOD_MobiusTransform |  19.6% | `PF_Cmd_SMART_RENDER` と `PF_Cmd_RENDER` で `crash` ×37 |
| AutoFill2           |       — | 読み込めない: バンドルにバイナリがない（Windows 版は互換です。下を参照） |

不足を 1 つずつ埋める手順は [compat-workflow.ja.md](compat-workflow.ja.md) にあります。

## Windows のプラグイン

Windows 10 x64、aexlo v0.3.0、2026-11-24 に `fixtures/plugins/windows` を
対象に測定した結果です。

| プラグイン | カバレッジ | シナリオ | `ok` | `n/a` | `crash` | 深度 | スマートレンダー | GPU |
| ---------- | ---------: | -------: | ---: | ----: | ------: | ---- | ---------------- | --- |
| AutoFill2   | 100%  | 474 | 469 | 5 | 0 | 8, 32 | あり | なし |
| HEISEI_DEMO | 98.1% |  60 |  52 | 7 | 1 | 8, 16 | なし | なし |

AutoFill2 は Windows で互換です。`PF_Cmd_SMART_PRE_RENDER` で、
ワイヤーバージョン 5（`AEGP_RenderSuite4`）の "AEGP Render Suite" を
取得し、それを通して `AEGP_GetCurrentTimestamp` を呼びます。aexlo はこの呼び出しを
断りますが、プラグインはタイムスタンプなしで処理を続けます。
フィクスチャはライセンス未登録のため、出力に赤い × 印が入ります。

HEISEI_DEMO は Windows 専用のため、上の macOS フィクスチャには含まれません。
カバレッジは 98.1% で、1 つのシナリオがプラグイン自身のコードでクラッシュするため、
一覧にも入っていません。

HEISEI_DEMO はスイートを 1 つも求めません。
該当するシナリオは `size:1x1` を除いてすべて `ok` になり、`size:1x1` だけが
`crash` になります。このクラッシュは
aexlo の不足ではなくプラグイン自身のバグです。どのホストで動かしても、
プラグイン自身のコードがヌルポインタを参照します。
このプラグインは、レイヤーを `(幅 / Factor) x (高さ / Factor)` ピクセルの
バッファに縮小します。`Factor`（既定値 4）は小数点以下を切り捨てた整数として
扱われます。レイヤーの幅か高さが `Factor` より小さいとそのバッファが空になり、
プラグインはヌルポインタの先を読みます。`Factor` を 1 にすると、
1x1 のレイヤーもレンダリングできます。`tests/e2e/tests/heisei_demo.rs` では、
`Factor` が収まるレイヤーをレンダリングできることを確認しています。
クラッシュそのものは、`compat` の測定で確認しています。

正しく出力されるのは 8 bpc だけです。このプラグインは 16 bpc
（`PF_OutFlag_DEEP_COLOR_AWARE`）を宣言していますが、16 bpc のワールドを
8 bpc のピクセルとして読み書きします。そのため、各行の左半分は崩れた絵になり、
右半分は書き込まれないまま残ります。`compat` はプラグインが動くことしか
確認しないので、このシナリオを `ok` と数えます。

## 一覧の作り直し

フィクスチャは各マシンにあり、リポジトリには含まれないため、この一覧は
1 回の実行のスナップショットです。自分の環境で作るには同じコマンドを
`--json` 付きで実行します。`coverage` が `1` のプラグインが互換です。

```bash
cargo build -p aexlo-cli
target/debug/aexlo compat fixtures/plugins/macos --json > compat.json
```

Windows では、代わりに `fixtures/plugins/windows` を指定して実行します。

変更の前後で一覧が増えたか減ったかは、ベースラインを渡すと分かります。
[compat.ja.md](compat.ja.md#5-ベースライン) を参照してください。
