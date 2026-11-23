//! The plugin side of `aexlo inspect`: owns the [`PluginInstance`], runs
//! commands against it and takes snapshots of what the UI shows.
//!
//! A `PluginInstance` stays on the thread that made it, so the TUI runs an
//! [`Engine`] on a thread of its own and talks to it over channels.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aexlo::{
	AnyLayer, Comp, InstanceState, Keyframe, LayerContent, LiveAllocation, MaskPath, MaskVertex, ObserveLevel,
	OutFlags, OutFlags2, ParamKind, ParamValue, PixelDepthKind, PluginInstance, Project, ProjectLayer, ShapeFill,
	ShapeGroup, ShapeItem, ShapePath, Strict, TextLayer, Time, TimePoint, Track,
};
use anyhow::{Context, Result, bail};

use super::command::{Command, InputSource, Outline};
use super::recorder::Recorder;

/// How the plugin is loaded (and reloaded).
#[derive(Clone)]
pub struct Setup {
	/// The plugin artifact.
	pub path: PathBuf,
	/// The crate's `Cargo.toml` when the plugin was given as a crate:
	/// `reload` rebuilds it.
	pub manifest: Option<PathBuf>,
	/// The input layer; its depth is the project depth.
	pub input: AnyLayer,
	/// The text when the layer is a text layer (`input text`).
	pub text: Option<TextLayer>,
	/// The project around the effect when the layer is a shape layer
	/// (`input shape`).
	pub project: Option<Project>,
	/// The layer's masks (`mask`).
	pub masks: Vec<MaskPath>,
	/// The font `input text` sets text in; `None` is a system sans-serif.
	pub font: Option<PathBuf>,
	pub size: Option<(u32, u32)>,
	pub level: ObserveLevel,
	pub strict: Strict,
	pub serial: bool,
}

/// Largest preview of the output frame, in pixels.
const PREVIEW_MAX: u32 = 320;

/// One parameter, ready to show.
#[derive(Debug, Clone)]
pub struct ParamRow {
	pub index: usize,
	pub name: String,
	pub kind: String,
	pub value: String,
	pub detail: String,
	pub hidden: bool,
	pub disabled: bool,
}

/// A small RGBA8 copy of the output.
#[derive(Debug, Clone, Default)]
pub struct Preview {
	pub width: u32,
	pub height: u32,
	pub rgba: Vec<u8>,
}

/// What the UI shows of the instance, taken after every command.
#[derive(Debug, Clone)]
pub struct Snapshot {
	pub path: PathBuf,
	pub params: Vec<ParamRow>,
	pub state: InstanceState,
	pub out_flags: OutFlags,
	pub out_flags2: OutFlags2,
	pub smart: bool,
	pub gpu: bool,
	pub custom_ui: bool,
	pub options_button: Option<String>,
	pub pixel_formats: Vec<String>,
	pub input: (u32, u32, PixelDepthKind),
	pub output: (u32, u32, PixelDepthKind),
	pub time: Time,
	pub parallel_iterate: bool,
	pub strict: Strict,
	pub level: ObserveLevel,
	pub allocations: Vec<LiveAllocation>,
	/// Every strict-mode finding so far, oldest first.
	pub findings: Vec<String>,
	/// Output pixels left unwritten, when the output is poisoned.
	pub unwritten: Option<usize>,
	/// Mean straight RGBA of the output, 0..1.
	pub mean: [f32; 4],
	pub preview: Preview,
}

pub struct Engine {
	setup: Setup,
	recorder: Recorder,
	instance: Option<PluginInstance>,
	/// The private copy `instance` was loaded from, removed after it.
	staged: Option<PathBuf>,
	loads: u32,
	findings: Vec<String>,
}

/// What a command printed, and how long it took.
pub struct Outcome {
	pub message: String,
	pub elapsed: Duration,
}

impl Engine {
	/// Load the plugin with `recorder` observing from `GLOBAL_SETUP` on.
	pub fn load(setup: Setup, recorder: Recorder) -> Result<Self> {
		let mut engine = Self {
			setup,
			recorder,
			instance: None,
			staged: None,
			loads: 0,
			findings: Vec::new(),
		};
		engine.recorder.step(&format!("load {}", engine.setup.path.display()));
		engine.open()?;
		engine.collect_findings();
		Ok(engine)
	}

