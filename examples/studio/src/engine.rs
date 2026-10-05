//! The render engine: a thread that owns every plugin instance and turns
//! project snapshots into frames.
//!
//! A layer renders as source → masks → effects → transform, like After
//! Effects. Layers linked to an effect's layer parameter render the same way
//! (without their transform) on demand, so an effect can read any other layer,
//! including hidden ones; a link back to the layer being rendered reads its
//! source before effects, and cycles are cut.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use aexlo::{Depth8, ParamKind, PluginInstance};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint};

use crate::model::{BlendMode, Effect, Id, Layer, LayerKind, PValue, Project};
use crate::plugins;
use crate::raster::{Fonts, Image, ImageCache, mask_coverage, render_shapes, render_text};

/// One plugin parameter, as declared at `PF_Cmd_PARAMS_SETUP`.
#[derive(Debug, Clone)]
pub struct ParamDesc {
	pub index: usize,
	pub name: String,
	pub kind: ParamKind,
	/// `None` for kinds without a [`PValue`] (layers, groups, buttons, ...).
	pub default: Option<PValue>,
	pub range: Option<(f64, f64)>,
	pub choices: Option<Vec<String>>,
}

/// What a loaded plugin told us about itself.
#[derive(Debug)]
pub struct PluginInfo {
	pub about: String,
	pub smart_render: bool,
	pub gpu: bool,
	pub params: Vec<ParamDesc>,
	/// Input layer size the defaults were declared against.
	pub load_size: (u32, u32),
}

impl PluginInfo {
	/// The default of parameter `index` on a `size` layer. Point defaults are
	/// relative to the layer (After Effects declares them as percentages).
	pub fn default_value(&self, index: usize, size: (u32, u32)) -> Option<PValue> {
		let default = self.params.get(index)?.default.clone()?;
		let (sx, sy) = (
			size.0 as f64 / self.load_size.0.max(1) as f64,
			size.1 as f64 / self.load_size.1.max(1) as f64,
		);
		Some(match default {
			PValue::Point([x, y]) => PValue::Point([(x as f64 * sx) as f32, (y as f64 * sy) as f32]),
			PValue::Point3D([x, y, z]) => PValue::Point3D([x * sx, y * sy, z * sy]),
			other => other,
		})
	}
}

pub enum Request {
	Render {
		project: Arc<Project>,
		frame: i32,
		generation: u64,
	},
	/// Press a button parameter (`PF_Cmd_USER_CHANGED_PARAM`), then re-render.
	Button {
		project: Arc<Project>,
		frame: i32,
		generation: u64,
		effect: Id,
		index: usize,
	},
	Export(ExportJob),
}

pub enum ExportKind {
	/// One frame (`start`) to a PNG file.
	Png(PathBuf),
	/// `frame_00000.png`, ... into a directory.
	Sequence(PathBuf),
	/// An H.264 movie, encoded by `ffmpeg` from a temporary sequence.
	Video(PathBuf),
}

pub struct ExportJob {
	pub project: Arc<Project>,
	pub start: i32,
	pub end: i32,
	pub kind: ExportKind,
}

pub struct FrameResult {
	pub generation: u64,
	pub frame: i32,
	pub image: Image,
	pub elapsed: Duration,
	/// Human-readable render problems (failed effects, missing files, ...).
	pub problems: Vec<String>,
	pub effect_times: HashMap<Id, Duration>,
}

pub enum Event {
	Frame(Box<FrameResult>),
	/// An effect's plugin was loaded (or failed to).
	Info {
		effect: Id,
		info: Result<Arc<PluginInfo>, String>,
	},
	/// The plugin changed which of its parameters are hidden or disabled.
	ParamUi {
		effect: Id,
		hidden: Vec<bool>,
		disabled: Vec<bool>,
	},
	/// The plugin changed parameter values itself (from a button press).
	ParamsChanged {
		effect: Id,
		frame: i32,
		values: Vec<(usize, PValue)>,
	},
	ExportProgress {
		done: i32,
		total: i32,
	},
	ExportFinished(Result<String, String>),
}

/// The UI's handle on the engine thread.
pub struct Engine {
	tx: Sender<Request>,
	pub events: Receiver<Event>,
	pub cancel_export: Arc<AtomicBool>,
}

impl Engine {
	/// Start the engine; `repaint` wakes the UI when an event is ready.
	pub fn spawn(repaint: impl Fn() + Send + Sync + 'static) -> Self {
		let (tx, requests) = channel();
		let (events_tx, events) = channel();
		let cancel_export = Arc::new(AtomicBool::new(false));
		let cancel = cancel_export.clone();
		std::thread::Builder::new()
			.name("aexlo-render".into())
			.spawn(move || {
				Worker {
					slots: HashMap::new(),
					sources: HashMap::new(),
					images: ImageCache::default(),
					fonts: Fonts::new(),
					notify: Notify {
						tx: events_tx,
						repaint: Box::new(repaint),
					},
					cancel,
				}
				.run(requests)
			})
			.expect("spawn render thread");
		Self {
			tx,
			events,
			cancel_export,
		}
	}

