//! The commands `aexlo inspect` takes at its prompt (and from `--exec`).

use std::path::PathBuf;

use aexlo::{ObserveLevel, PixelDepthKind, RenderPath, Strict};
use anyhow::{Context, Result, bail};

pub const HELP: &str = "\
PLUGIN COMMANDS (each runs on the plugin's thread and is traced)
  about                     PF_Cmd_ABOUT; prints the plugin's text
  render [auto|smart|legacy|gpu]
                            Render a frame through that path  [auto]
  set <param> <value>       Set a parameter (index, #index or name)
  changed <param>           PF_Cmd_USER_CHANGED_PARAM (+ UPDATE_PARAMS_UI if asked)
  updateui                  PF_Cmd_UPDATE_PARAMS_UI
  dialog                    PF_Cmd_DO_DIALOG (the Options button)
  time <frame> [fps]        Move to a frame  [fps: 30]
  size <w> <h>              Output size of later renders
  input <png|black|white|gradient> [w h]
                            Replace the input layer (keeps its depth)
  depth <8|16|32>           Convert the input (project) depth
  reload                    Tear the plugin down and load it again (a crate
                            given as <plugin> is rebuilt first)
HOST SETTINGS
  level <commands|calls|args>
                            Trace only commands, every suite call too, or
                            calls with their arguments  [args]
  strict <on|off|poison,guard,alloc,checkout>
                            Strict-mode checks; findings go to State
  serial <on|off>           Run iterate callbacks on the plugin's thread, so
                            their suite calls and allocations are traced
  save <path>               Write the output frame (.png, .exr)
VIEW
  mem <address|global|sequence>
                            Show an allocation in the Memory tab
  focus <text|#seq>         Diagnose the newest trace line containing <text>
                            (or entry #seq): its arguments, the command it
                            ran in, the memory it names, what came around it
  find <text>               Filter the Trace and Calls tabs (empty: clear)
  clear                     Forget the trace and totals
  help                      This text
  quit                      Leave (q in normal mode)
";

/// A parsed prompt command.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
	About,
	Render(RenderPath),
	Set { key: String, value: String },
	Changed(String),
	UpdateUi,
	Dialog,
	Time { frame: i32, fps: u32 },
	Size(u32, u32),
	Input(InputSource),
	Depth(PixelDepthKind),
	Reload,
	Level(ObserveLevel),
	Strict(Strict),
	Serial(bool),
	Save(PathBuf),
	Mem(MemTarget),
	Focus(String),
	Find(String),
	Clear,
	Help,
	Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputSource {
	Png(PathBuf),
	Fill([f32; 4], Option<(u32, u32)>),
	Gradient(Option<(u32, u32)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemTarget {
	Address(usize),
	Global,
	Sequence,
}

impl Command {
	/// Whether the command runs on the plugin's thread (as opposed to only
	/// changing what the UI shows).
	pub fn is_engine(&self) -> bool {
		!matches!(
			self,
			Command::Mem(_) | Command::Focus(_) | Command::Find(_) | Command::Clear | Command::Help | Command::Quit
		)
	}
}

/// Split a command line on `;` into its commands; blank ones are skipped.
pub fn parse_script(text: &str) -> Result<Vec<(String, Command)>> {
	text.split([';', '\n'])
		.map(str::trim)
		.filter(|line| !line.is_empty() && !line.starts_with('#'))
		.map(|line| parse(line).map(|cmd| (line.to_string(), cmd)))
		.collect()
}

/// Parse one command line.
pub fn parse(line: &str) -> Result<Command> {
	let line = line.trim().trim_start_matches(':');
	let (head, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
	let rest = rest.trim();
	let args: Vec<&str> = rest.split_whitespace().collect();
	let none = |name: &str| -> Result<()> {
		if args.is_empty() {
			Ok(())
		} else {
			bail!("{name} takes no arguments")
		}
	};
	let command = match head.to_ascii_lowercase().as_str() {
		"about" => none("about").map(|_| Command::About)?,
		"render" | "r" => Command::Render(match args.as_slice() {
			[] | ["auto"] => RenderPath::Auto,
			["smart"] => RenderPath::Smart,
			["legacy"] => RenderPath::Legacy,
			["gpu"] => RenderPath::Gpu,
			_ => bail!("render [auto|smart|legacy|gpu]"),
		}),
		"set" => {
			// The value may contain spaces ("1, 2"): everything after the key.
			let (key, value) = rest
				.split_once(char::is_whitespace)
				.or_else(|| rest.split_once('='))
				.context("set <param> <value>")?;
			Command::Set {
				key: key.trim().to_string(),
				value: value.trim().trim_start_matches('=').trim().to_string(),
			}
		}
		"changed" => match args.as_slice() {
			[key] => Command::Changed(key.to_string()),
			_ => bail!("changed <param>"),
		},
		"updateui" | "update-ui" => none("updateui").map(|_| Command::UpdateUi)?,
		"dialog" => none("dialog").map(|_| Command::Dialog)?,
		"time" => match args.as_slice() {
			[frame] => Command::Time {
				frame: frame.parse().context("time <frame> [fps]")?,
				fps: 30,
			},
			[frame, fps] => Command::Time {
				frame: frame.parse().context("time <frame> [fps]")?,
				fps: fps
					.parse()
					.ok()
					.filter(|&f| f > 0)
					.context("fps must be a positive integer")?,
			},
			_ => bail!("time <frame> [fps]"),
		},
		"size" => match args.as_slice() {
			[w, h] => Command::Size(dimension(w)?, dimension(h)?),
			_ => bail!("size <w> <h>"),
		},
		"input" => {
			let size = match args.get(1..) {
				Some([w, h]) => Some((dimension(w)?, dimension(h)?)),
				Some([]) | None => None,
				_ => bail!("input <png|black|white|gradient> [w h]"),
			};
			Command::Input(match args.first().copied() {
				Some("black") => InputSource::Fill([0.0, 0.0, 0.0, 1.0], size),
				Some("white") => InputSource::Fill([1.0; 4], size),
				Some("gradient") => InputSource::Gradient(size),
				Some(path) if size.is_none() => InputSource::Png(PathBuf::from(path)),
				_ => bail!("input <png|black|white|gradient> [w h]"),
			})
		}
		"depth" => Command::Depth(match args.as_slice() {
			["8"] => PixelDepthKind::U8,
			["16"] => PixelDepthKind::U16,
			["32"] => PixelDepthKind::F32,
			_ => bail!("depth <8|16|32>"),
		}),
		"reload" => none("reload").map(|_| Command::Reload)?,
		"level" => Command::Level(match args.as_slice() {
			["commands" | "cmd"] => ObserveLevel::Commands,
			["calls" | "call"] => ObserveLevel::Calls,
			["args" | "arguments"] => ObserveLevel::Arguments,
			_ => bail!("level <commands|calls|args>"),
		}),
		"strict" => Command::Strict(parse_strict(&args)?),
		"serial" => Command::Serial(match args.as_slice() {
			["on"] => true,
			["off"] => false,
			_ => bail!("serial <on|off>"),
		}),
		"save" => match args.as_slice() {
			[_] => Command::Save(PathBuf::from(rest)),
			_ => bail!("save <path>"),
		},
		"mem" | "memory" => Command::Mem(match args.as_slice() {
			["global"] => MemTarget::Global,
			["sequence" | "seq"] => MemTarget::Sequence,
			[address] => MemTarget::Address(parse_address(address)?),
			_ => bail!("mem <address|global|sequence>"),
		}),
		"find" | "/" => Command::Find(rest.to_string()),
		"focus" | "f" | "diag" => {
			if rest.is_empty() {
				bail!("focus <text|#seq>");
			}
			Command::Focus(rest.to_string())
		}
		"clear" => none("clear").map(|_| Command::Clear)?,
		"help" | "?" => Command::Help,
		"quit" | "q" | "exit" => Command::Quit,
		"" => bail!("empty command"),
		other => bail!("unknown command '{other}' (try `help`)"),
	};
	Ok(command)
}

fn dimension(raw: &str) -> Result<u32> {
	raw.parse()
		.ok()
		.filter(|&v: &u32| v > 0 && v <= 16384)
		.with_context(|| format!("'{raw}' is not a size in 1..=16384"))
}

/// `0x7f..` or decimal.
pub fn parse_address(raw: &str) -> Result<usize> {
	let parsed = match raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
		Some(hex) => usize::from_str_radix(hex, 16),
		None => raw.parse(),
	};
	parsed.with_context(|| format!("'{raw}' is not an address"))
}

fn parse_strict(args: &[&str]) -> Result<Strict> {
	match args {
		["on" | "all"] => Ok(Strict::all()),
		["off" | "none"] => Ok(Strict::default()),
		[list] => {
			let mut strict = Strict::default();
			for item in list.split(',') {
				match item.trim() {
					"poison" => strict.poison_output = true,
					"guard" | "guards" => strict.guard_bands = true,
					"alloc" | "allocations" => strict.track_allocations = true,
					"checkout" | "checkouts" => strict.track_checkouts = true,
					other => bail!("unknown strict check '{other}' (poison, guard, alloc, checkout)"),
				}
			}
			Ok(strict)
		}
		_ => bail!("strict <on|off|poison,guard,alloc,checkout>"),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_commands_and_scripts() {
		assert_eq!(parse("render smart").unwrap(), Command::Render(RenderPath::Smart));
		assert_eq!(
			parse(":set Center 10, 20").unwrap(),
			Command::Set {
				key: "Center".into(),
				value: "10, 20".into()
			}
		);
		assert_eq!(
			parse("set 2=0.5").unwrap(),
			Command::Set {
				key: "2".into(),
				value: "0.5".into()
			}
		);
		assert_eq!(parse("mem 0x10").unwrap(), Command::Mem(MemTarget::Address(16)));
		assert_eq!(parse("time 12 24").unwrap(), Command::Time { frame: 12, fps: 24 });
		let strict = parse("strict poison,alloc").unwrap();
		let Command::Strict(strict) = strict else { panic!() };
		assert!(strict.poison_output && strict.track_allocations && !strict.guard_bands);
		assert!(parse("render sideways").is_err());
		assert!(parse("frobnicate").is_err());

		let script = parse_script("about; render\n# note\nset 1 2").unwrap();
		let names: Vec<&str> = script.iter().map(|(line, _)| line.as_str()).collect();
		assert_eq!(names, ["about", "render", "set 1 2"]);
	}
}