	/// Load the plugin into `self.instance`.
	///
	/// A single-file plugin is loaded from a private copy: cargo re-links its
	/// output in place, and reopening a path can hand back the image already
	/// mapped instead of a rebuilt one.
	fn open(&mut self) -> Result<()> {
		self.loads += 1;
		let staged = if self.setup.path.is_file() {
			let ext = self.setup.path.extension().and_then(|e| e.to_str()).unwrap_or("dylib");
			let staged =
				std::env::temp_dir().join(format!("aexlo-inspect-{}-{}.{ext}", std::process::id(), self.loads));
			crate::watch::copy_built(&self.setup.path, &staged)
				.with_context(|| format!("staging {}", self.setup.path.display()))?;
			Some(staged)
		} else {
			None
		};
		let result = self.load_from(staged.as_deref().unwrap_or(&self.setup.path));
		match result {
			Ok(instance) => {
				self.instance = Some(instance);
				self.staged = staged;
				Ok(())
			}
			Err(err) => {
				if let Some(staged) = staged {
					let _ = std::fs::remove_file(staged);
				}
				Err(err)
			}
		}
	}

	fn load_from(&self, path: &Path) -> Result<PluginInstance> {
		let setup = &self.setup;
		let mut loader = aexlo::Host::get()
			.loader(path)
			.observer(self.recorder.observer(), setup.level)
			.strict(setup.strict);
		loader = loader.input(setup.input.clone());
		if let Some((w, h)) = setup.size {
			loader = loader.output_size(w, h);
		}
		let mut instance = loader
			.load()
			.with_context(|| format!("loading plugin {}", setup.path.display()))?;
		instance.set_parallel_iterate(!setup.serial);
		instance.set_text_layer(setup.text.clone());
		instance.set_project(setup.project.clone());
		instance.set_mask_paths(setup.masks.clone());
		Ok(instance)
	}

	fn instance(&mut self) -> Result<&mut PluginInstance> {
		self.instance
			.as_mut()
			.context("the plugin is not loaded (try `reload`)")
	}

	/// Run one plugin command (see [`Command::is_engine`]).
	pub fn run(&mut self, line: &str, command: &Command) -> Result<Outcome> {
		self.recorder.step(line);
		let started = Instant::now();
		let result = self.execute(command);
		let elapsed = started.elapsed();
		flush_c_stdio();
		self.collect_findings();
		result.map(|message| Outcome { message, elapsed })
	}

