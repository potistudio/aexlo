use after_effects_sys::*;
use std::ptr::null_mut;

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

	pub fn build(self) -> PF_LayerDef {
		self.layer
	}
}
