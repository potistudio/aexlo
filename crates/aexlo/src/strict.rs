//! Strict mode: host-side instrumentation that reports plugin bugs After
//! Effects would hide (see [`PluginInstance::set_strict`]).
//!
//! The host only detects and records; whether a finding fails anything is
//! up to the caller.
//!
//! [`PluginInstance::set_strict`]: crate::PluginInstance::set_strict

use wrapper::{AnyLayer, Layer, PixelDepth};

/// Which strict-mode instrumentation an instance runs. All off by default,
/// in which case the host's allocation paths and costs are unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Strict {
	/// Fill output worlds with a sentinel before each render, so pixels the
	/// plugin never wrote can be found afterwards ([`unwritten_pixels`]).
	pub poison_output: bool,
	/// Surround host-allocated worlds with guard bands, verified on dispose.
	pub guard_bands: bool,
	/// Count handle and world allocations against disposals.
	pub track_allocations: bool,
	/// Record checkout/checkin of layers and params in smart render.
	pub track_checkouts: bool,
}

impl Strict {
	/// Every feature on.
	pub fn all() -> Self {
		Self {
			poison_output: true,
			guard_bands: true,
			track_allocations: true,
			track_checkouts: true,
		}
	}

	/// Whether any feature is on.
	pub fn any(&self) -> bool {
		self.poison_output || self.guard_bands || self.track_allocations || self.track_checkouts
	}
}

/// The 8 and 16 bpc output poison: every byte of the pixel is `0xCD`.
pub const POISON_BYTE: u8 = 0xCD;

/// The 32 bpc output poison: a signalling NaN with a recognizable payload.
pub const POISON_F32_BITS: u32 = 0x7FA0_CDCD;

/// The poison value of one channel at depth `D`.
fn poison_channel<D: PixelDepth>() -> D::Depth {
	let mut value = D::Depth::default();
	// SAFETY: `D::Depth` is `u8`, `u16` or `f32`; every bit pattern is valid.
	unsafe {
		let bytes = std::slice::from_raw_parts_mut(&mut value as *mut D::Depth as *mut u8, size_of::<D::Depth>());
		if size_of::<D::Depth>() == 4 {
			bytes.copy_from_slice(&POISON_F32_BITS.to_ne_bytes());
		} else {
			bytes.fill(POISON_BYTE);
		}
	}
	value
}

/// Whether `value` is the poison channel at depth `D`, bit for bit.
fn is_poison_channel<D: PixelDepth>(value: &D::Depth) -> bool {
	let poison = poison_channel::<D>();
	// SAFETY: both are plain `size_of::<D::Depth>()`-byte values.
	unsafe {
		std::slice::from_raw_parts(value as *const D::Depth as *const u8, size_of::<D::Depth>())
			== std::slice::from_raw_parts(&poison as *const D::Depth as *const u8, size_of::<D::Depth>())
	}
}

/// Fill every channel of `layer` with the poison.
pub(crate) fn poison(layer: &mut AnyLayer) {
	fn fill<D: PixelDepth>(layer: &mut Layer<D>) {
		let value = poison_channel::<D>();
		for pixel in layer.iter_mut() {
			pixel.alpha = value;
			pixel.red = value;
			pixel.green = value;
			pixel.blue = value;
		}
	}
	match layer {
		AnyLayer::U8(l) => fill(l),
		AnyLayer::U16(l) => fill(l),
		AnyLayer::F32(l) => fill(l),
	}
}