	fn execute(&mut self, command: &Command) -> Result<String> {
		match command {
			Command::About => {
				let text = self.instance()?.about().context("PF_Cmd_ABOUT failed")?;
				Ok(text.trim().replace('\r', "\n"))
			}
			Command::Render(path) => {
				let fx = self.instance()?;
				fx.render_with(*path)
					.with_context(|| format!("{path:?} render failed"))?;
				let (w, h) = fx.output_size();
				Ok(format!("rendered {w}x{h} ({path:?})"))
			}
			Command::Set { .. } | Command::SetWords(_) => {
				let fx = self.instance()?;
				let (index, key, value) = match command {
					Command::Set { key, value } => (crate::param::resolve_key(fx, key)?, key.clone(), value.clone()),
					Command::SetWords(words) => split_words(fx, words)?,
					_ => unreachable!(),
				};
				settable(fx, index)?;
				let parsed = match popup_choice(fx, index, &value) {
					Some(choice) => ParamValue::Popup(choice),
					None => crate::param::parse_value(fx, index, &value)?,
				};
				fx.set_param(index, parsed.clone())
					.with_context(|| format!("setting parameter {key}"))?;
				let mut message = format!("#{index} = {}", describe_value(&parsed));
				if let Some(warning) = out_of_range(fx, index, &parsed) {
					message += &format!("  ({warning})");
				}
				Ok(message)
			}
			Command::Key { key, frame, value } => {
				let fx = self.instance()?;
				let index = crate::param::resolve_key(fx, key)?;
				settable(fx, index)?;
				let parsed = match popup_choice(fx, index, value) {
					Some(choice) => ParamValue::Popup(choice),
					None => crate::param::parse_value(fx, index, value)?,
				};
				let now = fx.time();
				let at = frame
					.checked_mul(now.step())
					.and_then(|at| TimePoint::new(at, now.scale()))
					.context("that frame is out of range")?;
				let mut keys: Vec<Keyframe<ParamValue>> =
					fx.param_track(index).map_or(Vec::new(), |t| t.keys().to_vec());
				keys.retain(|existing| existing.time != at);
				keys.push(Keyframe::linear(at, parsed.clone()));
				let count = keys.len();
				let track = Track::keyed(keys).context("no keyframes")?;
				fx.set_param_track(index, track)
					.with_context(|| format!("keyframing parameter {key}"))?;
				Ok(format!(
					"#{index} = {} at frame {frame} ({count} key(s))",
					describe_value(&parsed)
				))
			}
			Command::Mask(outline) => {
				let fx = self.instance()?;
				let masks = outline.map_or(Vec::new(), |outline| outline_paths(outline, fx.input_size()));
				fx.set_mask_paths(masks.clone());
				let message = format!("{} mask(s)", masks.len());
				self.setup.masks = masks;
				Ok(message)
			}
			Command::Changed(key) => {
				let fx = self.instance()?;
				let index = crate::param::resolve_key(fx, key)?;
				existing(fx, index)?;
				fx.user_changed_param(index)
					.context("PF_Cmd_USER_CHANGED_PARAM failed")?;
				Ok(format!("told the plugin #{index} changed"))
			}
			Command::Press(key) => {
				let fx = self.instance()?;
				let index = resolve_button(fx, key)?;
				if fx.param_hidden(index) || fx.param_disabled(index) {
					let state = if fx.param_hidden(index) { "hidden" } else { "disabled" };
					bail!("the plugin {state} button #{index}; `changed {index}` sends it anyway");
				}
				let before = fx.param_values();
				fx.user_changed_param(index)
					.context("PF_Cmd_USER_CHANGED_PARAM failed")?;
				let changed: Vec<String> = fx
					.param_values()
					.into_iter()
					.filter(|(i, value)| !before.iter().any(|(j, old)| j == i && old == value))
					.map(|(i, value)| format!("#{i} = {}", describe_value(&value)))
					.collect();
				let text = button_text(fx, index);
				Ok(if changed.is_empty() {
					format!("pressed #{index} {text}; no values changed")
				} else {
					format!("pressed #{index} {text}; {}", changed.join(", "))
				})
			}
			Command::UpdateUi => {
				self.instance()?
					.update_params_ui()
					.context("PF_Cmd_UPDATE_PARAMS_UI failed")?;
				Ok("params UI updated".into())
			}
			Command::Dialog => {
				let text = self.instance()?.do_dialog().context("PF_Cmd_DO_DIALOG failed")?;
				Ok(if text.trim().is_empty() {
					"dialog returned".into()
				} else {
					text.trim().to_string()
				})
			}
			Command::Time { frame, fps } => {
				self.instance()?.set_time(Time::frame(*frame, *fps));
				Ok(format!("time = frame {frame} at {fps} fps"))
			}
			Command::Size(w, h) => {
				self.instance()?.set_render_size(*w, *h);
				self.setup.size = Some((*w, *h));
				Ok(format!("output size = {w}x{h}"))
			}
			Command::Input(source) => {
				let fx = self.instance()?;
				let depth = fx.input_depth();
				let (w, h) = fx.input_size();
				let (layer, text, project) = match source {
					InputSource::Shape(outline) => {
						let (layer, project) = shape_layer(*outline, depth, (w, h))?;
						(layer, None, Some(project))
					}
					_ => {
						let (layer, text) = build_input(source, depth, (w, h), self.setup.font.as_deref())?;
						(layer, text, None)
					}
				};
				let (w, h) = (layer.width(), layer.height());
				let fx = self.instance()?;
				set_input(fx, layer.clone());
				fx.set_text_layer(text.clone());
				fx.set_project(project.clone());
				let kind = match (&text, &project) {
					(Some(text), _) => format!(" text layer, {} outline(s)", text.outlines().len()),
					(None, Some(project)) => format!(
						" shape layer, {} path(s)",
						project.effect_layer().outlines(TimePoint::ZERO).len()
					),
					(None, None) => String::new(),
				};
				self.setup.input = layer;
				self.setup.text = text;
				self.setup.project = project;
				Ok(format!("input = {w}x{h} {}-bit{kind}", depth.bits()))
			}
			Command::Depth(kind) => {
				if !self.instance()?.supports_depth(*kind) {
					bail!("the plugin does not declare {}-bit support", kind.bits());
				}
				let layer = self.setup.input.convert(*kind);
				set_input(self.instance()?, layer.clone());
				self.setup.input = layer;
				Ok(format!("project depth = {} bpc", kind.bits()))
			}
			Command::Reload => {
				let values = self
					.instance
					.as_ref()
					.map(PluginInstance::param_values)
					.unwrap_or_default();
				let tracks: Vec<(usize, Track<ParamValue>)> = self.instance.as_ref().map_or(Vec::new(), |fx| {
					(1..fx.param_count())
						.filter_map(|index| Some((index, fx.param_track(index)?.clone())))
						.collect()
				});
				self.unload();
				let rebuilt = match &self.setup.manifest {
					Some(manifest) => {
						self.setup.path = crate::watch::build_cdylib(manifest)?;
						" (rebuilt)"
					}
					None => "",
				};
				self.open()?;
				// Carry the values over where the parameter is still there and
				// of the same kind.
				let fx = self.instance()?;
				let restored = values
					.into_iter()
					.filter(|(index, value)| fx.set_param(*index, value.clone()).is_ok())
					.count();
				for (index, track) in tracks {
					// As with the values: where it still fits.
					let _ = fx.set_param_track(index, track);
				}
				Ok(format!("reloaded{rebuilt}, {restored} parameter value(s) kept"))
			}
			Command::Level(level) => {
				self.setup.level = *level;
				let observer = self.recorder.observer();
				self.instance()?.set_observer(Some(observer), *level);
				Ok(format!("trace level = {level:?}"))
			}
			Command::Strict(strict) => {
				self.setup.strict = *strict;
				self.instance()?.set_strict(*strict);
				Ok(format!("strict = {}", describe_strict(*strict)))
			}
			Command::Serial(serial) => {
				self.setup.serial = *serial;
				self.instance()?.set_parallel_iterate(!serial);
				Ok(format!("serial iterate = {}", if *serial { "on" } else { "off" }))
			}
			Command::Save(path) => {
				let fx = self.instance()?;
				save_output(fx, path)?;
				Ok(format!("saved {}", path.display()))
			}
			Command::Mem(_)
			| Command::Focus(_)
			| Command::Find(_)
			| Command::Clear
			| Command::Report
			| Command::Help
			| Command::Quit => Ok(String::new()),
		}
	}

