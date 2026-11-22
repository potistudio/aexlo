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
mod preview;
mod render;
mod session;
mod toolkit;
mod view;
mod viewer;
mod watch;
mod web;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Run After Effects plugins without After Effects.
#[derive(Parser)]
#[command(name = "aexlo", version, after_help = PLUGIN_ARG, arg_required_else_help = true)]
struct Cli {
	#[command(subcommand)]
	command: Command,
}

#[derive(Subcommand)]
enum Command {
	/// Print the plugin's ABOUT text.
	About(info::PluginArgs),
	/// List the plugin's parameters.
	///
	/// Prints each one's index, name, kind and value, whether it is hidden,
	/// and its UI flags.
	Params(info::PluginArgs),
	/// Render a frame and write it to a PNG or EXR.
	#[command(after_help = PLUGIN_ARG)]
	Render(render::RenderArgs),
	/// Send PF_Cmd_DO_DIALOG (the effect's Options button).
	///
	/// Prints the plugin's flags and parameters first, and keeps the process
	/// alive while a plugin window is visible.
	Dialog(info::PluginArgs),
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
	/// Rerun a #[aexlo_test::preview] test on save, with a live viewer.
	///
	/// A built-in `bacon` replacement. Like `cargo build`, it works on the
	/// crate in the current directory unless -p names another.
	Dev(dev::DevArgs),
	/// Preview a built plugin in the browser, with live controls.
	///
	/// Serves a finished .plugin/.aex/.dll and exposes its parameters as live
	/// controls. No compiler in the loop: this is the `preview` to `dev`'s
	/// watch loop.
	Preview(preview::PreviewArgs),
	/// Show a PNG in a window, reloading it when it changes.
	///
	/// A `dev`-driven #[aexlo_test::preview] opens one by itself; pair it
	/// with your own re-runner otherwise.
	View(view::ViewArgs),
	/// What `test`, `bench` and `compat` spawn to run a plugin in.
	#[command(hide = true)]
	Worker,
}

/// What a `<PLUGIN>` argument may be.
const PLUGIN_ARG: &str = "\
<PLUGIN> is a path to the plugin (.plugin, .aex, .dll), or a bare name the \
platform's extension is added to (SDK_Noise -> SDK_Noise.plugin). about, \
params, render, dialog, inspect and bench also take a crate directory (with \
a Cargo.toml) and build its cdylib first.";

fn main() -> ExitCode {
	let _ = env_logger::try_init();
	// A usage error exits 2, like the preset-driven commands' invalid flags.
	let cli = Cli::parse();
	match cli.command {
		Command::About(args) => report(info::about(args)),
		Command::Params(args) => report(info::params(args)),
		Command::Render(args) => report(render::run(args)),
		Command::Dialog(args) => report(info::dialog(args)),
		Command::Inspect(args) => report(inspect::run(args)),
		Command::Bench(args) => bench::cmd(args),
		Command::Compat(args) => compat::cmd(args),
		Command::Presets(args) => toolkit::cmd_presets(args),
		Command::Test(args) => toolkit::cmd_test(args),
		Command::Check(args) => toolkit::cmd_check(args),
		Command::Dev(args) => report(dev::cmd(args)),
		Command::Preview(args) => report(preview::cmd(args)),
		Command::View(args) => report(view::cmd(args)),
		Command::Worker => toolkit::cmd_worker(),
	}
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
		Cli::command().debug_assert();
	}
}
