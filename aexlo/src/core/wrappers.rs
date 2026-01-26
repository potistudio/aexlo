//! Newtype wrappers for After Effects low-level types.
//!
//! These wrappers provide safe, ergonomic APIs around the raw FFI types.

use after_effects_sys::*;
use std::ptr::null_mut;

// ============================================================================
// InData Wrapper
// ============================================================================

/// Wrapper around `PF_InData` providing safe initialization and access.
pub struct InData {
	inner: PF_InData,
}

impl InData {
	/// Create a new `InData` with sensible defaults.
	pub fn new(width: i32, height: i32) -> Self {
		let mut inner = unsafe { std::mem::zeroed::<PF_InData>() };

		// Version info
		inner.version = PF_SpecVersion {
			major: 13,
			minor: 28,
		};

		// Application identifiers
		inner.serial_num = -2147483648;
		inner.appl_id = 1180193859;

		// CPU/FPU info
		inner.what_cpu = 3;
		inner.what_fpu = 0;

		// Time settings
		inner.time_step = 1024;
		inner.local_time_step = 0;
		inner.time_scale = 0;
		inner.current_time = 0;
		inner.total_time = 0;

		// Field rendering
		inner.field = PF_Field_UPPER as PF_Field;
		inner.shutter_angle = 0;
		inner.shutter_phase = 0;

		// Dimensions
		inner.width = width;
		inner.height = height;
		inner.extent_hint = PF_UnionableRect {
			left: 0,
			top: 0,
			right: width,
			bottom: height,
		};

		// Output origin
		inner.output_origin_x = 0;
		inner.output_origin_y = 0;

		// Rational scales (must be non-zero denominator)
		inner.downsample_x = PF_RationalScale { num: 1, den: 1 };
		inner.downsample_y = PF_RationalScale { num: 1, den: 1 };
		inner.pixel_aspect_ratio = PF_RationalScale { num: 1, den: 1 };

		// Flags
		inner.in_flags = PF_InFlag_NONE as PF_InFlags;

		// Quality
		inner.quality = PF_Quality_HI;

		// Pointers (will be set externally)
		inner.utils = null_mut();
		inner.pica_basicP = null_mut();
		inner.effect_ref = null_mut();
		inner.global_data = null_mut();
		inner.sequence_data = null_mut();
		inner.frame_data = null_mut();

		// Sound (zeroed)
		inner.src_snd = unsafe { std::mem::zeroed() };

		Self { inner }
	}

	/// Set the interact callbacks.
	pub fn set_interact_callbacks(&mut self, callbacks: PF_InteractCallbacks) {
		self.inner.inter = callbacks;
	}

	/// Set the utility callbacks pointer.
	pub fn set_utils(&mut self, utils: *mut _PF_UtilCallbacks) {
		self.inner.utils = utils;
	}

	/// Set the PICA basic suite pointer.
	pub fn set_pica(&mut self, pica: *mut SPBasicSuite) {
		self.inner.pica_basicP = pica;
	}

	/// Set the effect reference pointer.
	pub fn set_effect_ref(&mut self, effect_ref: PF_ProgPtr) {
		self.inner.effect_ref = effect_ref;
	}

	/// Set the current time.
	pub fn set_current_time(&mut self, time: i32, time_scale: u32) {
		self.inner.current_time = time;
		self.inner.time_scale = time_scale;
	}

	/// Get the width.
	pub fn width(&self) -> i32 {
		self.inner.width
	}

	/// Get the height.
	pub fn height(&self) -> i32 {
		self.inner.height
	}

	/// Get a mutable reference to the inner `PF_InData`.
	pub fn as_mut(&mut self) -> &mut PF_InData {
		&mut self.inner
	}

	/// Get an immutable reference to the inner `PF_InData`.
	pub fn as_ref(&self) -> &PF_InData {
		&self.inner
	}

	/// Consume the wrapper and return the inner `PF_InData`.
	pub fn into_inner(self) -> PF_InData {
		self.inner
	}
}

// ============================================================================
// OutData Wrapper
// ============================================================================

