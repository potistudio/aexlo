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

Batch and pipe output is text by default. Add `--format json` to get one JSON
object per line, which a program can parse ([Events](#4-events)).

- [1. Quick start](#1-quick-start)
- [2. Options](#2-options)
- [3. Commands](#3-commands)
- [4. Events](#4-events)
- [5. Shared objects](#5-shared-objects)
- [6. Exit status and errors](#6-exit-status-and-errors)
- [7. Recipes](#7-recipes)

## 1. Quick start

`<plugin>` is a plugin artifact (`.plugin`, `.aex`, `.dll`, `.dylib`) or a
crate directory. A crate directory is built first. A bare name such as
`SDK_Noise` is also tried with the platform's extension.

```bash
# Look around by hand. `:help` lists the commands, q quits.
aexlo inspect build/MyGlow.plugin

# Run a fixed list and print the analysis as text.
aexlo inspect MyGlow --exec "set Radius 50; render smart; focus checkout"

# Same, as JSON lines.
aexlo inspect MyGlow --format json --exec "render; render smart"

# Keep the plugin loaded and feed it commands on stdin, one per line.
aexlo inspect MyGlow --format json --script -
```

### `--exec`, `--script <file>` and `--script -`

All three run the [commands](#3-commands) you would type at the TUI's prompt.
They differ in where the commands come from and how mistakes are handled.

| | `--exec "<cmds>"` | `--script <file>` | `--script -` |
| - | - | - | - |
| Commands come from | The argument, separated by `;` | A file, one per line (`;` also works) | Standard input, one per line (`;` also works) |
| `#` comment lines | — | Skipped | Skipped |
| When they are parsed | All of them, before the plugin loads | All of them, before the plugin loads | One line at a time, as it arrives |
| An unknown command | Stops everything before loading (exit 1) | Stops everything before loading (exit 1) | Fails that line (`ok: false`). The session goes on. |
| When it ends | After the last command | After the last command | On `quit`, or when stdin closes |

`--script -` answers each line before it reads the next one. A program can
therefore send `render`, read the result, and only then decide whether to
send `focus #29` or `set Radius 10`, all against the same loaded plugin. The
other two modes run their list straight through.

All three modes print the analysis (`report`) once more at the end.

## 2. Options

| Option | Meaning | Default |
| ------ | ------- | ------- |
| `-i`, `--input <png>` | The input layer | A gradient |
| `--size <W>x<H>` | Size of the default input and of the output | 640x360 |
| `--depth <8\|16\|32>` | Project depth (bits per channel) | 8 |
| `--level <args\|calls\|commands>` | What is recorded. `args` records every suite call with its arguments. `calls` records the calls without arguments. `commands` records only `PF_Cmd_*`. | `args` |
| `--strict` | Turn on every strict check (poison, guard bands, allocation and checkout tracking) from the first command | Off |
| `--serial` | Run iterate callbacks on the plugin's thread, so their suite calls are recorded too | Off |
| `--exec <cmds>` | Batch: run the `;`-separated commands | — |
| `--script <file\|->` | Batch from a file, or a session over stdin with `-` | — |
| `--format <human\|json>` | Batch output: text, or [JSON lines](#4-events) | `human` |
| `--trace` | In batch mode, also print the whole trace in the report | Off |

`--format` and `--trace` need `--exec` or `--script`.

## 3. Commands

These are the same commands the TUI's prompt takes (a leading `:` is allowed).
Plugin commands run on the plugin's thread and are recorded under a *step*,
which is the command line you sent.

| Command | What it does |
| ------- | ------------ |
| `about` | `PF_Cmd_ABOUT`; the message is the plugin's text |
| `render [auto\|smart\|legacy\|gpu]` | Render a frame through that path (`auto` picks GPU, then smart, then legacy) |
| `set <param> <value>` | Set a parameter. `<param>` is an index, `#index` or a name. The value may contain spaces (`set Center 10, 20`). |
| `changed <param>` | `PF_Cmd_USER_CHANGED_PARAM` (and `UPDATE_PARAMS_UI` if the plugin asks for it) |
| `updateui` | `PF_Cmd_UPDATE_PARAMS_UI` |
| `dialog` | `PF_Cmd_DO_DIALOG` (the Options button) |
| `time <frame> [fps]` | Move to a frame (fps defaults to 30) |
| `size <w> <h>` | Output size of later renders |
| `input <png\|black\|white\|gradient> [w h]` | Replace the input layer, keeping its depth |
| `depth <8\|16\|32>` | Convert the input (project) depth |
| `reload` | Tear the plugin down and load it again. A crate is rebuilt first. |
| `level <commands\|calls\|args>` | Change what is recorded |
| `strict <on\|off\|poison,guard,alloc,checkout>` | Change the strict checks. Their findings appear in the report. |
| `serial <on\|off>` | See `--serial` |
| `save <path>` | Write the output frame (`.png`, `.exr`) |
| `focus <text\|#seq>` | Diagnose one trace entry: the newest entry whose text contains `<text>` (case-insensitive), or entry number `#seq` |
| `report` | Print the analysis now (batch only) |
| `clear` | Forget the trace and totals. Entry numbers start again from 1. |
| `help` | Print this list |
| `quit` | End the session (batch: stop reading commands) |
| `mem ...`, `find ...` | Only change what the TUI shows. Batch mode skips them. |

## 4. Events

With `--format json`, every line on stdout is one JSON object. Its `event`
field says what kind of line it is. Read stdout line by line and dispatch on
`event`. The order is always:

```text
loaded
command | focus | report | help      (one per command you send)
report                              (at the end)
```

If the plugin cannot be found or loaded, `error` is the only line.

Durations are in microseconds (`*_us`). Addresses are hex strings (`"0x1238…"`),
which is the form `focus` and `mem` accept.

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

One command ran.

| Field | Type | |
| ----- | ---- | - |
| `line` | string | The command as sent |
| `ok` | bool | Whether it succeeded |
| `message` | string | When `ok`: what it printed (`rendered 16x8 (Smart)`, `#1 = 7`, the ABOUT text, ...) |
| `error` | string | When not `ok`: why |
| `elapsed_us` | number | When `ok`: how long it took |
| `step` | string \| null | The entry number of this step, such as `"#10"`. Pass it to `focus` to see what the step caused. |
| `commands` | array | The `PF_Cmd_*` the step sent, outermost only. Each is `{command, elapsed_us, error}`, where `error` is an [error object](#error-object) or `null`. |
| `suite_calls` | number | Suite calls made during the step |
| `allocations`, `frees` | number | Handles and worlds allocated and freed during the step |
| `skipped` | string | Present instead of the fields above for `mem` and `find` |

A command that could not be parsed (`--script -` only) has just `line`, `ok:
false` and `error`.

A command can have `ok: true` and still contain a `PF_Cmd_*` with an error.
For example, a plugin may fail `SMART_PRE_RENDER` and then fall back to
another path. Check `commands[].error` too.

```json
{"event":"command","line":"render legacy","ok":false,
 "error":"Legacy render failed: Plugin rejected Render with error code: 512",
 "step":"#29",
 "commands":[{"command":"PF_Cmd_RENDER","elapsed_us":10,
              "error":{"code":512,"name":"PF_Err_INTERNAL_STRUCT_DAMAGED"}}],
 "suite_calls":0,"allocations":0,"frees":0}
```

### `focus`

The diagnosis of one trace entry.

| Field | Type | |
| ----- | ---- | - |
| `line`, `query` | string | The command, and the text or `#seq` after `focus` |
| `ok` | bool | `false` when nothing in the trace matches; `error` then says so. This counts as a failed command. |
| `entry` | object | The [entry](#entry-object) that matched |
| `diagnosis` | object | `{title, sections}`; see [diagnosis](#diagnosis-object) |

What the diagnosis covers depends on the entry:

- **Suite call:** its arguments and result, each address judged against the
  allocation history (live, already freed, not yet allocated). Also the
  memory the call made the host allocate and whether it was freed, the
  command it ran in, and how often the function was called.
- **`PF_Cmd_*` (begin or end):** how long it took, its error with a hint,
  what ran inside it, what it allocated and still holds, and the last call
  before an error.
- **Allocation or free:** the whole lifetime of that address, whether it is
  live now, and the calls that named it.
- **Step:** the commands it caused, its suite calls, allocations and errors.

Every diagnosis ends with an `around it` section showing a few neighbouring
entries.

```json
{"event":"focus","line":"focus host_new_handle","query":"host_new_handle","ok":true,
 "entry":{"seq":23,"type":"call","suite":"PF_HandleSuite1","function":"host_new_handle",
          "args":[{"name":"size","value":"64"}],"result":null, ...},
 "diagnosis":{"title":"PF_HandleSuite1::host_new_handle","sections":[
   {"heading":null,"items":[{"key":"inside","value":"PF_Cmd_SMART_RENDER  (+70 µs)","tone":"plain"}, ...]},
   {"heading":"arguments","items":[{"key":"size","value":"64","tone":"plain"}]},
   {"heading":"memory it moved","items":[
     {"text":"#24 + alloc Handle 0x123805590 (64 B)","tone":"plain"},
     {"text":"never freed: still held (Memory tab shows its bytes)","tone":"warn"}]},
   ...]}}
```

### `report`

The analysis of everything recorded so far. It is printed for `report` and
once at the end.

| Field | Type | |
| ----- | ---- | - |
| `failed` | number | Commands that failed so far |
| `loaded` | bool | Whether a plugin is loaded. When `false` (a failed `reload`), the fields from `plugin` down are missing. |
| `trace` | object | `{entries, dropped}`. With `--trace` it also has `lines`, an array of every [entry](#entry-object). The trace keeps the latest 200,000 entries and counts the rest in `dropped`. |
| `commands` | array | Totals per `PF_Cmd_*`, slowest total first: `{command, count, errors, max_us, total_us}` |
| `suite_calls` | array | Totals per suite function, most called first: `{suite, function, count, commands}`. `commands` lists the `PF_Cmd_*` it was called from. |
| `errors` | array | Every `PF_Cmd_*` that returned an error: `{seq, command, error}` |
| `plugin` | object | `path`, `smart_render`, `gpu`, `custom_ui`, `options_button`, `pixel_formats`, `out_flags`, `out_flags2` ([flags](#flags-object)) |
| `state` | object | `global_data`, `sequence_data` (address or `null`); `time` `{value, step, scale}`; `input` and `output` `{width, height, bpc}`; `last_out_flags`, `last_out_flags2`, `last_message` (what the last command left in `PF_OutData`); `output_mean` (RGBA, 0..1); `unwritten_pixels` (under `strict poison`, else `null`); `parallel_iterate`; `strict` `{poison, guard, alloc, checkout}` |
| `params` | array | `{index, name, kind, value, detail, hidden, disabled}`. `detail` is the range or the popup choices. |
| `memory` | array | Live handles and worlds: `{kind, address, bytes, allocated_in, role, head}`. `role` is `global_data`, `sequence_data` or `null`. `head` is the first 64 bytes as hex. |
| `findings` | array of string | Strict-mode findings, oldest first: overwritten guard bands (out-of-bounds writes), leaked handles and worlds, checkout violations |

Use `memory` to find leaks: an entry whose `role` is `null` and whose
`allocated_in` is a render command has outlived that render.

### `help`

`{"event":"help","text":"..."}`: the command list as text.

### `error`

`{"event":"error","error":"no plugin at nope.dylib"}`: the plugin could not
be found, built or loaded. Nothing else is printed, and the exit status is 1.

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
| `repeat` | Consecutive identical calls folded into this entry |
| `text` | The line as the TUI and text output show it |
| `type` | One of the types below, with extra fields |

| `type` | Extra fields |
| ------ | ------------ |
| `step` | `line`: the command you sent |
| `begin` | `command` |
| `end` | `command`, `elapsed_us`, `error` (error object or `null`) |
| `call` | `suite`, `function`, `args` (`[{name, value}]`), `result` (string or `null`), `last` (`{args, result}` of the last folded repeat when it differs, else `null`) |
| `alloc`, `free` | `kind` (`Handle`, `World`, ...), `address`, `bytes` |
| `checkout` | `kind`, `id` |

Arguments are recorded only at `--level args` (the default). At the lower
levels, `args` is empty.

### Diagnosis object

`{"title": "...", "sections": [{"heading": "...", "items": [...]}]}`. The
first section's `heading` is `null` when it holds where the entry ran
(`entry`, `inside`, `after`). Each item takes one of two shapes:

- `{"key", "value", "tone", "note"?}`: a labelled value. `note` explains what
  the analysis made of it, such as `"live Handle, 64 B, allocated at #24 in
  PF_Cmd_RENDER"` or `"null pointer"`.
- `{"text", "tone"}`: a free line.

`tone` carries the verdict:

| `tone` | Meaning |
| ------ | ------- |
| `bad` | A bug: use after free, a world leaked from a render, an error code |
| `warn` | Suspicious: a null pointer, memory still held, a value not allocated yet |
| `good` | Checked and fine: a live allocation, `PF_Err_NONE` |
| `plain` | Information |
| `dim` | Context, such as neighbouring entries |

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

In JSON mode, stdout holds only JSON lines. Everything else goes to stderr:

- What the plugin prints with `printf` or `std::cout`. On Unix, descriptor 1
  points at stderr while the session runs.
- Log output, and cargo's build output when `<plugin>` is a crate.
- The final `error: N command(s) failed`.

Some mistakes are found before any JSON is printed: a bad flag, or an unknown
command in `--exec` or a `--script` file. These print only `error: ...` on
stderr. If stdout is empty and the exit status is 1, read stderr.

A plugin that crashes takes the process down with it, and the exit status
is the signal's. The events already printed stay valid. The command that was
running never gets its `command` line, and there is no final `report`. (The
TUI restores the terminal and names the `PF_Cmd_*` and the suite call that
crashed. Batch mode does not.)

## 7. Recipes

### Shell and jq

```bash
# The commands that failed
aexlo inspect MyGlow --format json --exec "render; render smart; render legacy" \
  | jq -c 'select(.event=="command" and .ok==false) | {line, error}'

# Every PF_Cmd_* that returned an error, even inside a command that succeeded
aexlo inspect MyGlow --format json --exec "render" \
  | jq -c 'select(.event=="report") | .errors[]'

# What the plugin still holds after two renders
aexlo inspect MyGlow --format json --exec "render; render" \
  | jq 'select(.event=="report") | .memory'

# Only the problems the diagnosis found
aexlo inspect MyGlow --format json --exec "render legacy; focus ◀ PF_Cmd_RENDER" \
  | jq -c 'select(.event=="focus") | .diagnosis.sections[].items[] | select(.tone=="bad" or .tone=="warn")'
```

### Driving a session from a program

```python
import json, subprocess

p = subprocess.Popen(
    ["aexlo", "inspect", "build/MyGlow.plugin", "--format", "json", "--script", "-"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

def send(line):
    p.stdin.write(line + "\n")
    p.stdin.flush()
    return json.loads(p.stdout.readline())

hello = json.loads(p.stdout.readline())
if hello["event"] == "error":
    raise SystemExit(hello["error"])

r = send("render")
failed = [c for c in r.get("commands", []) if c["error"]]
if not r["ok"] or failed:
    # Diagnose the step: which PF_Cmd_* it caused and where it failed.
    print(send(f"focus {r['step']}")["diagnosis"])
    # Diagnose the failing command itself.
    print(send("focus ◀ PF_Cmd_RENDER")["diagnosis"])

leaks = send("report")["memory"]
send("quit")                      # answered by the final report
p.wait()
```

Each command produces exactly one line, so `send` can read one line per line
it writes. The exception is a line holding several `;`-separated commands,
which produces one line per command.

### A loop for finding a bug

1. Send `render` (or the command that misbehaves) and read its `command`
   event.
2. If `ok` is `false`, or any `commands[].error` is set, send `focus` with
   its `step`, then `focus ◀ PF_Cmd_<NAME>` for the failing command.
3. Collect the `bad` and `warn` items from each diagnosis. When an item names
   an address, `focus alloc Handle 0x…` (or `alloc World 0x…`) diagnoses the
   allocation itself, including its lifetime and the calls that named it. A bare
   `focus 0x…` picks the newest entry of any kind that mentions the address.
4. Change one thing (`set`, `size`, `depth`, `render smart`, `strict on`) and
   go back to step 1. `clear` first if you want the next report to cover only
   what follows.