/// Whether the pixel at linear `index` still holds the output poison.
///
/// At 8 bpc all four channels must be poisoned (`0xCD` is a legal value);
/// at 16 bpc (`0xCDCD` is above AE's 32768) and 32 bpc (a signalling NaN)
/// any one channel is enough.
pub fn is_unwritten(layer: &AnyLayer, index: usize) -> bool {
	fn check<D: PixelDepth>(layer: &Layer<D>, index: usize, all: bool) -> bool {
		let Some(p) = layer.get_linear(index) else {
			return false;
		};
		let channels = [&p.alpha, &p.red, &p.green, &p.blue];
		if all {
			channels.iter().all(|c| is_poison_channel::<D>(c))
		} else {
			channels.iter().any(|c| is_poison_channel::<D>(c))
		}
	}
	match layer {
		AnyLayer::U8(l) => check(l, index, true),
		AnyLayer::U16(l) => check(l, index, false),
		AnyLayer::F32(l) => check(l, index, false),
	}
}

/// How many pixels of a poisoned output the plugin left unwritten.
pub fn unwritten_pixels(layer: &AnyLayer) -> usize {
	let count = layer.width() as usize * layer.height() as usize;
	(0..count).filter(|&i| is_unwritten(layer, i)).count()
}

/// A guard band around a host world was overwritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardViolation {
	/// What the world was (`"PF_NewWorld 64x48"`, `"output"`, ...).
	pub world: String,
	/// Where: `"above"`, `"below"` or `"row tail"`.
	pub band: &'static str,
	/// Bytes found changed.
	pub bytes: usize,
}

/// A handle or world still alive when its scope ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leak {
	pub kind: crate::AllocationKind,
	pub address: usize,
	pub bytes: usize,
	/// The command during which it was allocated.
	pub allocated_in: &'static str,
	/// Where it should have been released by: `"SEQUENCE_SETDOWN"`, ...
	pub scope: &'static str,
}

/// A smart-render checkout rule broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutViolation {
	pub message: String,
}

/// What strict mode found since the last [`PluginInstance::strict_report`].
///
/// [`PluginInstance::strict_report`]: crate::PluginInstance::strict_report
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StrictReport {
	pub guard_violations: Vec<GuardViolation>,
	pub leaks: Vec<Leak>,
	pub checkout_violations: Vec<CheckoutViolation>,
}

impl StrictReport {
	pub fn is_empty(&self) -> bool {
		self.guard_violations.is_empty() && self.leaks.is_empty() && self.checkout_violations.is_empty()
	}
}

//==== Tracking =========================================================

/// Rows of guard band above and below a guarded world.
pub(crate) const GUARD_ROWS: usize = 4;
/// Pixels of guard band at the end of every row (the `rowbytes` tail).
pub(crate) const GUARD_TAIL_PIXELS: usize = 4;
/// The bytes guard bands are filled with, repeated.
const GUARD_PATTERN: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

/// A pixel buffer with guard bands: [`GUARD_ROWS`] rows above and below the
/// image, and a [`GUARD_TAIL_PIXELS`] tail on every row, all filled with a
/// pattern that [`Self::verify`] checks is intact.
pub(crate) struct Guarded {
	/// 8-byte aligned storage (`f32` pixels need 4).
	storage: Vec<u64>,
	/// Bytes of image data per row.
	row_data: usize,
	/// Bytes per row including the tail: the world's `rowbytes`.
	rowbytes: usize,
	height: usize,
}

impl Guarded {
	/// A guarded buffer for `height` rows of `row_data` bytes; the image area
	/// starts zeroed.
	pub(crate) fn new(row_data: usize, height: usize, bytes_per_pixel: usize) -> Self {
		let rowbytes = row_data + GUARD_TAIL_PIXELS * bytes_per_pixel;
		let total = rowbytes * (height + 2 * GUARD_ROWS);
		let mut guarded = Self {
			storage: vec![0u64; total.div_ceil(8)],
			row_data,
			rowbytes,
			height,
		};
		let bytes = guarded.bytes_mut();
		for (i, byte) in bytes.iter_mut().enumerate() {
			*byte = GUARD_PATTERN[i % 4];
		}
		for row in 0..height {
			let start = (GUARD_ROWS + row) * rowbytes;
			bytes[start..start + row_data].fill(0);
		}
		guarded
	}

