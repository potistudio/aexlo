# Closing compatibility gaps — the workflow

English | [日本語](compat-workflow.ja.md)

This is the loop for making aexlo drive more plugins. Each turn of it closes
one gap that [`aexlo compat`](compat.md) reports, checks that nothing got
worse, and commits.

```text
[0] save a baseline
 └▶[1] pick a gap ─▶[2] reproduce, read ─▶[3] implement ─▶[4] test ─▶[5] compare ─▶[6] commit
        ▲                                                                              │
        └──────────────────────── save a new baseline, pick the next gap ◀────────────┘
```

- [0. Save a baseline](#0-save-a-baseline)
- [1. Pick a gap](#1-pick-a-gap)
- [2. Reproduce it and read what the plugin does](#2-reproduce-it-and-read-what-the-plugin-does)
- [3. Implement](#3-implement)
- [4. Test](#4-test)
- [5. Compare with the baseline](#5-compare-with-the-baseline)
- [6. Commit](#6-commit)
- [Running the loop](#running-the-loop)

## 0. Save a baseline

Before touching aexlo, build the CLI and record where every plugin stands.

```bash
cargo fmt --all -- --check
cargo build -p aexlo-cli
mkdir -p target/compat
aexlo compat fixtures/plugins/macos --format json \
  --save-baseline target/compat/base.json > target/compat/before.json
```

The fixtures are local to each machine, so the baseline is too: keep it out of
the repository. `target/` is ignored by git; `cargo clean` deletes it, so save
a new one if it is gone.

## 1. Pick a gap

`gaps` in the JSON report (and the end of the text report) lists every gap,
most widespread first. That list is the to-do list. Within it, take them in
this order:

| Order | What | Why |
| ----- | ---- | --- |
| 1 | `crash`, `timeout` | Something is broken, and the scenarios behind it cannot be measured |
| 2 | A `missing-suite` on `load` | It makes every scenario of that plugin `partial` |
| 3 | A `missing-suite` many plugins hit | One implementation moves the most scenarios |
| 4 | `stub` | The call reports success and does nothing, so pixels are likely wrong |
| 5 | `declined` | Arguments aexlo serves the call for, but not these. Usually small |
| — | `invalid` | A gap in `compat`, not in aexlo. Fix `compat`, in its own commit |

Take **one gap per turn**. Two at once make the baseline diff hard to read.

## 2. Reproduce it and read what the plugin does

Narrow `compat` to the plugin and scenarios that hit the gap:

```bash
aexlo compat fixtures/plugins/macos/RainyDay.plugin --only "param:#1" --all
```

Then drive the plugin with [`aexlo inspect`](inspect.md) to see which calls
it makes, with which arguments, and what it does with the result:

```bash
aexlo inspect fixtures/plugins/macos/RainyDay.plugin --format json --script -
# render / set / changed / updateui to trigger it, focus #<seq> to dig in.
# For a crash, add --strict --serial and look around last_command.
```

What to read:

- `refs/After Effects SDK - 25.2/`: the SDK headers and the SDK Guide PDF.
  They define what the call must do.
- `refs/after-effects/after-effects-sys`: the Rust types and struct layouts.
- The plugin's source, when it is in `refs/` (`aod-AE-plugin`,
  `F-s-PluginsProjects_forMac`, `BitonicPixelSorterForAE`). It shows how the
  plugin uses the result, and therefore how much of the call it needs.
- Without source, disassemble the plugin binary (`lipo`, `llvm-objdump`).

## 3. Implement

| Gap | Where |
| --- | ----- |
| `missing-suite` | A new `crates/aexlo/src/suites/<name>.rs`, declared in `suites/mod.rs`, a field on `SuiteContainer` filled in `SUITE_CONTAINER`, and an arm in `rusty_acquire_suite` |
| `stub` | Replace the `stub_log!` function with the real behavior |
| `declined` | Extend the function to handle the arguments |

- Suites are stateless vtables shared by every instance; keep any state
  behind the pointers the callbacks receive. Follow the safety premises at the
  top of `suites/mod.rs`.
- **Do not report success for what is still not done.** A branch aexlo cannot
  handle yet keeps calling `observe::unsupported(UnsupportedKind::…, …)`.
  Otherwise `compat` counts it as `ok` and its numbers stop meaning anything.

## 4. Test

- Unit tests next to the suite.
- Behavior with a real plugin: `tests/e2e/tests/<plugin>.rs`. Load the
  fixture with `test_e2e::fixture("<stem>")` and skip when it returns `None`,
  since most fixtures are local.
- A plugin that misuses the call: `tests/misbehave`.
- Pixels: `aexlo test`. `compat` only checks that the plugin runs.
- Then the workspace:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
```

## 5. Compare with the baseline

```bash
cargo build -p aexlo-cli
aexlo compat fixtures/plugins/macos --format json \
  --baseline target/compat/base.json > target/compat/after.json
echo $?
```

- The exit status must be 0, and `worse` empty. A worse scenario is fixed
  before anything else.
- The scenarios you aimed at should be in `better`. If they are not, another
  gap was hiding behind the one you closed: go back to [step 1](#1-pick-a-gap)
  with it.

## 6. Commit

`AGENTS.md` requires a commit for every change.

- One gap, one commit, such as `feat(aexlo): serve AEGP Stream Suite v11`.
  A fix to `compat` itself goes in a separate commit.
- Update `CHANGELOG.md`, and the docs in both English and Japanese
  (`*.ja.md`), when the change is visible to users.
- Do not tag a release from this loop. Tags come after review and merge.

Then save a new baseline (step 0) and pick the next gap.

## Running the loop

- **Track progress** by keeping the gap list at the end of each turn's report.
  The counts in `summary` show how far aexlo has come.
- **In parallel**, use one worktree per gap and pick gaps that do not touch
  the same suite. Cargo re-links the dylibs in `target/` on every build, so two
  loops must not build into and load from the same `target/` at once.
- **Windows** fixtures live in `fixtures/plugins/windows`; run the same loop
  there with `.aex` plugins when a change is platform-specific.