/// Wrapper around `PF_OutData` providing safe initialization and access.
pub struct OutData {
	inner: PF_OutData,
}

impl OutData {
	/// Create a new `OutData` with sensible defaults.
	pub fn new() -> Self {
		let mut inner = unsafe { std::mem::zeroed::<PF_OutData>() };

		// Sound defaults
		inner.dest_snd = PF_SoundWorld {
			fi: PF_SoundFormatInfo {
				rateF: 44100.0,
				num_channels: 2,
				format: 16,
				sample_size: 1024,
			},
			num_samples: 1024,
			dataP: null_mut(),
		};

		// Flags
		inner.out_flags = PF_OutFlag_NONE as PF_OutFlags;
		inner.out_flags2 = PF_OutFlag2_NONE as PF_OutFlags2;

		Self { inner }
	}

	/// Set the output size.
	pub fn set_size(&mut self, width: i32, height: i32) {
		self.inner.width = width;
		self.inner.height = height;
	}

	/// Get the plugin name from the return message.
	pub fn name(&self) -> &[i8; 32] {
		&self.inner.name
	}

	/// Get the return message.
	pub fn return_msg(&self) -> &[i8; 256] {
		&self.inner.return_msg
	}

	/// Get a mutable reference to the inner `PF_OutData`.
	pub fn as_mut(&mut self) -> &mut PF_OutData {
		&mut self.inner
	}

	/// Get an immutable reference to the inner `PF_OutData`.
	pub fn as_ref(&self) -> &PF_OutData {
		&self.inner
	}

	/// Consume the wrapper and return the inner `PF_OutData`.
	pub fn into_inner(self) -> PF_OutData {
		self.inner
	}
}

impl Default for OutData {
	fn default() -> Self {
		Self::new()
	}
}

// ============================================================================
// LayerDef (EffectWorld) Wrapper
// ============================================================================

/// Wrapper around `PF_LayerDef` (aka `PF_EffectWorld`) providing safe initialization.
pub struct LayerDef {
	inner: PF_LayerDef,
}

impl LayerDef {
	/// Create a new `LayerDef` with the specified dimensions.
	/// Note: The buffer pointer must be set separately via `set_buffer()`.
	pub fn new(width: i32, height: i32) -> Self {
		let mut inner = unsafe { std::mem::zeroed::<PF_LayerDef>() };

		inner.width = width;
		inner.height = height;
		inner.rowbytes = width * 4; // Assuming 8-bit ARGB (4 bytes per pixel)

		// Pixel aspect ratio (must be non-zero denominator)
		inner.pix_aspect_ratio = PF_RationalScale { num: 1, den: 1 };

		Self { inner }
	}

	/// Set the pixel buffer pointer.
	pub fn set_buffer(&mut self, buffer: *mut PF_Pixel) {
		self.inner.data = buffer;
	}

	/// Set the extent hint rectangle.
	pub fn set_extent_hint(&mut self, left: i32, top: i32, right: i32, bottom: i32) {
		self.inner.extent_hint = PF_UnionableRect {
			left,
			top,
			right,
			bottom,
		};
	}

	/// Get the width.
	pub fn width(&self) -> i32 {
		self.inner.width
	}

	/// Get the height.
	pub fn height(&self) -> i32 {
		self.inner.height
	}

	/// Get the row bytes (stride).
	pub fn rowbytes(&self) -> i32 {
		self.inner.rowbytes
	}

	/// Get the pixel buffer pointer.
	pub fn data(&self) -> *mut PF_Pixel {
		self.inner.data
	}

	/// Get a mutable reference to the inner `PF_LayerDef`.
	pub fn as_mut(&mut self) -> &mut PF_LayerDef {
		&mut self.inner
	}

	/// Get an immutable reference to the inner `PF_LayerDef`.
	pub fn as_ref(&self) -> &PF_LayerDef {
		&self.inner
	}

	/// Consume the wrapper and return the inner `PF_LayerDef`.
	pub fn into_inner(self) -> PF_LayerDef {
		self.inner
	}
}
