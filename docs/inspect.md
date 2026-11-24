# `aexlo inspect` — driving and analyzing a plugin

English | [日本語](inspect.ja.md)

`aexlo inspect` loads a built After Effects plugin and runs commands against
it, such as rendering, setting a parameter or sending `PF_Cmd_DO_DIALOG`.
It records everything the plugin does in return:

- every `PF_Cmd_*` it is sent,
- every host suite function it calls back, with its arguments,
- every handle and world it allocates or frees.

It then tells you what the recording means: errors, leaks, null pointers,
use after free.

You can run it three ways:

| Mode | How | For |
| ---- | --- | --- |
| TUI | `aexlo inspect <plugin>` | A person exploring a plugin by hand |
| Batch | `aexlo inspect <plugin> --exec "<commands>"` or `--script <file>` | Scripts and CI: run a fixed list, print the analysis |
| Session over a pipe | `aexlo inspect <plugin> --script -` | Agents and tools: send one command, read the answer, decide the next |

Batch and pipe output is text by default. Add `--json` to get one JSON
object per line, which a program can parse ([Events](#4-events)).
`aexlo inspect --help` prints the options and commands.

- [1. Quick start](#1-quick-start)
- [2. Options](#2-options)
- [3. Commands](#3-commands)
- [4. Events](#4-events)
- [5. Shared objects](#5-shared-objects)
- [6. Exit status and errors](#6-exit-status-and-errors)
- [7. Recipes](#7-recipes)

## 1. Quick start

`<plugin>` is a plugin artifact (`.plugin`, `.aex`, `.dll`, `.dylib`) or a
crate directory. A crate directory is built first.

A name without a path separator, such as `SDK_Noise`, is also tried with the
platform's extension (`.plugin` on macOS, `.aex` on Windows) in the current
directory. A path with a separator must name the file exactly:
`fixtures/plugins/macos/SDK_Noise.plugin`, not `fixtures/plugins/macos/SDK_Noise`.

```bash
# Look around by hand. `:help` lists the commands, q quits.
aexlo inspect build/MyGlow.plugin

# Run a fixed list and print the analysis as text.
aexlo inspect MyGlow --exec "set Radius 50; render smart; focus checkout"

# Same, as JSON lines.
aexlo inspect MyGlow --json --exec "render; render smart"

# Keep the plugin loaded and feed it commands on stdin, one per line.
aexlo inspect MyGlow --json --script -
```

### `--exec`, `--script <file>` and `--script -`

All three run the [commands](#3-commands) you would type at the TUI's prompt.
They differ in where the commands come from and how mistakes are handled.
Give only one of them; giving two is an error.

| | `--exec "<cmds>"` | `--script <file>` | `--script -` |
| - | - | - | - |
| Commands come from | The argument | A file | Standard input |
| Separators | `;` (newlines too) | Newlines and `;` | Newlines and `;` |
| `#` comments | A piece that starts with `#` is skipped | A line (or piece) that starts with `#` is skipped | A line (or piece) that starts with `#` is skipped |
| When they are parsed | All of them, before the plugin loads | All of them, before the plugin loads | One line at a time, as it arrives |
| An unknown command | Stops everything before loading (exit 1) | Stops everything before loading (exit 1), even one after a `quit` | Fails that line (`ok: false`). The session goes on. |
| When it ends | After the last command, or at `quit` | After the last command, or at `quit` | At `quit`, or when stdin closes |

`;` always splits commands and cannot be escaped, so no command can take a
value or path containing `;`.

`--script -` answers each line before it reads the next one. A program can
therefore send `render`, read the result, and only then decide whether to
send `focus #29` or `set Radius 10`, all against the same loaded plugin. The
other two modes run their list straight through.

All three modes end the same way: the plugin is unloaded (`GLOBAL_SETDOWN`)
and the analysis is printed once more ([`report`](#report)).

## 2. Options

| Option | Meaning | Default |
| ------ | ------- | ------- |
| `-i`, `--input <png>` | The input layer | A gradient |
| `--text <string>` | Make the layer a text layer showing `<string>`, as [`input text`](#3-commands) does. Not with `--input`. | Not a text layer |
| `--font <file>` | The TrueType or OpenType font `--text` and `input text` set text in | A sans-serif face installed with the system (Arial on macOS and Windows) |
| `--size <W>x<H>` | Size of the default input and of the output | 640x360, or the size of `--input` |
| `--depth <8\|16\|32>` | Project depth (bits per channel) | 8 |
| `--level <args\|calls\|commands>` | What is recorded; see below | `args` |
| `--strict` | Turn on every strict check (poison, guard bands, allocation and checkout tracking) from the first command | Off |
| `--serial` | Run iterate callbacks on the plugin's thread, so their suite calls are recorded too | Off |
| `--exec <cmds>` | Batch: run the commands | — |
| `--script <file\|->` | Batch from a file, or a session over stdin with `-` | — |
| `--json` | Batch output as [JSON lines](#4-events) instead of text | text |
| `--trace` | In batch mode, also print the whole trace in the report | Off |
| `-h`, `--help` | Print the options and commands | — |

`--json` and `--trace` need `--exec` or `--script`.

`--level` decides whether suite calls are kept in the trace. Steps,
`PF_Cmd_*` begins and ends, allocations, frees and checkouts are recorded at
every level.

| Level | Suite calls |
| ----- | ----------- |
| `args` | Recorded with their arguments and result |
| `calls` | Recorded without arguments |
| `commands` | Not recorded |

## 3. Commands

These are the same commands the TUI's prompt takes. Leading `:`s are ignored,
and command names ignore case. Plugin commands run on the plugin's thread and
are recorded under a *step*, which is the command line you sent.

| Command | Aliases | What it does |
| ------- | ------- | ------------ |
| `about` | | `PF_Cmd_ABOUT`; the message is the plugin's text |
| `render [auto\|smart\|legacy\|gpu]` | `r` | Render a frame through that path. `auto` picks GPU, then smart, then legacy, falling back when one fails. |
| `set <param> <value>` | | Set a parameter; see [Setting parameters](#setting-parameters) |
| `key <param> <frame> <value>` | | Add a linear keyframe at a frame of the current rate. Quote a name with spaces in it. `set` replaces a parameter's keyframes with its value. |
| `mask <rect\|ellipse\|triangle\|ring\|none>` | | Replace the layer's masks with that figure |
| `changed <param>` | | `PF_Cmd_USER_CHANGED_PARAM` (and `UPDATE_PARAMS_UI` if the plugin asks for it). `<param>` is the rest of the line, quotes optional. |
| `press <button>` | | Press a button: `changed` for a button parameter, named by index, `#index`, name, or the text on it (ignoring case). The message lists the values the plugin changed, as in `pressed #88 "Reset Tint"; #84 = 255, 0, 0, 255, #87 = 100`. A button the plugin hid or disabled is refused; `changed` sends it anyway. In the TUI, Enter on a button in the Params tab presses it. |
| `updateui` | `update-ui` | `PF_Cmd_UPDATE_PARAMS_UI` |
| `dialog` | | `PF_Cmd_DO_DIALOG` (the Options button); see [Dialogs](#dialogs) |
| `time <frame> [fps]` | | Move to a frame (fps defaults to 30) |
| `size <w> <h>` | | Output size of later renders (1 to 16384) |
| `input <png>` | | Replace the input layer with a PNG, converted to the current depth. The rest of the line is the path; quotes are optional. |
| `input <black\|white\|gradient> [w h]` | | Replace the input layer with a generated one, at the given size or the current one |
| `input text <string>` | | Make the effect's layer a text layer showing `<string>` at the current size; `\n` breaks lines, quotes are optional. See [Text layers](#text-layers). |
| `input shape <rect\|ellipse\|triangle\|ring>` | | Make the effect's layer a shape layer showing that figure at the current size. See [Shape layers](#shape-layers). |
| `depth <8\|16\|32>` | | Convert the input (project) depth. Fails if the plugin does not declare that depth. |
| `reload` | | Tear the plugin down and load it again. A crate is rebuilt first. |
| `level <commands\|calls\|args>` | `cmd`, `call`, `arguments` | Change what is recorded |
| `strict <on\|off\|list>` | `all`, `none` | Set the strict checks. `list` is any of `poison`, `guard`, `alloc`, `checkout`, separated by commas (spaces allowed). It replaces the current set: `strict guard` turns the others off. |
| `serial <on\|off>` | | See `--serial` |
| `save <path>` | | Write the output frame as `.png` (8 bpc, or 16 bpc for deeper output) or `.exr` (32 bpc). Other extensions are refused. The path may contain spaces. Before the first render it writes whatever the output holds, which is the host's initial buffer. |
| `focus <text\|#seq>` | `f`, `diag` | Diagnose one trace entry; see [`focus`](#focus) |
| `report` | | Print the analysis now (batch only) |
| `clear` | | Forget the trace and totals; see [State across commands](#state-across-commands) |
| `help` | `?` | Print the command list |
| `quit` | `q`, `exit` | End the session. In batch mode, stop running commands, then unload the plugin and print the final report. `quit` itself prints no `command` event. |
| `mem <address\|global\|sequence>` | `memory` | TUI only: show an allocation in the Memory tab. Batch mode skips it. |
| `find <text>` | `/` | TUI only: filter the Trace and Calls tabs. Batch mode skips it. |

### Setting parameters

`<param>` names a parameter in one of these ways:

| Form | Example |
| ---- | ------- |
| An index (`#0` is the input layer and cannot be set) | `set 3 0.5`, `set #3 0.5` |
| A name, quoted (`"` or `'`) | `set "Noise variation" 25` |
| A name before `=` | `set Noise variation=25` |
| A name as plain words | `set Noise variation 25` |

A name matches exactly, or ignoring case when that picks out one parameter.
In the plain-words form, the longest run of leading words that names a
parameter is the name, and the rest is the value: `set Emitter Type 2` sets
`Emitter Type` even if a group is called `Emitter`. When a name belongs to
several parameters, the error lists their indices; use one of them.

The `=` form splits at the first `=`, so a value that contains `=` needs the
quoted form.

The value is read according to the parameter's type (the `kind` in
[`report.params`](#report)):

| `kind` | Value | Example |
| ------ | ----- | ------- |
| `FloatSlider`, `FixedSlider` | A number | `set Gain 1.5` |
| `Slider` | An integer | `set Count 10` |
| `Angle` | Degrees, optionally followed by `°` or `deg` | `set Rotation 90°` |
| `Checkbox` | `true`/`false`, `on`/`off`, `yes`/`no`, `1`/`0` | `set Enable on` |
| `Popup` | The 1-based choice number, or the choice's label (ignoring case) | `set Mode 3`, `set Mode Leak` |
| `Color` | `r,g,b` or `r,g,b,a` with 0–255 channels (alpha defaults to 255), or `#rrggbb` / `#rrggbbaa` | `set Tint 255, 128, 0`, `set Tint #ff8000` |
| `Point` | `x,y` | `set Center 320, 180` |
| `Point3D` | `x,y,z` | `set Position 1, 2, 3` |
| `Path` | A mask id (integer) | `set Mask 1` |

Colors are 8-bit (`PF_Pixel`) whatever the project depth, and are shown as
stored. If a plugin declares an alpha of `1`, its color shows as
`0, 0, 0, 1`, meaning an alpha of 1 out of 255.

`Button`, `Layer`, `GroupStart`, `GroupEnd`, `NoData`, `ArbitraryData` and
`Custom` parameters have no value to set; `set` says so and names the type.

A value outside the slider range, or a popup number outside its choices, is
still set, because a plugin may well be tested with one. The message notes
it, as in `#2 = 9  (outside the slider range 0..4)`. After Effects would not
let a user enter such a value.

### State across commands

| What | `clear` | `reload` |
| ---- | ------- | -------- |
| The trace, the command and suite-call totals, `errors` | Forgotten; entry numbers start again from `#1` | Kept; the teardown and the new setup are added to them |
| The `failed` count | Kept | Kept |
| The plugin, its memory and its parameter values | Untouched | Unloaded and loaded again. Parameter values are carried over where the parameter still exists with the same type. |
| `size`, `input` (and its text), `depth`, `level`, `strict`, `serial` | Untouched | Carried over |

The output size is the input's size unless `--size` or `size` sets it.
Replacing the input later with `input <png>` keeps the output size, so the
two can differ. The plugin then renders an output of one size from an input
of another, the way it would for an effect that grows its layer.

### Text layers

Some effects only work on a text layer: they ask `AEGP_GetLayerObjectType`
and read the glyph outlines through the AEGP Text Layer Suite. On any other
layer they usually pass their input through. `input text <string>` (or
`--text`) makes the effect's layer a text layer:

- `AEGP_GetLayerObjectType` answers `AEGP_ObjectType_TEXT`.
- `AEGP_GetNewTextOutlines` hands out one closed outline per glyph contour,
  in reading order, in layer pixels. The PF Path Data Suite reads them with
  their tangents as absolute points, as After Effects gives text outlines.
- The input layer is the text, white on transparent, as After Effects
  renders a text layer before its effects.

The text is set as a centred paragraph, centred on the layer, as large as fits
(40% of the layer's height at most). Any other `input` makes the layer an AV
layer again.

```bash
aexlo inspect TextGrad --text "TextGrad" --exec "render; save text.png"
```

### Shape layers

`input shape <figure>` puts the effect on a shape layer in a project of one
comp. The plugin finds the figure's paths in the layer's streams
(`ADBE Root Vectors Group` down to `ADBE Vector Shape`) and reads them through
the AEGP Mask Outline Suite. The input is the figure, white on transparent.
The figure sits left of and above the middle, so paths read in the wrong
space show. `mask <figure>` gives the same figure as masks instead.

```bash
aexlo inspect Extrude --exec "input shape ring; render smart; save ring.png"
aexlo inspect Extrude --exec "input shape ring; key 27 0 0; key 27 30 200; time 15; render smart; save half.png"
```

### Dialogs

`dialog` sends `PF_Cmd_DO_DIALOG`, and many plugins answer by opening a
native window. In batch mode or over a pipe there may be no event loop or
window server for that window. The plugin may then block, or crash and take
the process with it (`DeepGlow2` does). Send `dialog` from the TUI instead:
the plugin's thread keeps running there, and on Windows it dispatches the
window's messages while it waits for the next command.

## 4. Events

With `--json`, every line on stdout is one JSON object. Its `event`
field says what kind of line it is. Read stdout line by line and dispatch on
`event`. The order is always:

```text
loaded
command | focus | report | help      (one per command you send; none for quit)
report                              (at the end)
```

If the plugin cannot be found or loaded, `error` is the only line on stdout.

Durations are in microseconds (`*_us`). Addresses are hex strings (`"0x1238…"`),
the form that `focus` accepts. Entry numbers (`seq`, `step`) are numbers;
`focus #<n>` takes them.

Each fact appears once, and a field with nothing to say is left out rather
than set to `null`: an error only where there was one, `repeat` only on a
folded entry, `role` only on `global_data` or `sequence_data`. Read optional
fields with a default (`.get("error")` in Python, `.error // empty` in jq).
`null` appears only where it is the value, as for a handle the plugin left
unset.

### `loaded`

The plugin has gone through `GLOBAL_SETUP`, `PARAMS_SETUP` and
`SEQUENCE_SETUP` and is ready for commands.

| Field | Type | |
| ----- | ---- | - |
| `plugin` | string | The artifact's path |
| `params` | number | Parameter count (excluding the input layer) |

```json
{"event":"loaded","params":4,"plugin":"build/MyGlow.plugin"}
```

### `command`

One command ran. Every `command` event has `line` (the command as sent) and
`ok` (whether it succeeded). The other fields depend on what happened:

| Field | Type | Present | |
| ----- | ---- | ------- | - |
| `message` | string | `ok` | What it printed (`rendered 16x8 (Smart)`, `#1 = 7`, the ABOUT text, ...) |
| `elapsed_us` | number | `ok` | How long it took |
| `error` | string | not `ok` | Why it failed |
| `step` | number | Ran, except `clear` | The entry number of this step, such as `10`. `focus #10` shows what the step caused. `clear` is not recorded as a step and has none. |
| `commands` | array | Ran | The `PF_Cmd_*` the step sent, outermost only. Each is `{command, elapsed_us, error?}`, where `error` is an [error object](#error-object), present when the command returned one. |
| `suite_calls` | number | Ran | Suite calls made during the step |
| `allocations`, `frees` | number | Ran | Handles and worlds allocated and freed during the step |
| `skipped` | string | `mem`, `find` | Why it did nothing (`"only meaningful in the TUI"`). `ok` is `true`. |

"Ran" covers every command except a skipped one and a line that could not
be parsed. Unparsable lines only reach this point with `--script -`, and
their event has just `line`, `ok: false` and `error`.

A command can have `ok: true` and still contain a `PF_Cmd_*` with an error.
For example, `render` (auto) may fail `SMART_RENDER` and then succeed through
the legacy path. Check `commands[].error` too.

```json
{"event":"command","line":"render legacy","ok":false,
 "error":"Legacy render failed: Plugin rejected Render with error code: 512",
 "step":29,
 "commands":[{"command":"PF_Cmd_RENDER","elapsed_us":10,
              "error":{"code":512,"name":"PF_Err_INTERNAL_STRUCT_DAMAGED"}}],
 "suite_calls":0,"allocations":0,"frees":0}
```

### `focus`

The diagnosis of one trace entry.

`focus #<n>` picks entry number `n`. Anything else is text, and the newest
entry whose [`text`](#entry-object) contains it (ignoring case) is picked.
The search covers the whole line, symbols and arguments included, so be
specific:

- `focus checkout` may match a later call whose arguments contain
  `checkout_idL`, rather than the `⇄ CheckoutLayerPixels` line.
  `focus ⇄ Checkout` picks the checkout.
- `focus ◀ PF_Cmd_RENDER` picks the end of the latest legacy render, which
  holds its error. (`◀ PF_Cmd_SMART_RENDER` does not contain that text.)
- `focus + alloc Handle 0x1238…` picks the allocation of that address.
  `focus 0x1238…` picks the newest entry of any kind that mentions it.

`#` followed by something other than a number is searched as text. `focus`
with nothing after it is a parse error.

| Field | Type | |
| ----- | ---- | - |
| `line`, `query` | string | The command, and the text or `#seq` after `focus` |
| `ok` | bool | `false` when nothing matches. The event then has `error` instead of `entry` and `diagnosis`, and it counts as a failed command. |
| `entry` | object | The [entry](#entry-object) that matched. A call's `args`, `result` and `last` are left out: the diagnosis lists them, each with its verdict. |
| `diagnosis` | object | `{sections}`; see [diagnosis](#diagnosis-object) |
| `around` | array | The entries either side of it, the matched one included: `{seq, depth, text}`, with `text` cut to 96 characters |

What the diagnosis covers depends on the entry:

- **Suite call:** its arguments and result, each address judged against the
  allocation history (live, already freed, not yet allocated). Also the
  memory the call made the host allocate and whether it was freed, the
  command it ran in, and how often the function was called.
- **`PF_Cmd_*` (begin or end):** how long it took, its error with a hint,
  what ran inside it, what it allocated and still holds, and the last call
  before an error.
- **Allocation or free:** the whole lifetime of that address, whether it is
  live now, its role (`global_data`, `sequence_data`), and the calls that
  named it.
- **Step:** the commands it caused, its suite calls, allocations and errors.

```json
{"event":"focus","line":"focus host_new_handle","query":"host_new_handle","ok":true,
 "entry":{"seq":23,"at_us":70345,"depth":1,"type":"call","suite":"PF_HandleSuite1",
          "function":"host_new_handle","text":"PF_HandleSuite1::host_new_handle(size=64)"},
 "diagnosis":{"sections":[
   {"items":[{"key":"inside","value":"PF_Cmd_SMART_RENDER  (+70 µs)"},
             {"key":"after","value":"» render smart"}]},
   {"heading":"arguments","items":[{"key":"size","value":"64"}]},
   {"heading":"memory it moved","items":[
     {"text":"#24 + alloc Handle 0x123805590 (64 B)"},
     {"text":"never freed: still held","tone":"warn"}]},
   ...]},
 "around":[{"seq":20,"depth":0,"text":"▶ PF_Cmd_SMART_RENDER"}, ...,
           {"seq":23,"depth":1,"text":"PF_HandleSuite1::host_new_handle(size=64)"}, ...]}
```

### `report`

The analysis of everything recorded so far. It is printed for `report`, and
once at the end.

The final report comes after the plugin is unloaded. Its trace, totals and
`errors` include the `unload` step and the `PF_Cmd_GLOBAL_SETDOWN` it sent,
and `findings` includes what strict mode judged then. `plugin`, `state`,
`params` and `memory` describe the plugin as it was just before the unload.

| Field | Type | |
| ----- | ---- | - |
| `failed` | number | Commands that failed so far |
| `loaded` | bool | Whether a plugin is loaded. When `false` (after a failed `reload`), the fields from `plugin` down are missing. |
| `trace` | object | `{entries, dropped}`. With `--trace` it also has `lines`, an array of every [entry](#entry-object). The trace keeps the latest 200,000 entries and counts the older ones in `dropped`. |
| `commands` | array | Totals per `PF_Cmd_*`, slowest total first: `{command, count, errors, max_us, total_us}` |
| `suite_calls` | array | Totals per suite function, most called first: `{suite, function, count, commands}`. `commands` lists the `PF_Cmd_*` it was called from. |
| `errors` | array | Every `PF_Cmd_*` that returned an error: `{seq, command, error}` |
| `plugin` | object | `path`; `smart_render`, `gpu`, `custom_ui` (bools); `options_button` (its label, when the plugin set one); `pixel_formats` (see below); `out_flags`, `out_flags2` ([flags](#flags-object)) |
| `state` | object | See below |
| `params` | array | One per parameter; see below |
| `memory` | array | Live handles and worlds: `{kind, address, bytes, allocated_in, role?, head?}`. `kind` is `Handle` or `World`. `role` is `global_data` or `sequence_data`, on the allocation that is one. `head` is the first 64 bytes as hex, or all of them for a smaller allocation, and missing when they could not be read. |
| `findings` | array of string | Strict-mode findings, oldest first; see [below](#findings-and-when-leaks-show-up) |

Each entry of `params` is `{index, name, kind, value, detail, hidden, disabled}`:

- `kind` is the type, as in [Setting parameters](#setting-parameters).
- `value` is written the way `set` reads it, except that an angle carries `°`
  and a path reads `mask <id>`. It is empty for a parameter without a value.
- `detail` holds the slider range (`0..100`), the popup choices
  (`1=OK 2=Crash ...`), the text on a button (`button "Reset"`), the UI flags
  and `custom UI`, separated by two spaces.

`plugin.pixel_formats` lists the formats the plugin declared, such as
`ARGB32`, `ARGB64`, `ARGB128`, `BGRA32`, `VUYA32` and `GPU_BGRA128`. A FourCC
that aexlo does not name appears as hex (`0x61726762`).

`state` holds:

| Field | |
| ----- | - |
| `global_data`, `sequence_data` | The handles the plugin returned (address or `null`) |
| `time` | `{value, step, scale}` of the next command |
| `input`, `output` | `{width, height, bpc}` |
| `last_out_flags`, `last_out_flags2`, `last_message` | What the last command left in `PF_OutData` |
| `output_mean` | The mean of each straight (unpremultiplied) RGBA channel over the output, 0..1, to 4 decimals. Non-finite values, such as the NaN that `strict poison` writes at 32 bpc, count as 0. |
| `unwritten_pixels` | Under `strict poison` only: output pixels the last render left unwritten |
| `parallel_iterate` | Whether iterate callbacks run on several threads (`serial off`) |
| `strict` | `{poison, guard, alloc, checkout}` |

#### Findings, and when leaks show up

Strict mode judges at fixed points, so a finding appears in `findings` only
once its point has passed:

| Finding | Needs | Judged | Text |
| ------- | ----- | ------ | ---- |
| Overwritten guard band (an out-of-bounds write) | `guard` | After each render | `guard band <band> of <world> overwritten (<n> B)` |
| World leaked from a render | `alloc` | When the render command that allocated it ends | `leak: World 0x… (<n> B) from <command> still held after <command>` |
| Handle leak | `alloc` | At `GLOBAL_SETDOWN`: in the final report, or after a `reload` | `leak: Handle 0x… (<n> B) from <command> still held after GLOBAL_SETDOWN` |
| Checkout violation | `checkout` | At the bad checkout or checkin, or when a command ends with a layer parameter still checked out | `checkout: …` |

Before then, `memory` already shows what is held. An entry without a `role`
whose `allocated_in` is a render command has outlived that render.
The final report under `strict alloc` tells you whether it was ever freed.

### `help`

`{"event":"help","text":"..."}`: the command list as text.

### `error`

`{"event":"error","error":"no plugin at nope.dylib"}`: the plugin could not
be found, built or loaded. Nothing else is printed on stdout, and the exit
status is 1. stderr also gets `error: ...` with the same message.

## 5. Shared objects

### Error object

`{"code": 512, "name": "PF_Err_INTERNAL_STRUCT_DAMAGED"}`. `name` is
`PF_Err <code>` for a code the SDK does not name (a plugin-defined error).

### Entry object

One line of the trace. Every entry has:

| Field | |
| ----- | - |
| `seq` | Its number; `focus #seq` names it |
| `at_us` | When it happened, from the start of the recording |
| `depth` | Nesting. 0 is a step or a top-level command, 1 is what that command calls, and so on. |
| `repeat` | How many calls were folded into this entry (see below). Only on a folded entry; missing means 1. |
| `text` | The line as the TUI and text output show it; `focus` searches it |
| `type` | One of the types below, which add fields |

| `type` | `text` looks like | Extra fields |
| ------ | ----------------- | ------------ |
| `step` | `» render smart` | `line`: the command you sent |
| `begin` | `▶ PF_Cmd_SMART_RENDER` | `command` |
| `end` | `◀ PF_Cmd_RENDER  PF_Err_INTERNAL_STRUCT_DAMAGED (512)  (10 µs)` | `command`, `elapsed_us`, `error` (error object, when it returned one) |
| `call` | `PF_HandleSuite1::host_new_handle(size=64)` | `suite`, `function`, `args` (`[{name, value}]`), `result` (string, when the function has one), `last` (see below) |
| `alloc`, `free` | `+ alloc Handle 0x… (64 B)`, `- free  Handle 0x… (64 B)` | `kind` (`Handle` or `World`), `address`, `bytes` |
| `checkout` | `⇄ CheckoutLayerPixels #0` | `kind`, `id` (the checkout index) |

A checkout's `kind` is one of these:

- `DeclareLayer`: `checkout_layer` during `SMART_PRE_RENDER`
- `CheckoutLayerPixels`, `CheckinLayerPixels`: during `SMART_RENDER`
- `CheckoutParam`, `CheckinParam`: `PF_CHECKOUT_PARAM`, `PF_CHECKIN_PARAM`

**Folding.** Consecutive calls to the same suite function at the same depth
are folded into one entry, whatever their arguments, so per-pixel callbacks
cannot flood the trace. `repeat` counts the calls, and `text` ends with
`  ×<repeat>`. `args` and `result` are the first call's. `last` is the last
call's `{args, result?}`, present only when it differs from the first.
The calls in between are counted, but their arguments are not kept.

Arguments are recorded only at `--level args` (the default). At `calls`,
`args` is empty; at `commands` there are no `call` entries.

### Diagnosis object

`{"sections": [{"heading": "...", "items": [...]}]}`. The entry's own name
and place in the trace are not repeated here: `entry` and `around` beside it
have them.

When the diagnosis starts with where the entry ran (`inside`, `after`), that
first section has no `heading`. A step's diagnosis has no such section and
starts with `what it caused`.

Each item takes one of two shapes:

- `{"key", "value", "tone"?, "note"?}`: a labelled value. `note`, when
  present, gives the reason for the verdict, such as
  `"live Handle, 64 B, allocated at #24 in PF_Cmd_RENDER"` or
  `"null pointer"`. A call's result has the key `result`.
- `{"text", "tone"?}`: a free line.

`tone` is there only where the analysis judges something:

| `tone` | Meaning |
| ------ | ------- |
| `bad` | A bug: use after free, a world leaked from a render, an error code |
| `warn` | Suspicious: a null pointer, memory never freed, a value not allocated yet |
| `good` | Checked and fine: an argument naming live memory, `PF_Err_NONE`, memory that was freed |

A program that only wants the problems can collect the items whose `tone` is
`bad` or `warn`.

### Flags object

`{"bits": 33555456, "names": ["PIX_INDEPENDENT", "DEEP_COLOR_AWARE"]}`.
Bits this version of aexlo does not name appear as a hex string in `names`.

## 6. Exit status and errors

| Exit | When |
| ---- | ---- |
| 0 | Every command succeeded |
| 1 | A command failed (including a `focus` with no match), the plugin could not be loaded, or a flag or command was invalid |
| Killed by a signal | The plugin crashed (see below) |

In JSON mode, stdout holds only JSON lines. Everything else goes to stderr:

- What the plugin prints with `printf` or `std::cout`. On Unix, descriptor 1
  points at stderr while the session runs.
- Log output, and cargo's build output when `<plugin>` is a crate.
- `error: ...` for a failure, and the final `error: N command(s) failed`.

Some mistakes are found before any JSON is printed: a bad flag, or an unknown
command in `--exec` or a `--script` file. These print only `error: ...` on
stderr. If stdout is empty and the exit status is 1, read stderr.

On Windows, the CLI ends as soon as it has torn the plugin down and flushed
its output. A DLL that is still loaded then (a plugin's own threads can keep
it so) does not get its process-exit detach, which would hang or crash once
those threads are gone. A crash during a command, a setdown or the unload is
still a crash. A program that embeds the `aexlo` library decides for itself
how to exit, and meets that detach if it exits normally.

A plugin that crashes takes the process down with it, and the exit status
is the signal's. The events already printed stay valid. The command that was
running never gets its `command` line, and there is no final `report`. (The
TUI restores the terminal and names the `PF_Cmd_*` and the suite call that
crashed. Batch mode does not.)

## 7. Recipes

### Shell and jq

```bash
# The commands that failed
aexlo inspect MyGlow --json --exec "render; render smart; render legacy" \
  | jq -c 'select(.event=="command" and .ok==false) | {line, error}'

# Every PF_Cmd_* that returned an error, even inside a command that succeeded
aexlo inspect MyGlow --json --exec "render" \
  | jq -c 'select(.event=="report") | .errors[]'

# Leaks: what strict mode found, including what was still held at GLOBAL_SETDOWN
aexlo inspect MyGlow --strict --json --exec "render; render" \
  | jq 'select(.event=="report") | .findings'

# Only the problems the diagnosis found
aexlo inspect MyGlow --json --exec "render legacy; focus ◀ PF_Cmd_RENDER" \
  | jq -c 'select(.event=="focus") | .diagnosis.sections[].items[] | select(.tone=="bad" or .tone=="warn")'
```

### Driving a session from a program

```python
import json, subprocess

p = subprocess.Popen(
    ["aexlo", "inspect", "build/MyGlow.plugin", "--json", "--script", "-"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

def send(line):
    p.stdin.write(line + "\n")
    p.stdin.flush()
    return json.loads(p.stdout.readline())

hello = json.loads(p.stdout.readline())
if hello["event"] == "error":
    raise SystemExit(hello["error"])

r = send("render")
failed = [c for c in r.get("commands", []) if "error" in c]
if not r["ok"] or failed:
    # Diagnose the step: which PF_Cmd_* it caused and where it failed.
    print(send(f"focus #{r['step']}")["diagnosis"])
    # Diagnose the failing command itself.
    print(send("focus ◀ PF_Cmd_RENDER")["diagnosis"])

held = send("report")["memory"]
final = send("quit")              # answered by the final report
print(final["findings"])          # includes leaks judged at GLOBAL_SETDOWN
p.wait()
```

Each line you send gets exactly one line back. `quit` is answered by the
final report, and a line holding several `;`-separated commands gets one
line per command.

### A loop for finding a bug

1. Send `render` (or the command that misbehaves) and read its `command`
   event.
2. If `ok` is `false`, or any `commands[].error` is set, send `focus` with
   `#` and its `step` (`focus #29`), then `focus ◀ PF_Cmd_<NAME>` for the failing command.
3. Collect the `bad` and `warn` items from each diagnosis. When an item names
   an address, `focus + alloc Handle 0x…` (or `World`) diagnoses that
   allocation: its lifetime and the calls that named it.
4. Change one thing (`set`, `size`, `depth`, `render smart`, `strict on`) and
   go back to step 1. `clear` first if you want the next report to cover only
   what follows.
5. For leaks, turn on `strict alloc` (or start with `--strict`) and read
   `findings` in the final report.
