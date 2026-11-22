//! `aexlo` - a command-line front-end for the aexlo plugin loader.
//!
//! Load a real After Effects plugin (`.plugin` bundle on macOS, `.aex`/`.dll`
//! on Windows) outside of After Effects, inspect it, and render frames to PNG.
//!
//! This file only routes: [`Cli`] names the commands, and each command's
//! flags are a clap `Args` struct in its own module.

mod args;
mod bench;
mod compat;
mod dev;
mod info;
mod inspect;
mod param;
mod render;
mod toolkit;
mod watch;

use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

/// Run After Effects plugins without After Effects.
#[derive(Parser)]
#[command(name = "aexlo", version, after_help = PLUGIN_ARG, arg_required_else_help = true)]
struct Cli {
	#[command(subcommand)]
	command: Command,
}

#[derive(Subcommand)]
enum Command {
	/// Show what the plugin says about itself.
	///
	/// Its PiPL resource, its ABOUT text, the version and flags it declares at
	/// GLOBAL_SETUP (beside the PiPL's, with the bits that differ marked) and
	/// its parameters. With no section flag, every section; if the plugin
	/// fails to load, the PiPL is still printed, then the error.
	#[command(after_help = PLUGIN_ARG)]
	Info(info::InfoArgs),
	/// Render a frame and write it to a PNG or EXR.
	#[command(after_help = PLUGIN_ARG)]
	Render(render::RenderArgs),
	/// Drive the plugin from a prompt and trace what it does.
	///
	/// A TUI shows the commands and suite calls it makes (trace and totals),
	/// its parameters, host state, live memory (hex dump, changes per
	/// command) and the output. --exec and --script run without it;
	/// docs/inspect.md describes the commands and every JSON event.
	#[command(after_help = inspect::COMMANDS_HELP)]
	Inspect(inspect::InspectArgs),
	/// Time plugins and rank them by throughput, or time the manifest's
	/// presets.
	///
	/// With plugins: time each one and rank them by megapixels per second,
	/// with the same measurements as `cargo bench -p aexlo-bench`. Without:
	/// time the manifest's presets (each preset's `bench` samples and
	/// warmup), one at a time, with a phase breakdown (pre-render, render,
	/// GPU, host overhead).
	#[command(after_help = toolkit::EXIT_CODES)]
	Bench(bench::BenchArgs),
	/// Measure how far aexlo can drive each plugin.
	///
	/// Loads it, renders every path and depth, other inputs, sizes and times,
	/// and each parameter set to other values, each in a worker. Effects that
	/// render a text layer differently from the same pixels on an AV layer go
	/// through it all again on a text layer (text:*). Reports ok, partial
	/// (ran, but asked for something aexlo does not do), n/a, error, crash or
	/// timeout per scenario, and the missing suites, stubs and declined calls
	/// behind them. See docs/compat.md.
	Compat(compat::CompatArgs),
	/// List the variants the manifest's presets expand to.
	#[command(after_help = toolkit::EXIT_CODES)]
	Presets(toolkit::PresetsArgs),
	/// Run each variant in a worker and compare it with its golden.
	///
	/// Reports pass, fail, error, crash, timeout or skipped per variant.
	#[command(after_help = toolkit::EXIT_CODES)]
	Test(toolkit::TestArgs),
	/// What CI gates on: test --strict, then bench --baseline.
	///
	/// The bench runs when a baseline is configured. Parity failures against
	/// After Effects goldens are listed apart from regressions.
	#[command(after_help = toolkit::EXIT_CODES)]
	Check(toolkit::CheckArgs),
	/// Rerun a #[aexlo_test::preview] test on save, with a live window.
	///
	/// A built-in `bacon` replacement: the window shows the frame the preview
	/// test wrote last. Like `cargo build`, it works on the crate in the
	/// current directory unless -p names another.
	Dev(dev::DevArgs),
	/// What `test`, `bench` and `compat` spawn to run a plugin in.
	#[command(hide = true)]
	Worker,
}

/// What a `<PLUGIN>` argument may be.
const PLUGIN_ARG: &str = "\
<PLUGIN> is a path to the plugin (.plugin, .aex, .dll), or a bare name the \
platform's extension is added to (SDK_Noise -> SDK_Noise.plugin). info, \
render, inspect and bench also take a crate directory (with a Cargo.toml) \
and build its cdylib first.";

fn main() -> ExitCode {
	let _ = env_logger::try_init();
	// A usage error exits 2, like the preset-driven commands' invalid flags.
	let mut command = whole_descriptions(Cli::command());
	let cli = Cli::from_arg_matches(&command.get_matches_mut()).unwrap_or_else(|err| err.format(&mut command).exit());
	match cli.command {
		Command::Info(args) => report(info::run(args)),
		Command::Render(args) => report(render::run(args)),
		Command::Inspect(args) => report(inspect::run(args)),
		Command::Bench(args) => bench::cmd(args),
		Command::Compat(args) => compat::cmd(args),
		Command::Presets(args) => toolkit::cmd_presets(args),
		Command::Test(args) => toolkit::cmd_test(args),
		Command::Check(args) => toolkit::cmd_check(args),
		Command::Dev(args) => report(dev::cmd(args)),
		Command::Worker => toolkit::cmd_worker(),
	}
}

/// The help of a command with a description: its first line, the rest of the
/// description, then usage and options.
const DESCRIBED_HELP: &str = "\
{about-with-newline}
{before-help}{usage-heading} {usage}

{all-args}{after-help}";

/// Show each command's whole description under both `-h` and `--help`.
///
/// clap prints only a doc comment's first paragraph for `-h` and keeps the
/// rest for `--help`, so `aexlo compat -h` said what compat is for but not
/// what it does. The first paragraph stays the about (the one line `aexlo
/// help` lists); the rest moves to before-help, which both print, placed
/// after the about.
fn whole_descriptions(cli: clap::Command) -> clap::Command {
	cli.mut_subcommands(|command| {
		let Some(long) = command.get_long_about().map(ToString::to_string) else {
			return command;
		};
		let Some((_, rest)) = long.split_once("\n\n") else {
			return command;
		};
		let rest = rest.to_owned();
		command.long_about(None).before_help(rest).help_template(DESCRIBED_HELP)
	})
}

/// Exit 1 after printing the error. The preset-driven commands choose their
/// own codes instead (`docs/toolkit.md` §12).
fn report(result: anyhow::Result<()>) -> ExitCode {
	match result {
		Ok(()) => ExitCode::SUCCESS,
		Err(err) => {
			eprintln!("error: {err:#}");
			ExitCode::FAILURE
		}
	}
}

#[cfg(test)]
mod tests {
	use clap::CommandFactory;

	use super::*;

	#[test]
	fn the_command_line_is_consistent() {
		whole_descriptions(Cli::command()).debug_assert();
	}
}