	fn bytes(&self) -> &[u8] {
		// SAFETY: plain bytes of the `u64` storage.
		unsafe { std::slice::from_raw_parts(self.storage.as_ptr() as *const u8, self.storage.len() * 8) }
	}

	fn bytes_mut(&mut self) -> &mut [u8] {
		// SAFETY: plain bytes of the `u64` storage.
		unsafe { std::slice::from_raw_parts_mut(self.storage.as_mut_ptr() as *mut u8, self.storage.len() * 8) }
	}

	/// The first image pixel: what the world's `data` points at.
	pub(crate) fn data_ptr(&mut self) -> *mut u8 {
		let offset = GUARD_ROWS * self.rowbytes;
		// SAFETY: within the storage, which holds all rows.
		unsafe { (self.storage.as_mut_ptr() as *mut u8).add(offset) }
	}

	pub(crate) fn rowbytes(&self) -> usize {
		self.rowbytes
	}

	/// Copy tightly packed rows (`row_data` bytes each) into the image.
	pub(crate) fn load_rows(&mut self, src: &[u8]) {
		let (row_data, rowbytes) = (self.row_data, self.rowbytes);
		let bytes = self.bytes_mut();
		for (row, chunk) in src.chunks(row_data).enumerate() {
			let start = (GUARD_ROWS + row) * rowbytes;
			bytes[start..start + chunk.len()].copy_from_slice(chunk);
		}
	}

	/// Copy the image out into tightly packed rows.
	pub(crate) fn store_rows(&self, dst: &mut [u8]) {
		let bytes = self.bytes();
		for (row, chunk) in dst.chunks_mut(self.row_data).enumerate() {
			let start = (GUARD_ROWS + row) * self.rowbytes;
			chunk.copy_from_slice(&bytes[start..start + chunk.len()]);
		}
	}

	/// Overwritten guard bytes, per band: `(band, bytes changed)`.
	///
	/// Only the rows above and below count: a row's tail is inside the
	/// world's `rowbytes`, which plugins may legitimately clear or scribble
	/// on. The tail is there so code assuming `rowbytes == width * bpp`
	/// renders visibly wrong instead of happening to work.
	pub(crate) fn verify(&self) -> Vec<(&'static str, usize)> {
		let bytes = self.bytes();
		let changed = |range: std::ops::Range<usize>| {
			range
				.filter(|&i| i < bytes.len() && bytes[i] != GUARD_PATTERN[i % 4])
				.count()
		};
		let above = changed(0..GUARD_ROWS * self.rowbytes);
		let below_start = (GUARD_ROWS + self.height) * self.rowbytes;
		let below = changed(below_start..below_start + GUARD_ROWS * self.rowbytes);
		[("above", above), ("below", below)]
			.into_iter()
			.filter(|(_, n)| *n > 0)
			.collect()
	}
}

/// An allocation the plugin holds.
struct Live {
	kind: crate::AllocationKind,
	bytes: usize,
	command: &'static str,
}

/// Commands whose worlds must be gone by the time they return.
const RENDER_COMMANDS: &[&str] = &["RENDER", "SMART_RENDER", "SMART_RENDER_GPU"];

