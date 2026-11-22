# Compatible plugins

English | [日本語](compatible-plugins.ja.md)

These are the plugins that [`aexlo compat`](compat.md) drives with no gaps:
every scenario that applies to them goes `ok`, or is `n/a` because the plugin
does not declare it, so their [coverage](compat.md#coverage) is 100%. aexlo
loaded each one, ran it on every render path and depth it declares, changed
its input, size and time, set every parameter to the values it declares, and
rendered it on a text layer too where that makes a difference. None of them
asked for a suite, host function or call that aexlo does not serve.

## The list

Measured on macOS arm64 with aexlo v0.3.0, on 2026-11-23, over the 34
plugins in `fixtures/plugins/macos`:

| Plugin | Scenarios | `ok` | `n/a` | Depths | Smart render | GPU |
| ------ | --------: | ---: | ----: | ------ | ------------ | --- |
| AOD_ChannelRemap           | 108 | 105 |  3 | 8, 16, 32 | yes | no |
| AOD_ColorAjust             |  39 |  36 |  3 | 8, 16, 32 | yes | no |
| AOD_ColorChange            | 153 | 150 |  3 | 8, 16, 32 | yes | no |
| AOD_ColorComposite         | 1065 | 1062 |  3 | 8, 16, 32 | yes | no |
| AOD_ColorConvert           |  45 |  42 |  3 | 8, 16, 32 | yes | no |
| AOD_ColorQuantize          |  68 |  65 |  3 | 8, 16, 32 | yes | no |
| AOD_ColorSelect            |  91 |  88 |  3 | 8, 16, 32 | yes | no |
| AOD_ContourGenerate        |  37 |  34 |  3 | 8, 16, 32 | yes | no |
| AOD_DifferentialGenerate   |  41 |  38 |  3 | 8, 16, 32 | yes | no |
| AOD_DistanceGenerate       |  42 |  39 |  3 | 8, 16, 32 | yes | no |
| AOD_FFT                    |  35 |  32 |  3 | 8, 16, 32 | yes | no |
| AOD_FourierFilter          | 103 | 100 |  3 | 8, 16, 32 | yes | no |
| AOD_IFFT                   |  35 |  32 |  3 | 8, 16, 32 | yes | no |
| AOD_ImageCalculate         |  86 |  83 |  3 | 8, 16, 32 | yes | no |
| AOD_LineRepaint            |  33 |  30 |  3 | 8, 16, 32 | yes | no |
| AOD_NormalGenerate         |  51 |  48 |  3 | 8, 16, 32 | yes | no |
| AOD_RegionColorize         |  42 |  39 |  3 | 8, 16, 32 | yes | no |
| AOD_SingularValueDecompose |  29 |  26 |  3 | 8, 16, 32 | yes | no |
| AOD_VoronoiGenerate        |  89 |  86 |  3 | 8, 16, 32 | yes | no |
| BitonicPixelSorter         |  71 |  71 |  0 | 8, 16, 32 | yes | yes |
| Circle-Repeater            |  56 |  53 |  3 | 8, 16, 32 | yes | no |
| Cosmic                     | 112 | 112 |  0 | 8, 16, 32 | yes | yes |
| DisplacerPro               | 106 | 101 |  5 | 8, 32 | yes | no |
| Falloff                    | 149 | 149 |  0 | 8, 16, 32 | yes | yes |
| RXXR                       | 767 | 764 |  3 | 8, 16, 32 | yes | no |
| SDK_Noise                  |  23 |  20 |  3 | 8, 16, 32 | yes | no |
| TextGrad                   | 535 | 525 | 10 | 8, 32 | yes | no |

That is 4,011 scenarios: 3,930 `ok` and 81 `n/a`.

## Not on the list yet

The other seven plugins in the fixture directory fall short in one of these
ways. Each is a gap in aexlo, or a fixture that is not installed, not a
plugin that is known to be wrong.

| Plugin | Coverage | What `compat` reports |
| ------ | -------: | --------------------- |
| DeepGlow2             |  99.5% | `error` ×2, SmartRender rejected with error 516 |
| Furikake              |  99.2% | `error` ×5, Render rejected with error 516 |
| RainyDay              |  99.1% | missing `PF Effect Sequence Data Suite` v1 at load |
| AnimatedNoise         |  97.4% | `crash` ×1 in `PF_Cmd_SMART_RENDER` |
| FillColor             |  96.0% | `crash` ×1 in `PF_Cmd_SMART_RENDER` |
| AOD_MobiusTransform   |  19.6% | `crash` ×37 in `PF_Cmd_SMART_RENDER` and `PF_Cmd_RENDER` |
| AutoFill2             |       — | does not load: the binary is missing from the bundle |

The loop that closes one of these is in
[compat-workflow.md](compat-workflow.md).

## Regenerating the list

The fixtures live on each machine and are not committed, so this list is a
snapshot of one run. To take your own, run the same command with `--json`;
a plugin is compatible when its `coverage` is `1`:

```bash
cargo build -p aexlo-cli
target/debug/aexlo compat fixtures/plugins/macos --json > compat.json
```

Pass a baseline from before a change to see whether the list grew or shrank;
see [compat.md](compat.md#5-baselines).
