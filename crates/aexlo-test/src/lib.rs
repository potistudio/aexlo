//! The dev-dependency for testing After Effects plugins written in Rust with
//! aexlo (`docs/toolkit.md` §13).
//!
//! ```ignore
//! // Cargo.toml: [dev-dependencies] aexlo-test = "0.1"
//!
//! #[aexlo::test(depth = [8, 16, 32], render = [smart, gpu], preset = "dot_glow")]
//! fn glow_is_symmetric(fx: &mut aexlo::PluginInstance) -> aexlo_test::Result {
//!     fx.set_param_named("Radius", 100.0)?;
//!     let frame = aexlo_test::render(fx)?;
//!     frame.assert_symmetric_about((320, 240), 0..20, 4.0 / 255.0)?;
//!     frame.assert_golden("dot_glow")?;
//!     Ok(())
//! }
//! ```
//!
//! Tests run in-process against the crate's own `EffectMain`. Goldens live
//! in `golden/` next to the nearest `aexlo.toml` (else the crate), shared
//! with `aexlo test`; set `AEXLO_BLESS=1` to write missing or changed ones.
//! A failed assertion writes `actual`/`expected`/`diff` images under
//! `target/aexlo/<test name>/` there.

use std::cell::RefCell;
use std::fmt::Display;
use std::ops::Range;
use std::path::{Path, PathBuf};

pub use aexlo;
use aexlo::PluginInstance;
use aexlo_harness::golden::{self, Verdict};
pub use aexlo_harness::{Frame, Tolerance};
use aexlo_harness::{PluginRef, PluginSource, RenderMode, Variant, apply, check, manifest};
pub use aexlo_macros::{preview, test};

/// What test bodies `use`: the extension traits.
pub mod prelude {
	pub use crate::{FrameAssert, InstanceExt};
}

/// A failed test step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
	message: String,
}

impl Error {
	pub fn new(message: impl Into<String>) -> Self {
		Self {
			message: message.into(),
		}
	}

	pub fn message(&self) -> &str {
		&self.message
	}
}

impl Display for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.message)
	}
}

impl std::error::Error for Error {}

impl From<aexlo::AexloError> for Error {
	fn from(err: aexlo::AexloError) -> Self {
		Self::new(err.to_string())
	}
}

impl From<aexlo_harness::Error> for Error {
	fn from(err: aexlo_harness::Error) -> Self {
		Self::new(err.message)
	}
}

/// What test bodies and assertions return.
pub type Result<T = ()> = std::result::Result<T, Error>;

/// The test being run on this thread.
struct Context {
	name: String,
	golden_suffix: String,
	/// Where goldens and artifacts live.
	root: PathBuf,
	variant: Variant,
}

thread_local! {
	static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
}

fn with_context<R>(f: impl FnOnce(Option<&Context>) -> R) -> R {
	CONTEXT.with(|c| f(c.borrow().as_ref()))
}

/// Render `fx` through the test's render path and return the frame.
pub fn render(fx: &mut PluginInstance) -> Result<Frame> {
	let mode = with_context(|c| c.map_or(RenderMode::Auto, |c| c.variant.render));
	apply::render(fx, mode).map_err(|e| Error::new(format!("{} render failed: {e}", mode.name())))?;
	Ok(Frame::new(fx.output().clone()))
}

