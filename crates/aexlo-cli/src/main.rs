//! `aexlo` - a command-line front-end for the aexlo plugin loader.
//!
//! Load a real After Effects plugin (`.plugin` bundle on macOS, `.aex`/`.dll`
//! on Windows) outside of After Effects, inspect it, and render frames to PNG.
//!
//! This file only routes: [`COMMANDS`] names each command, its help and the
//! function that runs it; each command reads its own flags in its module.

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

/// The arguments after the command's name.
type Args = std::vec::IntoIter<String>;

/// How a command runs, and so how its exit code is chosen.
enum Run {
	/// Exit 1 on an error.
	Simple(fn(Args) -> anyhow::Result<()>),
	/// Chooses its own exit code (`docs/toolkit.md` §12: 1 judged bad,
	/// 2 invalid, 3 harness error).
	Coded(fn(Args) -> ExitCode),
}

struct Command {
	name: &'static str,
	/// Printed in order by `aexlo help <name>` and `--help`.
	help: &'static [&'static str],
	run: Run,
}

const COMMANDS: &[Command] = &[
	Command {
		name: "about",
		help: &[info::ABOUT_HELP, PLUGIN_ARG],
		run: Run::Simple(info::about),
	},
	Command {
		name: "params",
		help: &[info::PARAMS_HELP, PLUGIN_ARG],
		run: Run::Simple(info::params),
	},
	Command {
		name: "render",
		help: &[render::HELP, PLUGIN_ARG],
		run: Run::Simple(render::run),
	},
	Command {
		name: "dialog",
		help: &[info::DIALOG_HELP, PLUGIN_ARG],
		run: Run::Simple(info::dialog),
	},
	Command {
		name: "inspect",
		help: &[inspect::HELP, inspect::COMMANDS_HELP],
		run: Run::Simple(inspect::run),
	},
	Command {
		name: "bench",
		help: &[bench::HELP, toolkit::PRESET_OPTIONS, PLUGIN_ARG],
		run: Run::Coded(bench::cmd),
	},
	Command {
		name: "compat",
		help: &[compat::HELP],
		run: Run::Coded(compat::cmd_compat),
	},
	Command {
		name: "presets",
		help: &[toolkit::PRESETS_HELP, toolkit::PRESET_OPTIONS],
		run: Run::Coded(toolkit::cmd_presets),
	},
	Command {
		name: "test",
		help: &[toolkit::TEST_HELP, toolkit::PRESET_OPTIONS],
		run: Run::Coded(toolkit::cmd_test),
	},
	Command {
		name: "check",
		help: &[toolkit::CHECK_HELP, toolkit::PRESET_OPTIONS],
		run: Run::Coded(toolkit::cmd_check),
	},
	Command {
		name: "dev",
		help: &[dev::HELP],
		run: Run::Simple(dev::cmd),
	},
	Command {
		name: "preview",
		help: &[preview::HELP, PLUGIN_ARG],
		run: Run::Simple(preview::cmd),
	},
	Command {
		name: "view",
		help: &[view::HELP],
		run: Run::Simple(view::cmd),
	},
];

const OVERVIEW: &str = "\
aexlo - run After Effects plugins without After Effects

Usage: aexlo <command> [args] [options]
       aexlo help <command>       Show a command's options (or <command> --help)