	fn collect_findings(&mut self) {
		if let Some(fx) = self.instance.as_mut() {
			let report = fx.strict_report();
			self.findings.extend(describe_report(&report));
		}
	}

	/// Copy part of a live allocation (see [`PluginInstance::read_allocation`]).
	pub fn read(&self, address: usize, offset: usize, len: usize) -> Option<Vec<u8>> {
		self.instance.as_ref()?.read_allocation(address, offset, len)
	}

	pub fn snapshot(&self) -> Option<Snapshot> {
		let fx = self.instance.as_ref()?;
		let params = (1..fx.param_count()).map(|index| param_row(fx, index)).collect();
		let (iw, ih) = fx.input_size();
		let (ow, oh) = fx.output_size();
		let output = fx.output();
		Some(Snapshot {
			path: self.setup.path.clone(),
			params,
			state: fx.state(),
			out_flags: fx.out_flags(),
			out_flags2: fx.out_flags2(),
			smart: fx.supports_smart_render(),
			gpu: fx.supports_gpu(),
			custom_ui: fx.has_custom_ui(),
			options_button: fx.options_button_name().map(str::to_string),
			pixel_formats: fx.supported_pixel_formats().iter().map(|f| format!("{f:?}")).collect(),
			input: (iw, ih, fx.input_depth()),
			output: (ow, oh, fx.output_depth()),
			time: fx.time(),
			parallel_iterate: fx.parallel_iterate(),
			strict: fx.strict(),
			level: self.setup.level,
			allocations: fx.allocations(),
			findings: self.findings.clone(),
			unwritten: fx.strict().poison_output.then(|| aexlo::unwritten_pixels(output)),
			mean: mean(output),
			preview: preview(output),
		})
	}
}