/// Run §7 checks on an in-process instance: `finite`, `coverage`,
/// `deterministic`, `iterate-parallel`, `bounds`, `allocations` and
/// `checkouts`. Renders as often as the checks need.
pub fn check(fx: &mut PluginInstance, ids: &[&str]) -> Result {
	let known = [
		"finite",
		"coverage",
		"deterministic",
		"iterate-parallel",
		"bounds",
		"allocations",
		"checkouts",
	];
	if let Some(unknown) = ids.iter().find(|id| !known.contains(id)) {
		return Err(Error::new(format!("check '{unknown}' does not run in-process")));
	}
	let wants = |id: &str| ids.contains(&id);
	let previous = fx.strict();
	fx.set_strict(aexlo::Strict {
		poison_output: wants("coverage") || previous.poison_output,
		guard_bands: wants("bounds") || previous.guard_bands,
		track_allocations: wants("allocations") || previous.track_allocations,
		track_checkouts: wants("checkouts") || previous.track_checkouts,
	});
	let result = (|| {
		let first = render(fx)?;
		let mut results = Vec::new();
		if wants("coverage") {
			results.push(check::coverage(&first));
		}
		if wants("finite") {
			results.push(check::finite(&first));
		}
		if wants("deterministic") {
			let second = render(fx)?;
			results.push(check::deterministic(&[first.clone(), second]));
		}
		if wants("iterate-parallel") {
			let parallel = fx.parallel_iterate();
			fx.set_parallel_iterate(!parallel);
			let other = render(fx);
			fx.set_parallel_iterate(parallel);
			results.push(check::agrees(
				"iterate-parallel",
				"the other iteration mode",
				&first,
				&other?,
				check::ITERATE_TOLERANCE,
			));
		}
		let report = fx.strict_report();
		let findings = aexlo_harness::exec::StrictFindings::from(report);
		for id in ["bounds", "allocations", "checkouts"] {
			if wants(id) {
				results.push(check::strict(id, &findings));
			}
		}
		for result in &results {
			if result.status == aexlo_harness::runner::CheckStatus::Fail {
				let message = format!("{}: {}", result.id, result.message.as_deref().unwrap_or("failed"));
				return Err(fail_with_artifacts(&message, &first, None, &Tolerance::default()));
			}
		}
		Ok(())
	})();
	fx.set_strict(previous);
	result
}

/// Setting parameters by name, for test bodies.
pub trait InstanceExt {
	/// Set the parameter named `name` (case-insensitive, or `#index`) to a
	/// TOML-like value interpreted by its kind, as a manifest's `params` are:
	/// numbers for sliders and angles, `true`/`false`, popup labels or
	/// 1-based indices, `[x, y]` points and normalized `[r, g, b(, a)]` colors.
	fn set_param_named(&mut self, name: &str, value: impl Into<toml::Value>) -> Result;
}

impl InstanceExt for PluginInstance {
	fn set_param_named(&mut self, name: &str, value: impl Into<toml::Value>) -> Result {
		let index = apply::resolve_param(self, name)?;
		let value = apply::param_value(self, index, &value.into()).map_err(|e| Error::new(format!("{name}: {e}")))?;
		self.set_param(index, value)?;
		Ok(())
	}
}

/// Where the current test's artifacts go.
fn artifacts_dir() -> PathBuf {
	with_context(|c| match c {
		Some(c) => golden::artifacts_dir(&c.root, &c.name),
		None => std::env::temp_dir().join("aexlo-test-artifacts"),
	})
}

/// An error whose message says where the frames that failed were written.
fn fail_with_artifacts(message: &str, actual: &Frame, expected: Option<&Frame>, tolerance: &Tolerance) -> Error {
	let dir = artifacts_dir();
	match golden::write_artifacts(&dir, actual, expected, tolerance) {
		Ok(dir) => Error::new(format!("{message}\n  artifacts: {}", dir.display())),
		Err(e) => Error::new(format!("{message}\n  (writing artifacts failed: {})", e.message)),
	}
}

fn within(a: [f32; 4], b: [f32; 4], max_abs: f32) -> bool {
	a.iter()
		.zip(b)
		.all(|(x, y)| (x - y).abs() <= max_abs || (x.is_nan() && y.is_nan()))
}

