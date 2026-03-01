use after_effects_sys::*;
use std::ptr::null_mut;

pub struct InDataBuilder {
	in_data: PF_InData,
}

impl InDataBuilder {
	pub fn new() -> Self {
		let in_data = unsafe { std::mem::zeroed::<PF_InData>() };
		let mut builder = Self { in_data };

		// Set sensible defaults
		builder.in_data.version = PF_SpecVersion {
			major: 13,
			minor: 28,
		};
		builder.in_data.serial_num = -2147483648;
		builder.in_data.appl_id = 1180193859;
		builder.in_data.what_cpu = 3;
		builder.in_data.time_step = 1024;
		builder.in_data.field = PF_Field_UPPER as PF_Field;
		builder.in_data.width = 1920;
		builder.in_data.height = 1080;
		builder.in_data.local_time_step = 0;
		builder.in_data.time_scale = 0;

		// Rational scales should default to 1/1 to avoid division by zero
		builder.in_data.downsample_x = PF_RationalScale { num: 1, den: 1 };
		builder.in_data.downsample_y = PF_RationalScale { num: 1, den: 1 };
		builder.in_data.pixel_aspect_ratio = PF_RationalScale { num: 1, den: 1 };

		builder
	}

	pub fn with_size(mut self, width: i32, height: i32) -> Self {
		self.in_data.width = width;
		self.in_data.height = height;
		self.in_data.extent_hint = PF_UnionableRect {
			left: 0,
			top: 0,
			right: width,
			bottom: height,
		};
		self
	}

	pub fn with_callbacks(mut self, interact: PF_InteractCallbacks) -> Self {
		self.in_data.inter = interact;
		self
	}

	pub fn with_utils(mut self, utils: *mut _PF_UtilCallbacks) -> Self {
		self.in_data.utils = utils;
		self
	}

	pub fn with_pica(mut self, pica: *mut SPBasicSuite) -> Self {
		self.in_data.pica_basicP = pica;
		self
	}

	pub fn with_global_data(mut self, global_data: PF_Handle) -> Self {
		self.in_data.global_data = global_data;
		self
	}

	pub fn build(self) -> PF_InData {
		self.in_data
	}
}

pub struct OutDataBuilder {
	out_data: PF_OutData,
}

impl OutDataBuilder {
	pub fn new() -> Self {
		let mut out_data = unsafe { std::mem::zeroed::<PF_OutData>() };

		// Defaults
		out_data.dest_snd = PF_SoundWorld {
			fi: PF_SoundFormatInfo {
				rateF: 44100.0,
				num_channels: 2,
				format: 16,
				sample_size: 1024,
			},
			num_samples: 1024,
			dataP: null_mut(),
		};

		Self { out_data }
	}

	pub fn build(self) -> PF_OutData {
		self.out_data
	}
}

pub struct LayerDefBuilder {
	layer: PF_LayerDef,
}

impl LayerDefBuilder {
	pub fn new() -> Self {
		let mut layer = unsafe { std::mem::zeroed::<PF_LayerDef>() };
		layer.pix_aspect_ratio = PF_RationalScale { num: 1, den: 1 };
		Self { layer }
	}

	pub fn with_size(mut self, width: i32, height: i32) -> Self {
		self.layer.width = width;
		self.layer.height = height;
		self.layer.rowbytes = width * 4; // Assuming 8-bit ARGB
		self
	}

	pub fn with_buffer(mut self, buffer: *mut PF_Pixel) -> Self {
		self.layer.data = buffer as *mut _;
		self
	}

	pub fn build(self) -> PF_LayerDef {
		self.layer
	}
}