impl Engine {
	/// Every strict-mode finding so far, oldest first.
	pub fn findings(&self) -> &[String] {
		&self.findings
	}

	/// Tear the plugin down now, recorded as an `unload` step: the
	/// `GLOBAL_SETDOWN` is traced and the handles still held then are judged
	/// leaks.
	pub fn shutdown(&mut self) {
		if self.instance.is_some() {
			self.recorder.step("unload");
		}
		self.unload();
	}

	/// Tear the plugin down while still observed, so SETDOWN is traced.
	fn unload(&mut self) {
		if let Some(fx) = self.instance.take() {
			self.findings.extend(describe_report(&fx.finish()));
			flush_c_stdio();
		}
		if let Some(staged) = self.staged.take() {
			let _ = std::fs::remove_file(staged);
		}
	}
}

impl Drop for Engine {
	fn drop(&mut self) {
		self.shutdown();
	}
}

/// Give C `stdio` buffers to their file descriptors, so a plugin's `printf`
/// shows up after the command that printed it rather than at exit.
fn flush_c_stdio() {
	#[cfg(unix)]
	// SAFETY: `fflush(NULL)` flushes every open output stream.
	unsafe {
		libc::fflush(std::ptr::null_mut());
	}
}

fn set_input(fx: &mut PluginInstance, layer: AnyLayer) {
	match layer {
		AnyLayer::U8(l) => fx.set_input_layer(l),
		AnyLayer::U16(l) => fx.set_input_layer(l),
		AnyLayer::F32(l) => fx.set_input_layer(l),
	}
}

/// The input layer `source` describes, and its text when it is a text layer.
pub fn build_input(
	source: &InputSource,
	depth: PixelDepthKind,
	current: (u32, u32),
	font: Option<&Path>,
) -> Result<(AnyLayer, Option<TextLayer>)> {
	if let InputSource::Text(text) = source {
		let (w, h) = current;
		let text = aexlo_harness::input::fit_text(text, font, w, h).context("setting the text")?;
		return Ok((text.rasterize(depth, w, h, [1.0; 4]), Some(text)));
	}
	let layer = match source {
		InputSource::Text(_) => unreachable!("handled above"),
		InputSource::Shape(_) => bail!("a shape layer needs a plugin loaded (`input shape`)"),
		InputSource::Png(path) => load_png(path)?.convert(depth),
		InputSource::Fill(rgba, size) => {
			let (w, h) = size.unwrap_or(current);
			AnyLayer::filled(depth, w, h, *rgba)
		}
		InputSource::Gradient(size) => {
			let (w, h) = size.unwrap_or(current);
			let mut layer = AnyLayer::filled(depth, w, h, [0.0; 4]);
			let values = (0..h).flat_map(|y| {
				(0..w).map(move |x| {
					let u = x as f32 / (w.max(2) - 1) as f32;
					let v = y as f32 / (h.max(2) - 1) as f32;
					[u, v, 1.0 - u, 1.0]
				})
			});
			layer.set_from_unit_rgba(values);
			layer
		}
	};
	Ok((layer, None))
}

/// A `size` shape layer showing `outline`, and the project that has it.
fn shape_layer(outline: Outline, depth: PixelDepthKind, (w, h): (u32, u32)) -> Result<(AnyLayer, Project)> {
	let comp = Comp::new("Comp 1", w, h, TimePoint::whole_seconds(10));
	let mut items: Vec<ShapeItem> = outline_paths(outline, (w, h))
		.into_iter()
		.enumerate()
		.map(|(i, path)| ShapeItem::Path(ShapePath::new(format!("Path {}", i + 1), path)))
		.collect();
	items.push(ShapeItem::Fill(ShapeFill::new([1.0; 4])));
	let group = ShapeItem::Group(ShapeGroup::new("Shape 1", items));
	let shape = ProjectLayer::new(1, "Shape Layer 1", LayerContent::Shape(vec![group]), &comp);
	let layer = shape
		.rasterize(depth, TimePoint::ZERO, [1.0; 4])
		.context("drawing the shape")?;
	Ok((layer, Project::new(comp, vec![shape], 1)?))
}

