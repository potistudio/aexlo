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
  press <button>            Press a button (index, #index, name or the text on
                            it); prints the values the plugin changed
  updateui                  PF_Cmd_UPDATE_PARAMS_UI
  dialog                    PF_Cmd_DO_DIALOG (the Options button)
  time <frame> [fps]        Move to a frame  [fps: 30]
  size <w> <h>              Output size of later renders
  input <png> | input <black|white|gradient> [w h]
                            Replace the input layer (keeps its depth)
  input text <string>       Make the layer a text layer showing <string>
                            (\n breaks lines): the plugin gets its glyph
                            outlines and the text, white, as its input
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
  report                    (--exec/--script) Print the analysis now
  help                      This text
  quit                      Leave (q in normal mode)
";

/// A parsed prompt command.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
	About,
	Render(RenderPath),
	/// `set "<key>" <value>` or `set <key>=<value>`: the key as given.
	Set {
		key: String,
		value: String,
	},
	/// `set <words...>`: the engine finds where the name ends.
	SetWords(String),
	Changed(String),
	/// `press <button>`: the key as given.
	Press(String),
	UpdateUi,
	Dialog,
	Time {
		frame: i32,
		fps: u32,
	},
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
	Report,
	Help,
	Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputSource {
	Png(PathBuf),
	Fill([f32; 4], Option<(u32, u32)>),
	Gradient(Option<(u32, u32)>),
	/// A text layer showing this string.
	Text(String),
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
			Command::Mem(_)
				| Command::Focus(_)
				| Command::Find(_)
				| Command::Clear
				| Command::Report
				| Command::Help
				| Command::Quit
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
		"set" => match split_set(rest) {
			Some((key, value)) => Command::Set { key, value },
			None if rest.split_whitespace().count() >= 2 => Command::SetWords(rest.to_string()),
			None => bail!("set <param> <value>"),
		},
		"changed" => match unquote(rest) {
			"" => bail!("changed <param>"),
			key => Command::Changed(key.to_string()),
		},
		"press" => match unquote(rest) {
			"" => bail!("press <button>"),
			key => Command::Press(key.to_string()),
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
		"input" => match args.first().copied() {
			Some("text") => match rest.strip_prefix("text").map(unquote) {
				Some(text) if !text.is_empty() => Command::Input(InputSource::Text(text.replace("\\n", "\n"))),
				_ => bail!("input text <string>"),
			},
			Some(kind @ ("black" | "white" | "gradient")) => {
				let size = match &args[1..] {
					[w, h] => Some((dimension(w)?, dimension(h)?)),
					[] => None,
					_ => bail!("input <black|white|gradient> [w h]"),
				};
				Command::Input(match kind {
					"black" => InputSource::Fill([0.0, 0.0, 0.0, 1.0], size),
					"white" => InputSource::Fill([1.0; 4], size),
					_ => InputSource::Gradient(size),
				})
			}
			// Everything else is a path, spaces and all.
			Some(_) => Command::Input(InputSource::Png(PathBuf::from(unquote(rest)))),
			None => bail!("input <png> | input <black|white|gradient> [w h] | input text <string>"),
		},
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
		// `poison, guard` is one list.
		"strict" => Command::Strict(parse_strict(&args.concat())?),
		"serial" => Command::Serial(match args.as_slice() {
			["on"] => true,
			["off"] => false,
			_ => bail!("serial <on|off>"),
		}),
		"save" => match unquote(rest) {
			"" => bail!("save <path>"),
			path => Command::Save(PathBuf::from(path)),
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
		"report" => none("report").map(|_| Command::Report)?,
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

fn parse_strict(list: &str) -> Result<Strict> {
	match list {
		"on" | "all" => Ok(Strict::all()),
		"off" | "none" => Ok(Strict::default()),
		"" => bail!("strict <on|off|poison,guard,alloc,checkout>"),
		list => {
			let mut strict = Strict::default();
			for item in list.split(',').filter(|item| !item.is_empty()) {
				match item {
					"poison" => strict.poison_output = true,
					"guard" | "guards" => strict.guard_bands = true,
					"alloc" | "allocations" => strict.track_allocations = true,
					"checkout" | "checkouts" => strict.track_checkouts = true,
					other => bail!("unknown strict check '{other}' (poison, guard, alloc, checkout)"),
				}
			}
			Ok(strict)
		}
	}
}

/// `"text"` or `'text'` without its quotes; anything else as it is.
pub fn unquote(text: &str) -> &str {
	let text = text.trim();
	for quote in ['"', '\''] {
		if let Some(inner) = text.strip_prefix(quote).and_then(|t| t.strip_suffix(quote)) {
			return inner;
		}
	}
	text
}

/// `set`'s key and value when the line says where the key ends: quoted
/// (`"Noise variation" 25`) or before an `=` (`Noise variation=25`).
/// `None` for plain words (`Noise variation 25`), which the engine splits.
fn split_set(rest: &str) -> Option<(String, String)> {
	let (key, value) = ['"', '\'']
		.into_iter()
		.find_map(|quote| rest.strip_prefix(quote)?.split_once(quote))
		.or_else(|| rest.split_once('='))?;
	let (key, value) = (key.trim(), value.trim().trim_start_matches('=').trim());
	(!key.is_empty() && !value.is_empty()).then(|| (key.to_string(), value.to_string()))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_commands_and_scripts() {
		assert_eq!(parse("render smart").unwrap(), Command::Render(RenderPath::Smart));
		assert_eq!(
			parse(":set Center 10, 20").unwrap(),
			Command::SetWords("Center 10, 20".into())
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
		let set = |key: &str, value: &str| Command::Set {
			key: key.into(),
			value: value.into(),
		};
		assert_eq!(
			parse("set \"Noise variation\" 25").unwrap(),
			set("Noise variation", "25")
		);
		assert_eq!(parse("set Noise variation=25").unwrap(), set("Noise variation", "25"));
		assert_eq!(parse("set 'Gain' = 2").unwrap(), set("Gain", "2"));
		assert_eq!(
			parse("changed \"Threshold Min\"").unwrap(),
			Command::Changed("Threshold Min".into())
		);
		assert_eq!(
			parse("changed Threshold Min").unwrap(),
			Command::Changed("Threshold Min".into())
		);
		assert_eq!(
			parse("save \"out dir/a b.png\"").unwrap(),
			Command::Save("out dir/a b.png".into())
		);
		let Command::Strict(spaced) = parse("strict poison, guard").unwrap() else {
			panic!()
		};
		assert!(spaced.poison_output && spaced.guard_bands && !spaced.track_allocations);
		assert!(parse("set Gain").is_err());
		assert_eq!(
			parse("input my frames/a b.png").unwrap(),
			Command::Input(InputSource::Png("my frames/a b.png".into()))
		);
		assert_eq!(
			parse("input black 4 2").unwrap(),
			Command::Input(InputSource::Fill([0.0, 0.0, 0.0, 1.0], Some((4, 2))))
		);
		assert_eq!(
			parse("input text \"Hello world\"").unwrap(),
			Command::Input(InputSource::Text("Hello world".into()))
		);
		assert_eq!(
			parse("input text two\\nlines").unwrap(),
			Command::Input(InputSource::Text("two\nlines".into()))
		);
		assert!(parse("input text").is_err());
		assert!(parse("render sideways").is_err());
		assert!(parse("frobnicate").is_err());

		assert_eq!(
			parse("press \"Reset Color\"").unwrap(),
			Command::Press("Reset Color".into())
		);
		assert_eq!(parse("press #7").unwrap(), Command::Press("#7".into()));
		assert!(parse("press").is_err());

		let script = parse_script("about; render\n# note\nset 1 2").unwrap();
		let names: Vec<&str> = script.iter().map(|(line, _)| line.as_str()).collect();
		assert_eq!(names, ["about", "render", "set 1 2"]);
	}
}