/// Per-instance strict-mode bookkeeping, reachable from host callbacks
/// through a thread-local while the instance dispatches a command.
#[derive(Default)]
pub(crate) struct Tracker {
	pub(crate) strict: Strict,
	pub(crate) observer: Option<std::sync::Arc<dyn crate::Observer>>,
	command: &'static str,
	live: std::collections::HashMap<usize, Live>,
	/// Checkout ids declared by the last `SMART_PRE_RENDER`.
	declared: std::collections::HashSet<i32>,
	/// Layer checkout ids checked out in the current `SMART_RENDER`.
	layers_out: std::collections::HashSet<i32>,
	/// Layer params checked out and not yet checked in: `(def address, index,
	/// command)`.
	params_out: Vec<(usize, i32, &'static str)>,
	pub(crate) report: StrictReport,
}

impl Tracker {
	/// Note that `command` begins; returns the one it interrupts (commands
	/// nest when setup triggers an arbitrary-data callback).
	pub(crate) fn begin(&mut self, command: &'static str) -> &'static str {
		if command == "SMART_PRE_RENDER" {
			self.declared.clear();
			self.report_params(command);
		}
		std::mem::replace(&mut self.command, command)
	}

	/// Note that `command` returned, judging what it left behind.
	pub(crate) fn end(&mut self, command: &'static str, resumed: &'static str) {
		if self.strict.track_allocations && RENDER_COMMANDS.contains(&command) {
			let stale: Vec<usize> = self
				.live
				.iter()
				.filter(|(_, l)| l.kind == crate::AllocationKind::World && l.command == command)
				.map(|(addr, _)| *addr)
				.collect();
			for address in stale {
				if let Some(live) = self.live.remove(&address) {
					self.report.leaks.push(Leak {
						kind: live.kind,
						address,
						bytes: live.bytes,
						allocated_in: live.command,
						scope: command,
					});
				}
			}
		}
		if command.starts_with("SMART_RENDER") {
			// Checking layer pixels back in is optional (the SDK: "not
			// strictly necessary"); the host does it when the command ends.
			self.layers_out.clear();
		}
		// Layer params checked out in pre-render may be checked in by the
		// render that follows; anywhere else, by the end of the command.
		if command != "SMART_PRE_RENDER" {
			self.report_params(command);
		}
		self.command = resumed;
	}

	/// Report layer params still checked out as `command` ends.
	fn report_params(&mut self, command: &'static str) {
		if !self.strict.track_checkouts {
			self.params_out.clear();
			return;
		}
		for (_, index, from) in std::mem::take(&mut self.params_out) {
			let place = if from == command {
				format!("PF_Cmd_{command}")
			} else {
				format!("PF_Cmd_{from} (still out after PF_Cmd_{command})")
			};
			self.report.checkout_violations.push(CheckoutViolation {
				message: format!("{place} checked out layer param #{index} and never checked it in"),
			});
		}
	}

	/// Everything still held once the plugin is torn down.
	pub(crate) fn torn_down(&mut self) {
		if !self.strict.track_allocations {
			return;
		}
		let mut leaks: Vec<Leak> = self
			.live
			.drain()
			.map(|(address, live)| Leak {
				kind: live.kind,
				address,
				bytes: live.bytes,
				allocated_in: live.command,
				scope: "GLOBAL_SETDOWN",
			})
			.collect();
		leaks.sort_by_key(|l| l.address);
		self.report.leaks.extend(leaks);
	}

	fn allocated(&mut self, kind: crate::AllocationKind, address: usize, bytes: usize) {
		if self.strict.track_allocations {
			self.live.insert(
				address,
				Live {
					kind,
					bytes,
					command: self.command,
				},
			);
		}
		if let Some(observer) = &self.observer {
			observer.allocation(&crate::AllocationEvent {
				kind,
				allocated: true,
				address,
				bytes,
			});
		}
	}

	fn disposed(&mut self, kind: crate::AllocationKind, address: usize) {
		let bytes = self.live.remove(&address).map_or(0, |l| l.bytes);
		if let Some(observer) = &self.observer {
			observer.allocation(&crate::AllocationEvent {
				kind,
				allocated: false,
				address,
				bytes,
			});
		}
	}

	fn checkout(&mut self, kind: crate::CheckoutKind, id: i32, address: usize) {
		use crate::CheckoutKind as K;
		if let Some(observer) = &self.observer {
			observer.checkout(&crate::CheckoutEvent { kind, id });
		}
		if !self.strict.track_checkouts {
			return;
		}
		let command = self.command;
		let mut violation = |message: String| self.report.checkout_violations.push(CheckoutViolation { message });
		match kind {
			K::DeclareLayer => {
				self.declared.insert(id);
			}
			K::CheckoutLayerPixels => {
				if !self.declared.contains(&id) {
					violation(format!(
						"PF_Cmd_{command} checked out layer {id}, which SMART_PRE_RENDER never declared"
					));
				}
				if !self.layers_out.insert(id) {
					violation(format!("PF_Cmd_{command} checked out layer {id} twice"));
				}
			}
			K::CheckinLayerPixels => {
				if !self.layers_out.remove(&id) {
					violation(format!(
						"PF_Cmd_{command} checked in layer {id}, which was not checked out"
					));
				}
			}
			K::CheckoutParam => self.params_out.push((address, id, command)),
			// Plugins may move the def between checkout and checkin, so match
			// by address when possible, else the latest checkout.
			K::CheckinParam => {
				let found = self
					.params_out
					.iter()
					.position(|(a, _, _)| *a == address)
					.or_else(|| self.params_out.len().checked_sub(1));
				match found {
					Some(i) => {
						self.params_out.remove(i);
					}
					None => violation(format!(
						"PF_Cmd_{command} checked in a layer param it never checked out"
					)),
				}
			}
		}
	}
}

/// A tracker shared between an instance and the thread dispatching it.
pub(crate) type SharedTracker = std::sync::Arc<std::sync::Mutex<Tracker>>;

thread_local! {
	static ACTIVE: std::cell::RefCell<Option<SharedTracker>> = const { std::cell::RefCell::new(None) };
}

/// Make `tracker` the one host callbacks on this thread report to until the
/// guard drops.
pub(crate) fn enter(tracker: Option<SharedTracker>) -> TrackGuard {
	TrackGuard {
		previous: ACTIVE.with(|a| std::mem::replace(&mut *a.borrow_mut(), tracker)),
	}
}

pub(crate) struct TrackGuard {
	previous: Option<SharedTracker>,
}

impl Drop for TrackGuard {
	fn drop(&mut self) {
		let previous = self.previous.take();
		ACTIVE.with(|a| *a.borrow_mut() = previous);
	}
}

fn with_active<R>(f: impl FnOnce(&mut Tracker) -> R) -> Option<R> {
	ACTIVE.with(|a| {
		let active = a.borrow();
		let tracker = active.as_ref()?;
		let mut tracker = tracker.lock().ok()?;
		Some(f(&mut tracker))
	})
}

/// The plugin got memory from the host.
pub(crate) fn allocated(kind: crate::AllocationKind, address: usize, bytes: usize) {
	with_active(|t| t.allocated(kind, address, bytes));
}

/// The plugin gave memory back.
pub(crate) fn disposed(kind: crate::AllocationKind, address: usize) {
	with_active(|t| t.disposed(kind, address));
}

/// The plugin checked something out or in.
pub(crate) fn checkout(kind: crate::CheckoutKind, id: i32, address: usize) {
	with_active(|t| t.checkout(kind, id, address));
}

/// Whether worlds allocated now should get guard bands.
pub(crate) fn guard_bands() -> bool {
	with_active(|t| t.strict.guard_bands).unwrap_or(false)
}

/// Record overwritten guard bands of `world`.
pub(crate) fn guards_broken(world: &str, bands: Vec<(&'static str, usize)>) {
	with_active(|t| {
		for (band, bytes) in bands {
			t.report.guard_violations.push(GuardViolation {
				world: world.to_string(),
				band,
				bytes,
			});
		}
	});
}

#[cfg(test)]
mod tests {
	use super::*;
	use wrapper::PixelDepthKind;

	#[test]
	fn poison_is_found_at_every_depth_and_only_where_left() {
		for kind in [PixelDepthKind::U8, PixelDepthKind::U16, PixelDepthKind::F32] {
			let mut layer = AnyLayer::filled(kind, 4, 2, [0.0; 4]);
			poison(&mut layer);
			assert_eq!(unwritten_pixels(&layer), 8, "{kind}");
			fn write<D: PixelDepth>(layer: &mut AnyLayer) {
				let layer = layer.get_mut::<D>().unwrap();
				for pixel in layer.iter_mut().take(3) {
					*pixel = wrapper::Pixel::from_unit([0.2, 0.4, 0.6, 1.0]);
				}
			}
			match kind {
				PixelDepthKind::U8 => write::<wrapper::Depth8>(&mut layer),
				PixelDepthKind::U16 => write::<wrapper::Depth16>(&mut layer),
				PixelDepthKind::F32 => write::<wrapper::Depth32>(&mut layer),
			}
			assert_eq!(unwritten_pixels(&layer), 5, "{kind}");
		}
	}

	#[test]
	fn guard_bands_catch_writes_outside_the_image() {
		let mut g = Guarded::new(8, 3, 4);
		assert_eq!(g.rowbytes(), 8 + 16);
		assert!(g.verify().is_empty());
		g.load_rows(&[7u8; 24]);
		let mut out = [0u8; 24];
		g.store_rows(&mut out);
		assert_eq!(out, [7u8; 24]);
		assert!(g.verify().is_empty());

		// One byte past the end of the first row, and one row past the last.
		let data = g.data_ptr();
		unsafe {
			data.add(8).write(0);
			std::ptr::write_bytes(data.add(3 * 24), 0, 24);
		}
		assert_eq!(g.verify(), [("below", 24)], "the row tail is the world's own");
	}

	#[test]
	fn trackers_judge_leaks_and_checkouts_by_command() {
		use crate::{AllocationKind, CheckoutKind};
		let tracker: SharedTracker = Default::default();
		tracker.lock().unwrap().strict = Strict::all();
		let _guard = enter(Some(tracker.clone()));
		let step = |name: &'static str, f: &dyn Fn()| {
			let resumed = tracker.lock().unwrap().begin(name);
			f();
			tracker.lock().unwrap().end(name, resumed);
		};
		step("SMART_PRE_RENDER", &|| checkout(CheckoutKind::DeclareLayer, 0, 0));
		step("SMART_RENDER", &|| {
			checkout(CheckoutKind::CheckoutLayerPixels, 0, 0);
			checkout(CheckoutKind::CheckoutLayerPixels, 5, 0);
			checkout(CheckoutKind::CheckinLayerPixels, 0, 0);
			checkout(CheckoutKind::CheckinLayerPixels, 0, 0);
			checkout(CheckoutKind::CheckoutParam, 2, 0x10);
			allocated(AllocationKind::World, 0x100, 64);
			allocated(AllocationKind::Handle, 0x200, 8);
			allocated(AllocationKind::Handle, 0x300, 8);
			disposed(AllocationKind::Handle, 0x300);
		});
		let mut t = tracker.lock().unwrap();
		t.torn_down();
		let messages: Vec<&str> = t
			.report
			.checkout_violations
			.iter()
			.map(|v| v.message.as_str())
			.collect();
		assert_eq!(
			messages,
			[
				"PF_Cmd_SMART_RENDER checked out layer 5, which SMART_PRE_RENDER never declared",
				"PF_Cmd_SMART_RENDER checked in layer 0, which was not checked out",
				"PF_Cmd_SMART_RENDER checked out layer param #2 and never checked it in",
			]
		);
		let leaks: Vec<(usize, &str)> = t.report.leaks.iter().map(|l| (l.address, l.scope)).collect();
		assert_eq!(leaks, [(0x100, "SMART_RENDER"), (0x200, "GLOBAL_SETDOWN")]);
	}

	#[test]
	fn float_poison_is_a_signalling_nan() {
		let value = f32::from_bits(POISON_F32_BITS);
		assert!(value.is_nan());
		assert_eq!(POISON_F32_BITS & 0x0040_0000, 0, "quiet bit clear");
	}
}