/// `outline` in a `size` layer. It sits left of and above the middle, so
/// paths read in the wrong space show.
fn outline_paths(outline: Outline, (w, h): (u32, u32)) -> Vec<MaskPath> {
	let (w, h) = (f64::from(w), f64::from(h));
	let (cx, cy) = (w * 0.4, h * 0.45);
	let r = w.min(h) * 0.3;
	let ellipse = |r: f64| {
		// Circle-to-cubic handle length.
		let t = r * 0.552_284_749_8;
		let at = |x: f64, y: f64, tx: f64, ty: f64| MaskVertex {
			x: cx + x,
			y: cy + y,
			tan_in_x: -tx,
			tan_in_y: -ty,
			tan_out_x: tx,
			tan_out_y: ty,
		};
		vec![
			at(0.0, -r, t, 0.0),
			at(r, 0.0, 0.0, t),
			at(0.0, r, -t, 0.0),
			at(-r, 0.0, 0.0, -t),
		]
	};
	let corner = |x: f64, y: f64| MaskVertex::corner(cx + x * r, cy + y * r);
	let paths = match outline {
		Outline::Rect => vec![vec![
			corner(-1.0, -0.7),
			corner(1.0, -0.7),
			corner(1.0, 0.7),
			corner(-1.0, 0.7),
		]],
		Outline::Ellipse => vec![ellipse(r)],
		Outline::Triangle => vec![vec![corner(0.0, -1.0), corner(1.0, 0.8), corner(-1.0, 0.8)]],
		Outline::Ring => {
			let inner = ellipse(r * 0.5).into_iter().rev().map(|v| MaskVertex {
				tan_in_x: v.tan_out_x,
				tan_in_y: v.tan_out_y,
				tan_out_x: v.tan_in_x,
				tan_out_y: v.tan_in_y,
				..v
			});
			vec![ellipse(r), inner.collect()]
		}
	};
	paths
		.into_iter()
		.zip(1..)
		.map(|(vertices, id)| MaskPath {
			id,
			name: format!("Mask {id}"),
			vertices,
			closed: true,
			..Default::default()
		})
		.collect()
}

pub fn load_png(path: &Path) -> Result<AnyLayer> {
	aexlo_harness::frame::load_image(path).map_err(anyhow::Error::msg)
}

fn save_output(fx: &PluginInstance, path: &Path) -> Result<()> {
	let extension = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
	let exr = match extension.as_deref() {
		Some("png") => false,
		Some("exr") => true,
		_ => bail!("save writes .png or .exr, not {}", path.display()),
	};
	let depth = match (exr, fx.output_depth()) {
		(true, _) => PixelDepthKind::F32,
		(false, PixelDepthKind::U8) => PixelDepthKind::U8,
		// PNG holds at most 16 bits per channel.
		(false, _) => PixelDepthKind::U16,
	};
	aexlo_harness::Frame::new(fx.output().clone())
		.convert(depth)
		.save(path)
		.map_err(anyhow::Error::msg)
}

/// A popup's 1-based choice named `value` (ignoring case).
/// Split `set Noise variation 25` into the parameter and its value: the
/// longest run of leading words that names a parameter is the name
/// (`Emitter Type 2` sets `Emitter Type`, not a group named `Emitter`).
fn split_words(fx: &PluginInstance, text: &str) -> Result<(usize, String, String)> {
	let words: Vec<&str> = text.split_whitespace().collect();
	// A key names something when it is an index or some parameter is called
	// that (ignoring case); an ambiguous name still ends the search, so its
	// error says which parameters it matches.
	let names = |key: &str| {
		key.trim_start_matches('#').parse::<usize>().is_ok()
			|| (1..fx.param_count()).any(|i| fx.param_name(i).is_some_and(|n| n.trim().eq_ignore_ascii_case(key)))
	};
	for taken in (1..words.len()).rev() {
		let key = words[..taken].join(" ");
		if names(&key) {
			let index = crate::param::resolve_key(fx, &key)?;
			return Ok((index, key, words[taken..].join(" ")));
		}
	}
	// Nothing matched: the error for the first word lists the names.
	let first = words.first().copied().unwrap_or_default();
	Err(crate::param::resolve_key(fx, first)
		.err()
		.unwrap_or_else(|| anyhow::anyhow!("set <param> <value>")))
}