/// Assertions on rendered frames. Values are normalized to `[0, 1]`
/// whatever the depth, so one tolerance fits every depth.
pub trait FrameAssert {
	/// Compare against the golden `name` (§8): `golden/<name>[.d16].<png|exr>`
	/// next to the nearest manifest, within the preset's tolerance (default
	/// `1/255`). Missing goldens fail unless `AEXLO_BLESS` is set.
	fn assert_golden(&self, name: &str) -> Result;
	fn assert_golden_within(&self, name: &str, tolerance: &Tolerance) -> Result;
	/// Every channel of every pixel within `max_abs` of `other`.
	fn assert_close(&self, other: &Frame, max_abs: f32) -> Result;
	/// The pixel at `(x, y)` within `max_abs` of normalized `rgba`.
	fn assert_pixel(&self, x: u32, y: u32, rgba: [f32; 4], max_abs: f32) -> Result;
	/// Every pixel of `x0..x1` × `y0..y1` within `max_abs` of `rgba`.
	fn assert_region(&self, rect: (Range<u32>, Range<u32>), rgba: [f32; 4], max_abs: f32) -> Result;
	/// Mirror-symmetric in x and y about `center`, for every pixel whose
	/// distance from it falls in `radii`.
	fn assert_symmetric_about(&self, center: (u32, u32), radii: Range<u32>, max_abs: f32) -> Result;
	/// Alpha is 1 everywhere.
	fn assert_opaque(&self) -> Result;
	/// No NaN or infinity anywhere.
	fn assert_finite(&self) -> Result;
}

impl FrameAssert for Frame {
	fn assert_golden(&self, name: &str) -> Result {
		let tolerance =
			with_context(|c| c.and_then(|c| c.variant.golden.as_ref()).map(|g| g.tolerance)).unwrap_or_default();
		self.assert_golden_within(name, &tolerance)
	}

	fn assert_golden_within(&self, name: &str, tolerance: &Tolerance) -> Result {
		let (root, suffix) = with_context(|c| match c {
			Some(c) => (c.root.clone(), c.golden_suffix.clone()),
			None => (PathBuf::from("."), String::new()),
		});
		let path = root
			.join("golden")
			.join(format!("{name}{suffix}.{}", Frame::extension(self.depth())));
		let spec = aexlo_harness::GoldenSpec {
			tolerance: *tolerance,
			..aexlo_harness::GoldenSpec::default()
		};
		let bless = std::env::var_os("AEXLO_BLESS").is_some();
		let (verdict, expected) = golden::judge(self, &path, &spec, bless)?;
		match verdict {
			Verdict::Matched(_) => Ok(()),
			Verdict::Blessed { created } => {
				eprintln!(
					"aexlo-test: {} {}",
					if created { "blessed new" } else { "re-blessed" },
					path.display()
				);
				Ok(())
			}
			Verdict::Missing => Err(fail_with_artifacts(
				&format!("missing golden {} (set AEXLO_BLESS=1 to write it)", path.display()),
				self,
				None,
				tolerance,
			)),
			Verdict::Mismatched(cmp) => Err(fail_with_artifacts(
				&format!("differs from {}: {}", path.display(), cmp.summary(tolerance)),
				self,
				expected.as_ref(),
				tolerance,
			)),
		}
	}

	fn assert_close(&self, other: &Frame, max_abs: f32) -> Result {
		let tolerance = Tolerance {
			max_abs,
			..Tolerance::default()
		};
		let cmp = self.compare(other, &tolerance);
		if cmp.passed {
			Ok(())
		} else {
			Err(fail_with_artifacts(
				&format!("frames differ: {}", cmp.summary(&tolerance)),
				self,
				Some(other),
				&tolerance,
			))
		}
	}

	fn assert_pixel(&self, x: u32, y: u32, rgba: [f32; 4], max_abs: f32) -> Result {
		if x >= self.width() || y >= self.height() {
			return Err(Error::new(format!(
				"pixel ({x}, {y}) is outside the {}x{} frame",
				self.width(),
				self.height()
			)));
		}
		let got = self.pixel(x, y);
		if within(got, rgba, max_abs) {
			Ok(())
		} else {
			Err(fail_with_artifacts(
				&format!("pixel ({x}, {y}) is {got:?}, expected {rgba:?} ± {max_abs}"),
				self,
				None,
				&Tolerance::default(),
			))
		}
	}

