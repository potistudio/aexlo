# aexlo-bench

[English](README.md) | 日本語

[`aexlo`](../aexlo) を通じて実際の After Effects プラグインを実行し、計測するためのベンチマーク基盤です。特定のプラグインに依存しない設計で、任意の `.plugin` / `.aex` を指定するだけで、コードを変更せずにベンチマークできます。

## ベンチマーク対象

| ターゲット | 計測内容 |
| --- | --- |
| `render_matrix` | **プラグイン × モード × 解像度 × パラメーター**のマトリクスにおけるレンダリングスループット（ピクセル/秒）。 |
| `load` | プラグインの**読み込み・初期化**コスト。`try_load` = バイナリを開く + エントリーポイントの解決 + `GLOBAL_SETUP` + `PARAMS_SETUP`。 |
| `summary` | 単一の解像度でプラグインを横断して比較する、スループット順の**ランキング**。**CSV/JSON 出力**と GPU 対 CPU の速度比も含みます。 |

## 実行

```sh
# 既定：選定済みのプラグイン（FillColor、SDK_Noise、DeepGlow2）を全解像度で計測
cargo bench -p aexlo-bench

# ターゲットを 1 つ指定
cargo bench -p aexlo-bench --bench render_matrix
cargo bench -p aexlo-bench --bench load
cargo bench -p aexlo-bench --bench summary
```

## 環境変数による設定

すべて環境変数で制御でき、ベンチマークのコードを変更する必要はありません。

| 変数 | 意味 | 例 |
| --- | --- | --- |
| `AEXLO_BENCH_PLUGINS` | フィクスチャ名**または**プラグインの絶対パスをカンマで区切った一覧。`all` は同梱の全フィクスチャ。 | `all` · `SDK_Noise,DeepGlow2` · `/abs/MyFx.plugin` |
| `AEXLO_BENCH_RESOLUTIONS` | 計測する解像度名をカンマで区切った一覧。 | `1080p,4k` |
| `AEXLO_BENCH_PARAMS` | パラメーターのスイープ：`Name=v1,v2;Other=v3,v4`。各組み合わせを計測します。名前は宣言されたパラメーター名と大文字小文字を区別せず照合し、値は数値です。 | `Radius=100,500,1000` |
| `AEXLO_BENCH_INPUT` | 入力フレームとして渡す画像の**絶対**パス（各解像度にリサイズ）。既定は生成したグラデーション。 | `/abs/footage.png` |
| `AEXLO_BENCH_SAMPLES` | `summary` 専用：プラグインごとに時間計測するレンダリング回数（既定は 30）。 | `50` |
| `AEXLO_BENCH_OUT` | `summary` 専用：出力パスの接頭辞。`<prefix>.csv` と `.json` を保存します（既定は `target/aexlo-bench/summary`）。 | `/tmp/run1` |
| `AEXLO_DISABLE_GPU` | GPU 対応のエフェクトでも CPU レンダリング経路を強制します（`aexlo` の設定）。 | `1` |

```sh
# 外部プラグインを 1080p と 4K で計測
AEXLO_BENCH_PLUGINS=/path/to/MyEffect.plugin \
AEXLO_BENCH_RESOLUTIONS=1080p,4k \
  cargo bench -p aexlo-bench --bench render_matrix

# パラメーターのスイープ：値に応じてレンダリングコストがどう変わるかを確認
AEXLO_BENCH_PLUGINS=DeepGlow2 AEXLO_BENCH_PARAMS="Radius=100,500,1000" \
  cargo bench -p aexlo-bench --bench render_matrix

# 全フィクスチャのランキングを CSV/JSON に書き出す
AEXLO_BENCH_PLUGINS=all AEXLO_BENCH_OUT=/tmp/aexlo cargo bench -p aexlo-bench --bench summary
```

解像度は `512`（512×512）、`720p`、`1080p`、`4k` です。[`ALL_RESOLUTIONS`](src/lib.rs) を参照してください。

## レポート内容

- **対応機能**（`smart_render`、`gpu`、`param_count`）とすべての**パラメーター設定**をプラグインごとに一度表示するため、各測定値が既知の再現可能な設定に対応します。`AEXLO_BENCH_PARAMS` で上書きしなければ、プラグインは既定値で実行されます。
- **CPU 対 GPU：** GPU 対応のエフェクトは `cpu` と `gpu` の両経路で計測し、直接比較できます。`summary` には GPU の速度比が表示されます。
- **スループット**はピクセル/秒なので、フレームサイズにかかわらずエフェクトを比較できます。

## 補足・制約

- **8 ビットのみ。** 公開入力 API（`set_input` / `Layer<Depth8>`）は 8 ビットのピクセルのみを受け付けるため、*解像度*はスイープできますが、*ビット深度*はスイープできません。16/32 ビットの入力経路が実装されたら、[`src/lib.rs`](src/lib.rs) の `synthetic_input` に深度の軸を追加します。
- **GPU 経路は同梱のフィクスチャでは未検証です。** いずれも `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` を宣言していないため、`gpu` モードはこのフラグを宣言する外部プラグインでのみ有効になります。比較のコードはありますが、ローカルで検証できる対象がありません。
- **`AEXLO_BENCH_INPUT` は絶対パスにしてください。** ベンチマークのバイナリは `benches/` クレートのディレクトリを作業ディレクトリとして実行されるため、相対パスはそこを基準に解決されます（読み込みに失敗すると警告し、生成した入力にフォールバックします）。
- **プラグインのクラッシュで実行全体が中断することがあります。** プラグインはプロセス内で実行するため、巻き戻しできないパニック（SIGABRT）が発生するとベンチマークプロセスも終了します。回復可能なレンダリング・読み込みの*エラー*は警告を出してスキップしますが、クラッシュは回復できません。問題が判明しているプラグインを避けるには、`AEXLO_BENCH_PLUGINS` で対象を絞ってください。e2e の `render_one` ハーネスのような、プラグインごとの完全なプロセス分離は、今後の追加候補です。

## 共通の計測ループ

すべての計測は、`aexlo bench` がマニフェストのプリセットに対して使うものと同じ `aexlo_harness::bench::measure` を通じて行われます（[ツールキット仕様書](../../docs/toolkit.ja.md) §10）。`AEXLO_BENCH_*` の変数は暗黙のプリセットに相当する設定を記述し、ウォームアップとサンプリングの手順も同じです。ベースラインや性能低下のチェックには、設定をプリセットとして記述し、`aexlo bench --save-baseline` / `--baseline` を使ってください。