/// `index` is one of the plugin's parameters (not the input layer, #0).
fn existing(fx: &PluginInstance, index: usize) -> Result<()> {
	let last = fx.param_count().saturating_sub(1);
	if index == 0 || index > last {
		bail!("no parameter #{index} (the plugin has #1..#{last}; #0 is the input layer)");
	}
	Ok(())
}

/// The button `key` names: an index, `#index` or name as for `set`, or else
/// the text on the button, ignoring case. Plugins often leave a button's name
/// empty and put what it does on the button.
fn resolve_button(fx: &PluginInstance, key: &str) -> Result<usize> {
	let is_button = |i: usize| fx.param_kind(i) == Some(ParamKind::Button);
	let by_name = crate::param::resolve_key(fx, key);
	if let Ok(index) = by_name
		&& is_button(index)
	{
		return Ok(index);
	}
	let labelled: Vec<usize> = (1..fx.param_count())
		.filter(|&i| {
			fx.param_button_label(i)
				.is_some_and(|label| label.eq_ignore_ascii_case(key.trim()))
		})
		.collect();
	match (labelled.as_slice(), by_name) {
		(&[index], _) => Ok(index),
		([], Ok(index)) => {
			existing(fx, index)?;
			let kind = fx.param_kind(index).map_or("unknown".into(), |k| format!("{k:?}"));
			let name = fx.param_name(index).unwrap_or_default();
			bail!("parameter #{index} '{name}' is a {kind}, not a button")
		}
		([], Err(err)) => Err(err.context(format!("no button is named or labelled '{}'", key.trim()))),
		(several, _) => {
			let list: Vec<String> = several.iter().map(|i| format!("#{i}")).collect();
			bail!(
				"{} buttons say '{}': {}; press one by index",
				several.len(),
				key.trim(),
				list.join(", ")
			)
		}
	}
}

/// How a message names a button: the text on it, or else its name.
fn button_text(fx: &PluginInstance, index: usize) -> String {
	match fx.param_button_label(index) {
		Some(label) => format!("\"{label}\""),
		None => format!("'{}'", fx.param_name(index).unwrap_or_default().trim()),
	}
}

/// `index` exists and holds a value `set` can change.
fn settable(fx: &PluginInstance, index: usize) -> Result<()> {
	existing(fx, index)?;
	if fx.get_param(index).is_none() {
		let kind = fx.param_kind(index).map_or("unknown".into(), |k| format!("{k:?}"));
		let name = fx.param_name(index).unwrap_or_default();
		bail!("parameter #{index} '{name}' is a {kind} and has no value to set");
	}
	Ok(())
}

/// A note when `value` lies outside what the plugin declared: its slider
/// range, or its popup's choices. The value is set anyway, since a plugin
/// may well be tested with one.
fn out_of_range(fx: &PluginInstance, index: usize, value: &ParamValue) -> Option<String> {
	let number = match *value {
		ParamValue::Float(v) => v,
		ParamValue::Fixed(v) => f64::from(v),
		ParamValue::Slider(v) => f64::from(v),
		ParamValue::Popup(v) => {
			let count = fx.param_choices(index)?.len();
			return (v < 1 || v as usize > count).then(|| format!("outside the popup's choices 1..{count}"));
		}
		_ => return None,
	};
	let (min, max) = fx.param_slider_range(index)?;
	(number < min || number > max).then(|| format!("outside the slider range {min}..{max}"))
}

fn popup_choice(fx: &PluginInstance, index: usize, value: &str) -> Option<i32> {
	let choices = fx.param_choices(index)?;
	let position = choices
		.iter()
		.position(|c| c.trim().eq_ignore_ascii_case(value.trim()))?;
	i32::try_from(position + 1).ok()
}

fn param_row(fx: &PluginInstance, index: usize) -> ParamRow {
	let kind = fx.param_kind(index).map(|k| format!("{k:?}")).unwrap_or_default();
	let value = fx.get_param(index).map(|v| describe_value(&v)).unwrap_or_default();
	let mut detail = Vec::new();
	if let Some((min, max)) = fx.param_slider_range(index) {
		detail.push(format!("{min}..{max}"));
	}
	if let Some(choices) = fx.param_choices(index) {
		let list: Vec<String> = choices
			.iter()
			.enumerate()
			.map(|(i, c)| format!("{}={c}", i + 1))
			.collect();
		detail.push(list.join(" "));
	}
	if let Some(label) = fx.param_button_label(index) {
		detail.push(format!("button \"{label}\""));
	}
	let flags = fx.param_ui_flags(index).unwrap_or_default();
	if flags != aexlo::ParamUiFlags::default() {
		detail.push(format!("{flags:?}"));
	}
	if fx.param_has_custom_ui(index) {
		detail.push("custom UI".into());
	}
	ParamRow {
		index,
		name: fx.param_name(index).unwrap_or_default(),
		kind,
		value,
		detail: detail.join("  "),
		hidden: fx.param_hidden(index),
		disabled: fx.param_disabled(index),
	}
}

