# `aexlo compat` — how far aexlo can drive a plugin

English | [日本語](compat.ja.md)

`aexlo compat` measures aexlo, not the plugin. It does not check that a
plugin's pixels are right; `aexlo test` does that. It checks whether aexlo can
load each plugin and run it under many conditions, and it lists what the
plugin asked for that aexlo does not do:

- suites, or versions of them, that aexlo does not serve (`missing-suite`),
- host functions that aexlo only stubs: they report success and do nothing
  (`stub`),
- calls that aexlo serves, but not with the arguments given (`declined`).

Each scenario runs in a worker process, on a fresh instance. A plugin that
crashes or hangs costs that one scenario, not the run.

```bash
aexlo compat fixtures/plugins/macos                  # every plugin in a directory
aexlo compat MyGlow.plugin Other.plugin --all        # list every scenario
aexlo compat fixtures/plugins/macos --save-baseline compat.json
aexlo compat fixtures/plugins/macos --baseline compat.json   # exit 1 if any got worse
```

- [1. Scenarios](#1-scenarios)
- [2. Statuses](#2-statuses)
- [3. Options](#3-options)
- [4. Output](#4-output)
- [5. Baselines](#5-baselines)

## 1. Scenarios

All scenarios except `size:*` render at 128×96. Unless a scenario says
otherwise, it uses a gradient input, 8 bpc, the `auto` render path, frame 0
at 24 fps and parallel iteration. Each one changes a single thing:

| Id | What changes |
| -- | ------------ |
| `load` | Nothing: loading the plugin (`GLOBAL_SETUP`, `PARAMS_SETUP`, `SEQUENCE_SETUP`) |
| `render:auto@8` | Nothing: one render with every default |
| `render:<legacy\|smart\|gpu>@<8\|16\|32>` | The render path and the depth, each pair on its own |
| `input:<solid\|transparent\|checker\|noise>` | The input layer |
| `input:text` | The effect's layer is a [text layer](inspect.md#text-layers) showing `Text layer` and `42` on two lines |
| `size:<1x1\|37x23\|1920x1080>` | The input and output size |
| `time:<90@30\|1@23.976>` | The frame and the frame rate |
| `iterate:serial` | Iterate callbacks run on one thread |
| `param:#<i> <name>=<value>` | One parameter: both ends of a slider's range, both checkbox states, each popup choice, two points, colors and angles |
| `layer:#<i> <name>=linked` | A checkerboard linked to one layer parameter |

Parameters take their values from what the plugin declares at load time, so
a plugin with many parameters gets many scenarios. Buttons, groups, paths,
custom UI and arbitrary data are not set.

### On a text layer

Some effects only work on a text layer: on any other layer they pass their
input through, so the scenarios above never reach what they do. `compat`
also renders `input:text`'s pixels on an AV layer. When the plugin renders
the two differently, it works on text layers, and every scenario except
`input:*` runs again on the text layer. Those ids start with `text:`, as in
`text:param:#18 Gradient Mode=Radial`; `--only text:` keeps only them.

## 2. Statuses

| Status | Meaning |
| ------ | ------- |
| `ok` | It ran, and asked for nothing aexlo does not do |
| `partial` | It ran, but asked for something aexlo does not do. The report lists what |
| `n/a` | The plugin does not declare it (a depth, smart render, GPU) |
| `invalid` | The scenario does not fit the plugin. This is a gap in `compat`, not in aexlo |
| `error` | The plugin returned a `PF_Err` |
| `timeout` | No result within `--timeout` |
| `crash` | The worker died. The report names the command it was in |

A scenario is only marked `partial` for gaps beyond the ones loading already
had. Those are reported once, on `load`.

## 3. Options

| Option | Default | |
| ------ | ------- | - |
| `<plugin\|dir>...` | | Plugin artifacts, or directories of them. A broken symlink in a directory is skipped |
| `--only <text>` | | Only scenarios whose id contains `<text>`. `load` always runs, and so does `input:text` to decide on the `text:` scenarios, but it is only listed if it matches |
| `--jobs <n>` | half the CPUs | Workers at once |
| `--timeout <s>` | `20` | Seconds per scenario |
| `--all` | | List every scenario on its own. Without it, only scenarios that are not `ok` or `n/a` are listed, grouped by how they went |
| `--format json` | text | The report as one JSON document |
| `--save-baseline <file>` | | Write each scenario's status |
| `--baseline <file>` | | Compare with a saved baseline |

The exit status is 0 unless `--baseline` finds a scenario that got worse
(1). An option it does not know, or a plugin it cannot find, exits 2. A
worker that cannot be started exits 3.

## 4. Output

The text report has one block per plugin, with its counts and the scenarios
that did not go well. Scenarios that failed the same way are grouped (same
status, same first line of the message, same gaps) and a few of them are
named. The report ends with every gap and the plugins that hit it, most
widespread first. That is the list of what to implement next.

```text
RainyDay  load partial · 20 ok · 93 partial
    partial  load
               missing-suite PF Effect Sequence Data Suite v1 (in PARAMS_SETUP)
    partial  ×93 scenarios
               missing-suite AEGP Stream Suite v11 (in UPDATE_PARAMS_UI)
               e.g. param:#1 Preset=Custom, param:#1 Preset=Default, … (+91)

Asked for, but not done by aexlo (plugins that asked):
   10  missing-suite AEGP Stream Suite v11  [AOD_ChannelRemap, …, RainyDay]
```

`--format json` prints:

```json
{
  "summary": { "ok": 1992, "partial": 1654, "n/a": 90, "error": 6, "crash": 38 },
  "plugins": [
    {
      "plugin": "RainyDay",
      "path": "fixtures/plugins/macos/RainyDay.plugin",
      "load": { "id": "load", "status": "partial", "unsupported": [ … ] },
      "info": { "params": [ … ], "smart": true, "gpu": false, "depths": [8, 16, 32], "formats": [] },
      "scenarios": [
        {
          "id": "param:#1 Preset=Custom",
          "status": "partial",
          "unsupported": [
            { "kind": "missing-suite", "name": "AEGP Stream Suite", "detail": "v11",
              "command": "UPDATE_PARAMS_UI", "count": 17 }
          ]
        }
      ]
    }
  ],
  "gaps": [ { "gap": "missing-suite AEGP Stream Suite v11", "plugins": ["AOD_ChannelRemap", …] } ],
  "worse": [],
  "better": []
}
```

A scenario also has `message` (why it did not pass) and, for `error`, `crash`
and `timeout`, `last_command`. `worse` and `better` are only present with
`--baseline`.

The same gaps appear per run in `aexlo test --format json` (`unsupported`).
In code, they reach `Observer::unsupported` as an `UnsupportedEvent`.

## 5. Baselines

`--save-baseline` writes each scenario's status per plugin:

```json
{ "plugins": { "RainyDay": { "load": "partial", "render:auto@8": "ok" } } }
```

`--baseline` reports every scenario that changed. A scenario is worse when it
moved down this order: `ok` → `partial` → `error` → `timeout` → `crash`.
Scenarios that are `n/a` or `invalid` on either side are not compared. Nor
are scenarios or plugins that only one side has, such as a parameter that
was renamed.

Most plugin fixtures are local to each machine (`fixtures/plugins/.gitignore`),
so keep a baseline next to them rather than in the repository. Save one
before a change to aexlo, then compare after it.

The whole loop, from picking a gap to committing the fix, is in
[compat-workflow.md](compat-workflow.md).
