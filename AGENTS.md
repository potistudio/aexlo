# Agent Guideline

## Development

MUST commit when you make any changes.

## Architecture

This crate is a host for existing After Effects plugin instances.

## Analyzing a plugin

To see what a plugin does (its commands, suite calls, allocations, errors),
use `aexlo inspect <plugin> --json --script -` and read one JSON
object per line. The commands and every event field are in
[docs/inspect.md](docs/inspect.md). The TUI (`aexlo inspect` with no
`--exec` or `--script`) needs a terminal, so an agent cannot drive it.

## Measuring how far aexlo drives plugins

To see which plugins aexlo can run, under which conditions, and what they
ask for that aexlo does not do, use
`aexlo compat fixtures/plugins/<platform> --json`. Save a baseline
before changing aexlo and pass it back with `--baseline` afterwards; exit 1
means a scenario got worse. See [docs/compat.md](docs/compat.md).

To close a gap that `compat` reports, follow the loop in
[docs/compat-workflow.md](docs/compat-workflow.md).

## Checking the SDK bindings

To check that the `after-effects-sys` structs aexlo and `playground/probe` use match
the After Effects SDK headers, run `cargo test -p aexlo_sdk_layout` on
Windows x64, the only target with a snapshot so far. Elsewhere, the snapshot
tests are ignored and compare nothing. When a test asks for a new snapshot, see
[docs/sdk-layout.md](docs/sdk-layout.md).