pub fn describe_value(value: &ParamValue) -> String {
	match value {
		ParamValue::Float(v) => format!("{v}"),
		ParamValue::Fixed(v) => format!("{v}"),
		ParamValue::Slider(v) => format!("{v}"),
		ParamValue::Popup(v) => format!("{v}"),
		ParamValue::Angle(v) => format!("{v}°"),
		ParamValue::Checkbox(v) => format!("{v}"),
		ParamValue::Point { x, y } => format!("{x}, {y}"),
		ParamValue::Color {
			red,
			green,
			blue,
			alpha,
		} => format!("{red}, {green}, {blue}, {alpha}"),
		ParamValue::Path(id) => format!("mask {id}"),
		ParamValue::Point3D { x, y, z } => format!("{x}, {y}, {z}"),
		other => format!("{other:?}"),
	}
}

pub fn describe_strict(strict: Strict) -> String {
	let on: Vec<&str> = [
		(strict.poison_output, "poison"),
		(strict.guard_bands, "guard"),
		(strict.track_allocations, "alloc"),
		(strict.track_checkouts, "checkout"),
	]
	.into_iter()
	.filter_map(|(on, name)| on.then_some(name))
	.collect();
	if on.is_empty() { "off".into() } else { on.join(",") }
}

fn describe_report(report: &aexlo::StrictReport) -> Vec<String> {
	let mut lines = Vec::new();
	for v in &report.guard_violations {
		lines.push(format!(
			"guard band {} of {} overwritten ({} B)",
			v.band, v.world, v.bytes
		));
	}
	for leak in &report.leaks {
		lines.push(format!(
			"leak: {:?} {:#x} ({} B) from {} still held after {}",
			leak.kind, leak.address, leak.bytes, leak.allocated_in, leak.scope
		));
	}
	for v in &report.checkout_violations {
		lines.push(format!("checkout: {}", v.message));
	}
	lines
}

fn mean(layer: &AnyLayer) -> [f32; 4] {
	let count = layer.len();
	if count == 0 {
		return [0.0; 4];
	}
	let mut sum = [0.0f64; 4];
	for i in 0..count {
		if let Some(p) = layer.unit_rgba(i) {
			for (s, c) in sum.iter_mut().zip(p) {
				// The 32 bpc poison is a NaN; leave it out of the mean.
				if c.is_finite() {
					*s += f64::from(c);
				}
			}
		}
	}
	sum.map(|s| (s / count as f64) as f32)
}

/// Nearest-neighbour downsample of `layer` to fit [`PREVIEW_MAX`].
fn preview(layer: &AnyLayer) -> Preview {
	let (w, h) = (layer.width(), layer.height());
	if w == 0 || h == 0 {
		return Preview::default();
	}
	let scale = (PREVIEW_MAX as f32 / w.max(h) as f32).min(1.0);
	let pw = ((w as f32 * scale).round() as u32).max(1);
	let ph = ((h as f32 * scale).round() as u32).max(1);
	let mut rgba = Vec::with_capacity((pw * ph * 4) as usize);
	for y in 0..ph {
		let sy = (y as u64 * h as u64 / ph as u64) as u32;
		for x in 0..pw {
			let sx = (x as u64 * w as u64 / pw as u64) as u32;
			let p = layer.unit_rgba((sy * w + sx) as usize).unwrap_or([0.0; 4]).map(|c| {
				if c.is_finite() {
					(c.clamp(0.0, 1.0) * 255.0).round() as u8
				} else {
					255
				}
			});
			rgba.extend_from_slice(&p);
		}
	}
	Preview {
		width: pw,
		height: ph,
		rgba,
	}
}