	pub fn send(&self, request: Request) {
		if let Request::Export(_) = request {
			self.cancel_export.store(false, Ordering::Relaxed);
		}
		let _ = self.tx.send(request);
	}
}

struct Notify {
	tx: Sender<Event>,
	repaint: Box<dyn Fn() + Send + Sync>,
}

impl Notify {
	fn send(&self, event: Event) {
		let _ = self.tx.send(event);
		(self.repaint)();
	}
}

/// A loaded effect instance.
struct Slot {
	plugin: String,
	instance: Result<PluginInstance, String>,
	info: Option<Arc<PluginInfo>>,
	/// Parameter values last written to the instance.
	applied: HashMap<usize, PValue>,
	/// Input size the applied values were written for.
	size: (u32, u32),
	linked: BTreeSet<usize>,
	hidden: Vec<bool>,
	disabled: Vec<bool>,
}

impl Slot {
	fn load(plugin: &str) -> Self {
		let path = plugins::resolve(plugin);
		log::info!("loading {plugin} from {}", path.display());
		let instance = aexlo::Host::get().try_load(&path).map_err(|e| e.to_string());
		let info = instance.as_ref().ok().map(|fx| Arc::new(describe(fx)));
		let mut slot = Self {
			plugin: plugin.to_string(),
			instance,
			info,
			applied: HashMap::new(),
			size: (0, 0),
			linked: BTreeSet::new(),
			hidden: Vec::new(),
			disabled: Vec::new(),
		};
		if let Ok(fx) = &mut slot.instance {
			match fx.about() {
				Ok(about) => {
					if let Some(info) = &mut slot.info {
						Arc::get_mut(info).expect("unshared").about = about.trim().to_string();
					}
				}
				Err(e) => log::debug!("{plugin}: PF_Cmd_ABOUT failed: {e}"),
			}
		}
		slot
	}

	/// Re-read the hidden/disabled flags; whether they changed.
	fn refresh_ui_flags(&mut self) -> bool {
		let Ok(fx) = &self.instance else { return false };
		let hidden: Vec<bool> = (0..fx.param_count()).map(|i| fx.param_hidden(i)).collect();
		let disabled: Vec<bool> = (0..fx.param_count()).map(|i| fx.param_disabled(i)).collect();
		let changed = hidden != self.hidden || disabled != self.disabled;
		self.hidden = hidden;
		self.disabled = disabled;
		changed
	}
}

fn describe(fx: &PluginInstance) -> PluginInfo {
	PluginInfo {
		about: String::new(),
		smart_render: fx.supports_smart_render(),
		gpu: fx.supports_gpu(),
		load_size: fx.input_size(),
		params: (0..fx.param_count())
			.map(|index| ParamDesc {
				index,
				name: fx.param_name(index).unwrap_or_default(),
				kind: fx.param_kind(index).unwrap_or(ParamKind::Other(-1)),
				default: fx.get_param(index).map(PValue::from),
				range: fx.param_slider_range(index),
				choices: fx.param_choices(index),
			})
			.collect(),
	}
}

struct CachedSource {
	key: u64,
	image: Image,
	pixmap: Option<Arc<Pixmap>>,
}

/// What a layer's source depends on: its contents and masks, and the frame
/// when any of them is keyframed.
fn source_key(layer: &Layer, comp: &crate::model::Comp, frame: i32) -> u64 {
	use std::hash::{Hash, Hasher};
	let json = serde_json::to_string(&(&layer.kind, &layer.masks)).unwrap_or_default();
	let mut h = std::collections::hash_map::DefaultHasher::new();
	json.hash(&mut h);
	(comp.width, comp.height).hash(&mut h);
	// Anims only serialize their keys when they have some.
	if json.contains("\"keys\"") {
		frame.hash(&mut h);
	}
	h.finish()
}

/// Per-frame render state.
struct Frame<'p> {
	project: &'p Project,
	frame: i32,
	/// Layers after masks and effects.
	done: HashMap<Id, Option<Image>>,
	/// Layers after masks, before effects.
	sources: HashMap<Id, Image>,
	visiting: HashSet<Id>,
	problems: Vec<String>,
	times: HashMap<Id, Duration>,
}