	fn assert_region(&self, (xs, ys): (Range<u32>, Range<u32>), rgba: [f32; 4], max_abs: f32) -> Result {
		for y in ys.start..ys.end.min(self.height()) {
			for x in xs.start..xs.end.min(self.width()) {
				let got = self.pixel(x, y);
				if !within(got, rgba, max_abs) {
					return Err(fail_with_artifacts(
						&format!("pixel ({x}, {y}) of the region is {got:?}, expected {rgba:?} ± {max_abs}"),
						self,
						None,
						&Tolerance::default(),
					));
				}
			}
		}
		Ok(())
	}

	fn assert_symmetric_about(&self, (cx, cy): (u32, u32), radii: Range<u32>, max_abs: f32) -> Result {
		let (w, h) = (self.width() as i64, self.height() as i64);
		let (cx, cy) = (cx as i64, cy as i64);
		let r = radii.end as i64;
		let inside = |x: i64, y: i64| x >= 0 && y >= 0 && x < w && y < h;
		for dy in 0..r {
			for dx in 0..r {
				let d2 = dx * dx + dy * dy;
				if d2 < (radii.start as i64).pow(2) || d2 >= r * r {
					continue;
				}
				let points = [
					(cx + dx, cy + dy),
					(cx - dx, cy + dy),
					(cx + dx, cy - dy),
					(cx - dx, cy - dy),
				];
				if !points.iter().all(|&(x, y)| inside(x, y)) {
					continue;
				}
				let reference = self.pixel(points[0].0 as u32, points[0].1 as u32);
				for &(x, y) in &points[1..] {
					let got = self.pixel(x as u32, y as u32);
					if !within(got, reference, max_abs) {
						return Err(fail_with_artifacts(
							&format!(
								"not symmetric about ({cx}, {cy}): ({x}, {y}) is {got:?} but ({}, {}) is {reference:?}",
								points[0].0, points[0].1
							),
							self,
							None,
							&Tolerance::default(),
						));
					}
				}
			}
		}
		Ok(())
	}

	fn assert_opaque(&self) -> Result {
		match self.unit_rgba().iter().position(|p| p[3] != 1.0) {
			None => Ok(()),
			Some(i) => {
				let (x, y) = (i as u32 % self.width(), i as u32 / self.width());
				Err(fail_with_artifacts(
					&format!("pixel ({x}, {y}) has alpha {}", self.pixel(x, y)[3]),
					self,
					None,
					&Tolerance::default(),
				))
			}
		}
	}

	fn assert_finite(&self) -> Result {
		match self.unit_rgba().iter().position(|p| p.iter().any(|c| !c.is_finite())) {
			None => Ok(()),
			Some(i) => {
				let (x, y) = (i as u32 % self.width(), i as u32 / self.width());
				Err(fail_with_artifacts(
					&format!("pixel ({x}, {y}) is {:?}", self.pixel(x, y)),
					self,
					None,
					&Tolerance::default(),
				))
			}
		}
	}
}

#[doc(hidden)]
pub mod __private {
	use super::*;

	/// One expanded `#[aexlo::test]`.
	pub struct TestSpec {
		pub entry: usize,
		pub crate_dir: &'static str,
		pub name: &'static str,
		pub preset: Option<&'static str>,
		pub depth: Option<u32>,
		pub render: Option<&'static str>,
		pub golden_suffix: &'static str,
	}

	/// What a test body may return.
	pub trait TestOutcome {
		fn into_result(self) -> Result;
	}

	impl TestOutcome for () {
		fn into_result(self) -> Result {
			Ok(())
		}
	}

	impl<E: Display> TestOutcome for std::result::Result<(), E> {
		fn into_result(self) -> Result {
			self.map_err(|e| Error::new(e.to_string()))
		}
	}

	fn in_process(entry: &str) -> PluginRef {
		PluginRef {
			source: PluginSource::Artifact(PathBuf::from("<in-process>")),
			entry: entry.to_string(),
		}
	}