Plugins:
  about <plugin>          Print the plugin's ABOUT text
  params <plugin>         List the plugin's parameters (index, name, value)
  render <plugin>         Render a frame and write it to a PNG or EXR
  dialog <plugin>         Send PF_Cmd_DO_DIALOG (the effect's Options button)
  inspect <plugin>        Drive the plugin from a prompt and trace what it does
  bench <plugin>...       Time plugins and rank them by throughput
  compat <plugin|dir>...  Measure how far aexlo can drive each plugin

Presets (aexlo.toml):
  presets [filter]        List the variants the manifest's presets expand to
  test [filter]           Run each variant in a worker and compare it with
                          its golden
  bench [filter]          Time the presets, with a phase breakdown
  check [filter]          What CI gates on: test --strict, then bench
                          --baseline

Developing a plugin:
  dev [filter]            Rerun a #[aexlo_test::preview] test on save, with a
                          live viewer
  preview <plugin>        Preview a built plugin in the browser, with live
                          controls
  view <png>              Show a PNG in a window, reloading it when it changes
";

/// What a `<plugin>` argument may be.
const PLUGIN_ARG: &str = "
<plugin> is a path to the plugin (.plugin, .aex, .dll), or a bare name the
platform's extension is added to (SDK_Noise -> SDK_Noise.plugin). about,
params, render, dialog and bench also take a crate directory (with a
Cargo.toml) and build its cdylib first - a one-shot alternative to `dev --bin`.
";

/// Exit code for a command line that names no command aexlo has.
const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
	let _ = env_logger::try_init();
	let mut args = std::env::args().skip(1);
	let Some(name) = args.next() else {
		eprint!("{OVERVIEW}{PLUGIN_ARG}");
		return ExitCode::from(USAGE_ERROR);
	};
	let args: Vec<String> = args.collect();
	match name.as_str() {
		"-h" | "--help" | "help" => return help(args.first().map(String::as_str)),
		"-V" | "--version" => {
			println!("aexlo {}", env!("CARGO_PKG_VERSION"));
			return ExitCode::SUCCESS;
		}
		// What `test`, `bench` and `compat` spawn to run a plugin in.
		"worker" => return toolkit::cmd_worker(),
		_ => {}
	}
	let Some(command) = find(&name) else {
		eprintln!("error: unknown command '{name}' (`aexlo help` lists them)");
		return ExitCode::from(USAGE_ERROR);
	};
	if args.iter().any(|arg| arg == "-h" || arg == "--help") {
		print_help(command);
		return ExitCode::SUCCESS;
	}
	match command.run {
		Run::Simple(run) => report(run(args.into_iter())),
		Run::Coded(run) => run(args.into_iter()),
	}
}

fn find(name: &str) -> Option<&'static Command> {
	COMMANDS.iter().find(|command| command.name == name)
}

/// `aexlo help [<command>]`.
fn help(name: Option<&str>) -> ExitCode {
	match name {
		None => {
			print!("{OVERVIEW}{PLUGIN_ARG}");
			ExitCode::SUCCESS
		}
		Some(name) => match find(name) {
			Some(command) => {
				print_help(command);
				ExitCode::SUCCESS
			}
			None => {
				eprintln!("error: unknown command '{name}' (`aexlo help` lists them)");
				ExitCode::from(USAGE_ERROR)
			}
		},
	}
}

fn print_help(command: &Command) {
	for part in command.help {
		print!("{part}");
	}
}

/// The exit code for a [`Run::Simple`] command, after printing its error.
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
	use super::*;

	#[test]
	fn every_command_is_in_the_overview_and_its_help() {
		for command in COMMANDS {
			let line = format!("\n  {} ", command.name);
			assert!(OVERVIEW.contains(&line), "{} is missing from OVERVIEW", command.name);
			let usage = format!("Usage: aexlo {} ", command.name);
			assert!(
				command.help[0].starts_with(&usage),
				"{}'s help starts otherwise",
				command.name
			);
		}
	}

	#[test]
	fn the_overview_names_only_commands() {
		for line in OVERVIEW.lines().filter(|line| line.starts_with("  ")) {
			let word = line.split_whitespace().next().unwrap();
			if line.starts_with("   ") {
				continue;
			}
			assert!(find(word).is_some(), "OVERVIEW lists '{word}', which no command is");
		}
	}

	#[test]
	fn help_lines_fit_80_columns() {
		let parts = COMMANDS
			.iter()
			.flat_map(|c| c.help.iter())
			.chain([&OVERVIEW, &PLUGIN_ARG]);
		for part in parts {
			for line in part.lines() {
				assert!(line.chars().count() <= 80, "too wide: {line}");
			}
		}
	}
}
