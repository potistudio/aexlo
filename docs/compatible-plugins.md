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
| AnimatedNoise              |  42 |  39 |  3 | 8, 16, 32 | yes | no |
| BitonicPixelSorter         |  71 |  71 |  0 | 8, 16, 32 | yes | yes |
| Circle-Repeater            |  56 |  53 |  3 | 8, 16, 32 | yes | no |
| Cosmic                     | 111 | 111 |  0 | 8, 16, 32 | yes | yes |
| DeepGlow2                  | 442 | 432 | 10 | 8, 32 | yes | no |
| DisplacerPro               | 106 | 101 |  5 | 8, 32 | yes | no |
| Falloff                    | 148 | 148 |  0 | 8, 16, 32 | yes | yes |
| FillColor                  |  28 |  25 |  3 | 8, 16, 32 | yes | no |
| Furikake                   | 610 | 604 |  6 | 8, 16, 32 | yes | no |
| RXXR                       | 767 | 764 |  3 | 8, 16, 32 | yes | no |
| RainyDay                   | 115 | 115 |  0 | 8, 16, 32 | yes | yes |
| SDK_Noise                  |  23 |  20 |  3 | 8, 16, 32 | yes | no |
| TextGrad                   | 535 | 525 | 10 | 8, 32 | yes | no |

That is 5,246 scenarios: 5,143 `ok` and 103 `n/a`.

## Not on the list yet

The other two plugins in the fixture directory fall short in one of these
ways. Each is a gap in aexlo, or a fixture that is not installed, not a
plugin that is known to be wrong.

| Plugin | Coverage | What `compat` reports |
| ------ | -------: | --------------------- |
| AOD_MobiusTransform   |  19.6% | `crash` ×37 in `PF_Cmd_SMART_RENDER` and `PF_Cmd_RENDER` |
| AutoFill2             |       — | does not load: the binary is missing from the bundle (the Windows build is compatible; see below) |

The loop that closes one of these is in
[compat-workflow.md](compat-workflow.md).

## On Windows

Measured on Windows 10 x64 with aexlo v0.3.0, on 2026-11-24, over
`fixtures/plugins/windows`:

| Plugin | Coverage | Scenarios | `ok` | `n/a` | `crash` | Depths | Smart render | GPU |
| ------ | -------: | --------: | ---: | ----: | ------: | ------ | ------------ | --- |
| AutoFill2   | 100%  | 474 | 469 | 5 | 0 | 8, 32 | yes | no |
| HEISEI_DEMO | 98.1% |  60 |  52 | 7 | 1 | 8, 16 | no  | no |

AutoFill2 is compatible on Windows. In `PF_Cmd_SMART_PRE_RENDER` it acquires
"AEGP Render Suite" at wire version 5 (`AEGP_RenderSuite4`) and calls
`AEGP_GetCurrentTimestamp` through it. aexlo hands it a project timestamp,
which goes up whenever something that affects rendering changes. The fixture
is unlicensed, so its output carries a red cross.

HEISEI_DEMO is Windows-only, so it is not in the macOS fixtures above. Its
coverage is 98.1%, so it is not on the list either: one scenario crashes in
the plugin's own code.

HEISEI_DEMO asks for no suites. Every scenario that applies goes `ok` except
`size:1x1`, which ends in a `crash`. That crash is a bug in the plugin, not a
gap in aexlo: the plugin's own code reads through a null pointer, whichever
host runs it. The plugin shrinks the layer into a buffer of
`(width / Factor) x (height / Factor)` pixels, with `Factor` (4 by default)
truncated to an integer. When the layer is narrower or shorter than `Factor`,
that buffer is empty, and the plugin reads its null data. With `Factor` set
to 1, a 1x1 layer renders. `tests/e2e/tests/heisei_demo.rs` checks that a
layer renders when `Factor` fits in it; the crash itself was measured with
`compat`.

Only its 8 bpc output is right. The plugin declares 16 bpc
(`PF_OutFlag_DEEP_COLOR_AWARE`), but reads and writes a 16 bpc world as if
its pixels were 8 bpc: the left half of each row comes out garbled and the
right half is left untouched. `compat` counts that scenario `ok`, because it
only checks that the plugin runs.

## Regenerating the list

The fixtures live on each machine and are not committed, so this list is a
snapshot of one run. To take your own, run the same command with `--json`;
a plugin is compatible when its `coverage` is `1`:

```bash
cargo build -p aexlo-cli
target/debug/aexlo compat fixtures/plugins/macos --json > compat.json
```

On Windows, run it over `fixtures/plugins/windows` instead.

Pass a baseline from before a change to see whether the list grew or shrank;
see [compat.md](compat.md#5-baselines).
