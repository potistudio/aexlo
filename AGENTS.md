# Agent Guideline

## Development

MUST commit when you make any changes.

## Architecture

This crate is a host for existing After Effects plugin instances.

## Analyzing a plugin

To see what a plugin does (its commands, suite calls, allocations, errors),
use `aexlo inspect <plugin> --format json --script -` and read one JSON
object per line. The commands and every event field are in
[docs/inspect.md](docs/inspect.md). The TUI (`aexlo inspect` with no
`--exec` or `--script`) needs a terminal, so an agent cannot drive it.