struct Worker {
	slots: HashMap<Id, Slot>,
	/// Layer sources (and their premultiplied form, for layers without
	/// effects) from earlier frames, reused while nothing they depend on changes.
	sources: HashMap<Id, CachedSource>,
	images: ImageCache,
	fonts: Fonts,
	notify: Notify,
	cancel: Arc<AtomicBool>,
}

impl Worker {
	fn run(mut self, requests: Receiver<Request>) {
		while let Ok(first) = requests.recv() {
			let mut queue = vec![first];
			queue.extend(requests.try_iter());
			// Only the newest render matters; everything else runs in order.
			let newest = queue.iter().rposition(|r| matches!(r, Request::Render { .. }));
			for (i, request) in queue.into_iter().enumerate() {
				match request {
					Request::Render { .. } if Some(i) != newest => {}
					Request::Render {
						project,
						frame,
						generation,
					} => {
						let result = self.render(&project, frame, generation);
						self.notify.send(Event::Frame(Box::new(result)));
					}
					Request::Button {
						project,
						frame,
						generation,
						effect,
						index,
					} => {
						self.render(&project, frame, generation);
						self.press_button(effect, index, frame);
						let result = self.render(&project, frame, generation);
						self.notify.send(Event::Frame(Box::new(result)));
					}
					Request::Export(job) => {
						let result = self.export(job);
						self.notify.send(Event::ExportFinished(result));
					}
				}
			}
		}
	}

	/// Load every effect in the project (so the UI gets their parameters even
	/// when their layer isn't showing) and drop the ones that are gone.
	fn sync_slots(&mut self, project: &Project) {
		let mut live = HashSet::new();
		for effect in project.layers.iter().flat_map(|l| &l.effects) {
			live.insert(effect.id);
			if self.slots.get(&effect.id).is_some_and(|s| s.plugin == effect.plugin) {
				continue;
			}
			let mut slot = Slot::load(&effect.plugin);
			slot.refresh_ui_flags();
			self.notify.send(Event::Info {
				effect: effect.id,
				info: slot
					.info
					.clone()
					.ok_or_else(|| slot.instance.as_ref().err().cloned().unwrap_or_default()),
			});
			self.notify.send(Event::ParamUi {
				effect: effect.id,
				hidden: slot.hidden.clone(),
				disabled: slot.disabled.clone(),
			});
			self.slots.insert(effect.id, slot);
		}
		self.slots.retain(|id, _| live.contains(id));
		self.sources.retain(|id, _| project.layer(*id).is_some());
	}

	fn render(&mut self, project: &Project, frame: i32, generation: u64) -> FrameResult {
		let started = Instant::now();
		self.sync_slots(project);
		let comp = &project.comp;
		let mut canvas = Pixmap::new(comp.width.max(1), comp.height.max(1)).expect("non-empty comp");
		let [r, g, b, a] = comp.background.map(|c| c.clamp(0.0, 1.0));
		canvas.fill(tiny_skia::Color::from_rgba(r, g, b, a).unwrap_or(tiny_skia::Color::BLACK));

		let mut ctx = Frame {
			project,
			frame,
			done: HashMap::new(),
			sources: HashMap::new(),
			visiting: HashSet::new(),
			problems: Vec::new(),
			times: HashMap::new(),
		};
		for layer in project.layers.iter().rev() {
			if !layer.is_active(frame) {
				continue;
			}
			if let LayerKind::Adjustment = layer.kind {
				self.adjust(&mut canvas, layer, &mut ctx);
				continue;
			}
			let Some(pixmap) = self.layer_pixmap(layer, &mut ctx) else {
				continue;
			};
			let transform = layer.transform.matrix(frame);
			// Whole-pixel moves sample exactly; anything else filters.
			let quality = if transform.is_translate() && transform.tx.fract() == 0.0 && transform.ty.fract() == 0.0 {
				FilterQuality::Nearest
			} else {
				FilterQuality::Bilinear
			};
			let paint = PixmapPaint {
				opacity: (layer.transform.opacity.at(frame) / 100.0).clamp(0.0, 1.0),
				blend_mode: blend_mode(layer.blend),
				quality,
			};
			canvas.draw_pixmap(0, 0, pixmap.as_ref().as_ref(), &paint, transform, None);
		}

		FrameResult {
			generation,
			frame,
			image: Image::from_pixmap(&canvas),
			elapsed: started.elapsed(),
			problems: ctx.problems,
			effect_times: ctx.times,
		}
	}