	/// The variant `spec` names, and the directory goldens live in.
	fn resolve(spec: &TestSpec) -> Result<(Variant, PathBuf)> {
		let crate_dir = Path::new(spec.crate_dir);
		let found = manifest::discover(crate_dir);
		let root = found
			.as_deref()
			.and_then(Path::parent)
			.unwrap_or(crate_dir)
			.to_path_buf();
		let mut variant = match spec.preset {
			Some(preset) => {
				let path = found.ok_or_else(|| {
					Error::new(format!(
						"preset '{preset}' needs an aexlo.toml in {} or its parents",
						crate_dir.display()
					))
				})?;
				let manifest = manifest::Manifest::load(&path)?;
				let variants = manifest.preset_variants(preset)?;
				// The preset's own variant at this depth and render path, if
				// it has one; otherwise its first, retargeted.
				let matching = variants.iter().find(|v| {
					spec.depth.is_none_or(|d| v.depth == d) && spec.render.is_none_or(|r| v.render.name() == r)
				});
				matching
					.or(variants.first())
					.cloned()
					.ok_or_else(|| Error::new(format!("preset '{preset}' has no variants")))?
			}
			None => Variant::standalone(spec.name, in_process("EffectMain")),
		};
		if let Some(depth) = spec.depth {
			variant.depth = depth;
		}
		if let Some(render) = spec.render {
			variant.render = RenderMode::parse(render)?;
		}
		Ok((variant, root))
	}

	/// Run one expanded `#[aexlo::test]`: load the in-process entry point,
	/// configure it as the variant says, then run the body. Panics with the
	/// failure, as a `#[test]` should.
	pub fn run_test<R: TestOutcome>(spec: TestSpec, body: impl FnOnce(&mut PluginInstance) -> R) {
		let result = (|| -> Result {
			let (variant, root) = resolve(&spec)?;
			// SAFETY: the address is the crate's own `extern "C"` entry point.
			let mut fx = unsafe { aexlo::Host::get().from_entry_raw(spec.entry) }?;
			if let Some(reason) = apply::unsupported(&fx, &variant) {
				eprintln!("aexlo-test: {} skipped: {reason}", spec.name);
				return Ok(());
			}
			apply::configure(&mut fx, &variant)?;
			CONTEXT.with(|c| {
				*c.borrow_mut() = Some(Context {
					name: spec.name.to_string(),
					golden_suffix: spec.golden_suffix.to_string(),
					root,
					variant,
				})
			});
			let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(&mut fx).into_result()));
			CONTEXT.with(|c| *c.borrow_mut() = None);
			match outcome {
				Ok(result) => result,
				Err(panic) => std::panic::resume_unwind(panic),
			}
		})();
		if let Err(err) = result {
			panic!("{}: {err}", spec.name);
		}
	}

	/// Run one `#[aexlo::preview]`: drive the entry point through `body`,
	/// write the frame to `target/aexlo-preview/`, and surface it as
	/// `AEXLO_PREVIEW` asks.
	pub fn preview(
		entry: usize,
		crate_dir: &str,
		module_path: &str,
		name: &str,
		body: impl FnOnce(&mut PluginInstance),
	) -> Result {
		use aexlo_harness::preview::{PreviewMode, ensure_live_viewer, open_in_viewer, preview_mode, preview_path};
		// SAFETY: the address is the crate's own `extern "C"` entry point.
		let mut fx = unsafe { aexlo::Host::get().from_entry_raw(entry) }?;
		body(&mut fx);
		let path = preview_path(crate_dir, module_path, name);
		aexlo_harness::preview::save_preview(&fx, &path)?;
		eprintln!("aexlo::preview: wrote {}", path.display());
		// AEXLO_PREVIEW: unset = save only, `live` = keep an `aexlo view`
		// window updated (pair with a re-runner like `bacon`), else open once.
		match preview_mode() {
			PreviewMode::Off => {}
			PreviewMode::Once => open_in_viewer(&path)?,
			PreviewMode::Live => ensure_live_viewer(&path)?,
		}
		Ok(())
	}
}