	/// Run an adjustment layer's effects over everything below it, limited by
	/// its masks and opacity.
	fn adjust(&mut self, canvas: &mut Pixmap, layer: &Layer, ctx: &mut Frame) {
		if !layer.effects.iter().any(|e| e.enabled) {
			return;
		}
		let below = Image::from_pixmap(canvas);
		let (w, h) = (below.width, below.height);
		let processed = self.apply_effects(layer, below, ctx).to_pixmap();
		let coverage = mask_coverage(&layer.masks, w, h, ctx.frame);
		let opacity = (layer.transform.opacity.at(ctx.frame) / 100.0).clamp(0.0, 1.0);
		for (i, (dst, src)) in canvas
			.data_mut()
			.as_chunks_mut::<4>()
			.0
			.iter_mut()
			.zip(processed.data().as_chunks::<4>().0)
			.enumerate()
		{
			let k = opacity * coverage.as_ref().map_or(1.0, |c| c[i]);
			for (d, &s) in dst.iter_mut().zip(src) {
				*d = (*d as f32 + (s as f32 - *d as f32) * k).round() as u8;
			}
		}
	}

	/// A layer after masks and effects, untransformed; `None` when it is
	/// outside its time span or part of a cycle.
	fn layer_image(&mut self, id: Id, ctx: &mut Frame) -> Option<Image> {
		if let Some(done) = ctx.done.get(&id) {
			return done.clone();
		}
		let project = ctx.project;
		let layer = project.layer(id)?;
		if !(layer.in_frame..layer.out_frame).contains(&ctx.frame) {
			return None;
		}
		if !ctx.visiting.insert(id) {
			ctx.problems.push(format!("{}: layer links form a cycle", layer.name));
			return None;
		}
		let source = self.source(layer, ctx);
		let out = Some(self.apply_effects(layer, source, ctx));
		ctx.visiting.remove(&id);
		ctx.done.insert(id, out.clone());
		out
	}

	/// A layer's own pixels with its masks applied.
	fn source(&mut self, layer: &Layer, ctx: &mut Frame) -> Image {
		if let Some(source) = ctx.sources.get(&layer.id) {
			return source.clone();
		}
		let key = source_key(layer, &ctx.project.comp, ctx.frame);
		if let Some(cached) = self.sources.get(&layer.id).filter(|c| c.key == key) {
			let image = cached.image.clone();
			ctx.sources.insert(layer.id, image.clone());
			return image;
		}
		let (w, h) = layer.size(&ctx.project.comp);
		let frame = ctx.frame;
		let mut image = match &layer.kind {
			LayerKind::Solid { color, .. } => Image::filled(w, h, color.at(frame)),
			LayerKind::Image { path, .. } => match self.images.get(path) {
				Ok(image) => image.clone(),
				Err(e) => {
					ctx.problems.push(format!("{}: {e}", layer.name));
					Image::filled(w, h, [1.0, 0.0, 1.0, 1.0])
				}
			},
			LayerKind::Shape { items } => render_shapes(items, w, h, frame),
			LayerKind::Text(text) => render_text(text, w, h, frame, &mut self.fonts),
			LayerKind::Adjustment => Image::transparent(w, h),
		};
		if let Some(coverage) = mask_coverage(&layer.masks, image.width, image.height, frame) {
			image.multiply_alpha(&coverage);
		}
		ctx.sources.insert(layer.id, image.clone());
		self.sources.insert(
			layer.id,
			CachedSource {
				key,
				image: image.clone(),
				pixmap: None,
			},
		);
		image
	}

	/// The premultiplied pixels to composite for a layer.
	fn layer_pixmap(&mut self, layer: &Layer, ctx: &mut Frame) -> Option<Arc<Pixmap>> {
		let has_effects = layer.effects.iter().any(|e| e.enabled);
		if !has_effects {
			let key = source_key(layer, &ctx.project.comp, ctx.frame);
			if let Some(pixmap) = self
				.sources
				.get(&layer.id)
				.filter(|c| c.key == key)
				.and_then(|c| c.pixmap.clone())
			{
				return Some(pixmap);
			}
		}
		let pixmap = Arc::new(self.layer_image(layer.id, ctx)?.to_pixmap());
		if !has_effects && let Some(cached) = self.sources.get_mut(&layer.id) {
			cached.pixmap = Some(pixmap.clone());
		}
		Some(pixmap)
	}

	fn apply_effects(&mut self, layer: &Layer, mut image: Image, ctx: &mut Frame) -> Image {
		for effect in layer.effects.iter().filter(|e| e.enabled) {
			// Render linked layers first: they may run other effects.
			let links: Vec<(usize, Option<Image>)> = effect
				.layers
				.iter()
				.map(|(&index, &target)| {
					let linked = if target == layer.id {
						Some(self.source(layer, ctx))
					} else {
						self.layer_image(target, ctx)
					};
					(index, linked)
				})
				.collect();
			let started = Instant::now();
			match self.run_effect(layer, effect, &image, links, ctx) {
				Ok(out) => image = out,
				Err(e) => ctx.problems.push(format!(
					"{} › {}: {e}",
					layer.name,
					plugins::display_name(&effect.plugin)
				)),
			}
			ctx.times.insert(effect.id, started.elapsed());
		}
		image
	}

	fn run_effect(
		&mut self,
		layer: &Layer,
		effect: &Effect,
		input: &Image,
		links: Vec<(usize, Option<Image>)>,
		ctx: &Frame,
	) -> Result<Image, String> {
		let slot = self.slots.get_mut(&effect.id).ok_or("not loaded")?;
		let info = slot.info.clone();
		let fx = slot.instance.as_mut().map_err(|e| format!("failed to load: {e}"))?;
		let info = info.ok_or("no plugin info")?;
		let (w, h) = (input.width, input.height);

		fx.set_input_layer(to_layer(input)?);
		fx.set_render_size(w, h);
		fx.set_time(ctx.frame - layer.start, 1, ctx.project.comp.fps.max(1));
		if slot.size != (w, h) {
			// Resizing the input re-resolves point defaults behind our back.
			slot.applied.clear();
			slot.size = (w, h);
		}

		let mut changed = false;
		for desc in &info.params {
			let Some(default) = info.default_value(desc.index, (w, h)) else {
				continue;
			};
			let value = effect.params.get(&desc.index).map_or(default, |a| a.at(ctx.frame));
			if slot.applied.get(&desc.index) == Some(&value) {
				continue;
			}
			if let Err(e) = fx.set_param(desc.index, (&value).into()) {
				log::warn!("{}: set_param({}) failed: {e}", slot.plugin, desc.index);
			}
			slot.applied.insert(desc.index, value);
			changed = true;
		}

		fx.set_mask_paths(layer.masks.iter().map(mask_path).collect());

		let mut linked = BTreeSet::new();
		for (index, image) in links {
			let layer = image.as_ref().map(to_layer).transpose()?;
			fx.set_layer_param(index, layer).map_err(|e| e.to_string())?;
			linked.insert(index);
		}
		for stale in slot.linked.difference(&linked) {
			let _ = fx.set_layer_param(*stale, None);
		}
		slot.linked = linked;

		if changed {
			if let Err(e) = fx.update_params_ui() {
				log::debug!("{}: update_params_ui failed: {e}", slot.plugin);
			}
			if slot.refresh_ui_flags() {
				self.notify.send(Event::ParamUi {
					effect: effect.id,
					hidden: slot.hidden.clone(),
					disabled: slot.disabled.clone(),
				});
			}
		}

		let Ok(fx) = &mut slot.instance else { unreachable!() };
		fx.render_frame().map_err(|e| e.to_string())?;
		let mut out = vec![0u8; w as usize * h as usize * 4];
		fx.write_rendered_pixels(&mut out).map_err(|e| e.to_string())?;
		Ok(Image {
			width: w,
			height: h,
			pixels: out,
		})
	}

	fn press_button(&mut self, effect: Id, index: usize, frame: i32) {
		let Some(slot) = self.slots.get_mut(&effect) else {
			return;
		};
		let Ok(fx) = &mut slot.instance else { return };
		if let Err(e) = fx.user_changed_param(index) {
			log::warn!("{}: button {index} failed: {e}", slot.plugin);
		}
		// The plugin may have changed other parameters in response.
		let mut values = Vec::new();
		for (i, value) in fx.param_values() {
			let value = PValue::from(value);
			if slot.applied.get(&i).is_some_and(|v| *v != value) {
				slot.applied.insert(i, value.clone());
				values.push((i, value));
			}
		}
		let _ = fx.update_params_ui();
		if slot.refresh_ui_flags() {
			self.notify.send(Event::ParamUi {
				effect,
				hidden: slot.hidden.clone(),
				disabled: slot.disabled.clone(),
			});
		}
		if !values.is_empty() {
			self.notify.send(Event::ParamsChanged { effect, frame, values });
		}
	}

	fn export(&mut self, job: ExportJob) -> Result<String, String> {
		let (start, end) = (job.start.min(job.end), job.start.max(job.end));
		let total = end - start + 1;
		let (dir, temp) = match &job.kind {
			ExportKind::Png(_) => (PathBuf::new(), false),
			ExportKind::Sequence(dir) => (dir.clone(), false),
			ExportKind::Video(_) => {
				let dir = std::env::temp_dir().join(format!("aexlo-studio-{}", std::process::id()));
				(dir, true)
			}
		};
		if !matches!(job.kind, ExportKind::Png(_)) {
			std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
		}
		let result = (|| {
			for (n, frame) in (start..=end).enumerate() {
				if self.cancel.load(Ordering::Relaxed) {
					return Err("export cancelled".to_string());
				}
				let image = self.render(&job.project, frame, 0).image;
				let path = match &job.kind {
					ExportKind::Png(path) => path.clone(),
					_ => dir.join(format!("frame_{n:05}.png")),
				};
				image::save_buffer(
					&path,
					&image.pixels,
					image.width,
					image.height,
					image::ExtendedColorType::Rgba8,
				)
				.map_err(|e| format!("{}: {e}", path.display()))?;
				self.notify.send(Event::ExportProgress {
					done: n as i32 + 1,
					total,
				});
			}
			match &job.kind {
				ExportKind::Png(path) => Ok(format!("Saved {}", path.display())),
				ExportKind::Sequence(dir) => Ok(format!("Saved {total} frames to {}", dir.display())),
				ExportKind::Video(out) => {
					let status = std::process::Command::new("ffmpeg")
						.args(["-y", "-loglevel", "error", "-framerate"])
						.arg(job.project.comp.fps.to_string())
						.arg("-i")
						.arg(dir.join("frame_%05d.png"))
						.args([
							"-c:v",
							"libx264",
							"-pix_fmt",
							"yuv420p",
							"-vf",
							"pad=ceil(iw/2)*2:ceil(ih/2)*2",
						])
						.arg(out)
						.status()
						.map_err(|e| format!("could not run ffmpeg: {e}"))?;
					if status.success() {
						Ok(format!("Saved {}", out.display()))
					} else {
						Err(format!("ffmpeg failed ({status})"))
					}
				}
			}
		})();
		if temp {
			let _ = std::fs::remove_dir_all(&dir);
		}
		result
	}
}

fn to_layer(image: &Image) -> Result<aexlo::Layer<Depth8>, String> {
	aexlo::Layer::<Depth8>::from_raw(image.pixels.clone(), image.width, image.height).map_err(|e| e.to_string())
}

fn mask_path(mask: &crate::model::Mask) -> aexlo::MaskPath {
	aexlo::MaskPath {
		id: mask.id as u32,
		name: mask.name.clone(),
		vertices: mask
			.path
			.vertices
			.iter()
			.map(|v| aexlo::MaskVertex {
				x: v.p[0] as f64,
				y: v.p[1] as f64,
				tan_in_x: v.tin[0] as f64,
				tan_in_y: v.tin[1] as f64,
				tan_out_x: v.tout[0] as f64,
				tan_out_y: v.tout[1] as f64,
			})
			.collect(),
		closed: mask.path.closed,
		inverted: mask.inverted,
		mode: mask.mode.to_aexlo(),
	}
}

fn blend_mode(mode: BlendMode) -> tiny_skia::BlendMode {
	use tiny_skia::BlendMode as B;
	match mode {
		BlendMode::Normal => B::SourceOver,
		BlendMode::Add => B::Plus,
		BlendMode::Multiply => B::Multiply,
		BlendMode::Screen => B::Screen,
		BlendMode::Overlay => B::Overlay,
		BlendMode::Darken => B::Darken,
		BlendMode::Lighten => B::Lighten,
		BlendMode::ColorDodge => B::ColorDodge,
		BlendMode::ColorBurn => B::ColorBurn,
		BlendMode::HardLight => B::HardLight,
		BlendMode::SoftLight => B::SoftLight,
		BlendMode::Difference => B::Difference,
		BlendMode::Exclusion => B::Exclusion,
		BlendMode::Hue => B::Hue,
		BlendMode::Saturation => B::Saturation,
		BlendMode::Color => B::Color,
		BlendMode::Luminosity => B::Luminosity,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::{Anim, BezPath, Effect, ShapeItem};

	fn worker() -> Worker {
		Worker {
			slots: HashMap::new(),
			sources: HashMap::new(),
			images: ImageCache::default(),
			fonts: Fonts::new(),
			notify: Notify {
				tx: channel().0,
				repaint: Box::new(|| {}),
			},
			cancel: Arc::new(AtomicBool::new(false)),
		}
	}

	fn project(width: u32, height: u32) -> Project {
		let mut p = Project::default();
		p.comp.width = width;
		p.comp.height = height;
		p.comp.background = [0.0; 4];
		p
	}

	fn solid(p: &mut Project, name: &str, color: [f32; 4], w: u32, h: u32) -> Layer {
		p.new_layer(
			name,
			LayerKind::Solid {
				color: Anim::new(color),
				width: w,
				height: h,
			},
		)
	}

	fn at(image: &Image, x: u32, y: u32) -> [u8; 4] {
		let i = ((y * image.width + x) * 4) as usize;
		image.pixels[i..i + 4].try_into().unwrap()
	}

	fn has_fixture(name: &str) -> bool {
		let found = plugins::list_fixtures().iter().any(|f| f == name);
		if !found {
			eprintln!("skipping: fixture '{name}' not present");
		}
		found
	}

	fn effect(p: &mut Project, plugin: &str, params: &[(usize, PValue)]) -> Effect {
		let mut fx = p.new_effect(plugin);
		for (i, v) in params {
			fx.params.insert(*i, Anim::new(v.clone()));
		}
		fx
	}

	#[test]
	fn layers_composite_with_their_transform() {
		let mut p = project(64, 32);
		let mut red = solid(&mut p, "red", [1.0, 0.0, 0.0, 1.0], 16, 16);
		red.transform.scale = Anim::new([200.0, 200.0]);
		p.layers.push(red);
		let out = worker().render(&p, 0, 1).image;
		assert_eq!(at(&out, 32, 16), [255, 0, 0, 255]);
		assert_eq!(at(&out, 17, 16), [255, 0, 0, 255]);
		assert_eq!(at(&out, 10, 16), [0, 0, 0, 0]);
	}

	#[test]
	fn top_layer_wins_and_respects_time_span() {
		let mut p = project(8, 8);
		let mut top = solid(&mut p, "top", [0.0, 1.0, 0.0, 1.0], 8, 8);
		top.in_frame = 5;
		let bottom = solid(&mut p, "bottom", [0.0, 0.0, 1.0, 1.0], 8, 8);
		p.layers = vec![top, bottom];
		let mut w = worker();
		assert_eq!(at(&w.render(&p, 0, 1).image, 4, 4), [0, 0, 255, 255]);
		assert_eq!(at(&w.render(&p, 5, 1).image, 4, 4), [0, 255, 0, 255]);
	}

	#[test]
	fn masks_cut_the_layer() {
		let mut p = project(20, 10);
		let mut layer = solid(&mut p, "s", [1.0; 4], 20, 10);
		layer.masks.push(crate::model::Mask {
			id: 1,
			name: "m".into(),
			path: BezPath::rect(0.0, 0.0, 10.0, 10.0),
			mode: crate::model::MaskMode::Add,
			inverted: false,
			opacity: Anim::new(100.0),
			feather: Anim::new(0.0),
		});
		p.layers.push(layer);
		let out = worker().render(&p, 0, 1).image;
		assert_eq!(at(&out, 5, 5)[3], 255);
		assert_eq!(at(&out, 15, 5)[3], 0);
	}

	#[test]
	fn plugin_effects_run_on_their_layer() {
		if !has_fixture("FillColor") {
			return;
		}
		let mut p = project(32, 16);
		let mut layer = solid(&mut p, "s", [1.0; 4], 16, 16);
		let fill = effect(
			&mut p,
			"FillColor",
			&[(1, PValue::Checkbox(true)), (2, PValue::Color([0, 0, 255, 255]))],
		);
		layer.effects.push(fill);
		p.layers.push(layer);
		let mut w = worker();
		let result = w.render(&p, 0, 1);
		assert!(result.problems.is_empty(), "{:?}", result.problems);
		assert_eq!(at(&result.image, 16, 8), [0, 0, 255, 255]);
		// Outside the 16 px layer, the comp stays empty.
		assert_eq!(at(&result.image, 2, 8), [0, 0, 0, 0]);

		// Disabled effects pass the layer through.
		p.layers[0].effects[0].enabled = false;
		assert_eq!(at(&w.render(&p, 0, 2).image, 16, 8), [255, 255, 255, 255]);
	}

	#[test]
	fn adjustment_layers_process_what_is_below() {
		if !has_fixture("FillColor") {
			return;
		}
		let mut p = project(16, 8);
		let mut adjust = p.new_layer("adjust", LayerKind::Adjustment);
		let fill = effect(
			&mut p,
			"FillColor",
			&[(1, PValue::Checkbox(true)), (2, PValue::Color([0, 255, 0, 255]))],
		);
		adjust.effects.push(fill);
		adjust.masks.push(crate::model::Mask {
			id: 1,
			name: "left".into(),
			path: BezPath::rect(0.0, 0.0, 8.0, 8.0),
			mode: crate::model::MaskMode::Add,
			inverted: false,
			opacity: Anim::new(100.0),
			feather: Anim::new(0.0),
		});
		let below = solid(&mut p, "below", [1.0, 0.0, 0.0, 1.0], 16, 8);
		p.layers = vec![adjust, below];
		let out = worker().render(&p, 0, 1).image;
		assert_eq!(at(&out, 3, 4), [0, 255, 0, 255]);
		assert_eq!(at(&out, 12, 4), [255, 0, 0, 255]);
	}

	/// DisplacerPro reads its map from another layer, which is hidden: the
	/// link still renders it.
	#[test]
	fn layer_params_read_other_layers() {
		if !has_fixture("DisplacerPro") {
			return;
		}
		const MAP_LAYER: usize = 1;
		const TRANSLATE_X: usize = 15;
		let mut p = project(64, 16);
		let mut map = solid(&mut p, "map", [1.0; 4], 64, 16);
		map.visible = false;
		let mut img = p.new_layer(
			"gradient",
			LayerKind::Shape {
				items: vec![{
					let mut item = ShapeItem::new(
						99,
						crate::model::Geometry::Rect {
							size: Anim::new([16.0, 16.0]),
							roundness: Anim::new(0.0),
						},
						[24.0, 8.0],
						[1.0, 0.0, 0.0, 1.0],
					);
					item.name = "block".into();
					item
				}],
			},
		);
		let mut displace = effect(&mut p, "DisplacerPro", &[(TRANSLATE_X, PValue::Float(25.0))]);
		displace.layers.insert(MAP_LAYER, map.id);
		img.effects.push(displace);
		p.layers = vec![img, map];

		let result = worker().render(&p, 0, 1);
		assert!(result.problems.is_empty(), "{:?}", result.problems);
		// 25% of 64 px: the block at x 16..32 moves to 32..48.
		assert_eq!(at(&result.image, 40, 8), [255, 0, 0, 255]);
		assert_eq!(at(&result.image, 20, 8)[3], 0);
	}

	#[test]
	fn layer_link_cycles_are_cut() {
		if !has_fixture("DisplacerPro") {
			return;
		}
		let mut p = project(16, 16);
		let mut a = solid(&mut p, "a", [1.0; 4], 16, 16);
		let mut b = solid(&mut p, "b", [1.0; 4], 16, 16);
		let mut fa = effect(&mut p, "DisplacerPro", &[]);
		fa.layers.insert(1, b.id);
		let mut fb = effect(&mut p, "DisplacerPro", &[]);
		fb.layers.insert(1, a.id);
		a.effects.push(fa);
		b.effects.push(fb);
		p.layers = vec![a, b];
		let result = worker().render(&p, 0, 1);
		assert!(
			result.problems.iter().any(|m| m.contains("cycle")),
			"{:?}",
			result.problems
		);
	}

	#[test]
	fn effects_see_layer_time() {
		if !has_fixture("Furikake") {
			return;
		}
		let mut p = project(320, 180);
		let mut null = solid(&mut p, "null", [0.0; 4], 320, 180);
		null.effects.push(p.new_effect("Furikake"));
		p.layers.push(null);
		let mut w = worker();
		let white = |img: &Image| img.pixels.chunks(4).filter(|px| px == &[255, 255, 255, 255]).count();
		let early = white(&w.render(&p, 0, 1).image);
		let late = white(&w.render(&p, 60, 1).image);
		assert!(late > early, "{early} -> {late}");
		// Sliding the layer 60 frames later restarts its clock.
		p.layers[0].shift(60);
		assert_eq!(white(&w.render(&p, 60, 1).image), early);
	}
}

#[cfg(test)]
mod profile {
	use super::*;

	/// `cargo test -p studio --release -- --ignored --nocapture profile`
	#[test]
	#[ignore]
	fn profile_demo() {
		let p = crate::demo::project();
		let mut w = tests_worker();
		for frame in 0..6 {
			let r = w.render(&p, frame, 1);
			eprintln!("frame {frame}: {:?} effects {:?}", r.elapsed, r.effect_times);
		}
		for i in 0..p.layers.len() {
			let mut one = p.clone();
			one.layers = vec![p.layers[i].clone()];
			let mut w = tests_worker();
			w.render(&one, 0, 1);
			let t: Vec<_> = (1..4).map(|f| w.render(&one, f, 1).elapsed).collect();
			eprintln!("only {}: {t:?}", p.layers[i].name);
		}
		let mut empty = p.clone();
		empty.layers.clear();
		eprintln!("empty: {:?}", w.render(&empty, 0, 1).elapsed);
		let mut layers_only = p.clone();
		for l in &mut layers_only.layers {
			l.effects.clear();
		}
		for frame in 0..3 {
			eprintln!("no effects {frame}: {:?}", w.render(&layers_only, frame, 1).elapsed);
		}
	}

	fn tests_worker() -> Worker {
		Worker {
			slots: HashMap::new(),
			sources: HashMap::new(),
			images: ImageCache::default(),
			fonts: Fonts::new(),
			notify: Notify {
				tx: channel().0,
				repaint: Box::new(|| {}),
			},
			cancel: Arc::new(AtomicBool::new(false)),
		}
	}
}
