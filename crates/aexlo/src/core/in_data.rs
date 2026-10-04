//! The pieces `PF_InData` is composed from, split by lifetime.
//!
//! After Effects hands the plugin a fresh `PF_InData` on every command. Rather
//! than keeping one mutable struct around, the host keeps its three sources
//! separately and assembles the struct right before each call:
//!
//! - [`HostInfo`]: the host's identity (version, app id, callbacks); immutable.
//! - [`EffectState`]: plugin-owned state carried across commands (`global_data`,
//!   `sequence_data`).
//! - [`RenderContext`]: the frame being processed (size, time, downsampling).

use crate::core::constants::{DEFAULT_HEIGHT, DEFAULT_WIDTH};
use after_effects_sys::*;
use std::ptr::null_mut;

const AE_VERSION_26_3_0: PF_SpecVersion = PF_SpecVersion { major: 13, minor: 29 };

/// Values that describe the host itself.
///
/// Fields are private and there are no setters: once built, the only thing
/// outside this module can do is hand it to [`compose`].
#[derive(Clone, Copy)]
pub(crate) struct HostInfo {
	version: PF_SpecVersion,
	serial_num: A_long,
	appl_id: A_long,
	what_cpu: A_long,
	inter: PF_InteractCallbacks,
}

impl HostInfo {
	pub fn new(inter: PF_InteractCallbacks) -> Self {
		Self {
			version: AE_VERSION_26_3_0,
			serial_num: -2147483648,
			appl_id: 1180193859,
			what_cpu: 3,
			inter,
		}
	}
}

/// Plugin-allocated handles that persist across commands.
///
/// Only the plugin (via [`EffectState::absorb`]) or an explicit
/// [`EffectState::reset_sequence`] can change them.
#[derive(Clone, Copy)]
pub(crate) struct EffectState {
	global_data: PF_Handle,
	sequence_data: PF_Handle,
}

impl Default for EffectState {
	fn default() -> Self {
		Self {
			global_data: null_mut(),
			sequence_data: null_mut(),
		}
	}
}

impl EffectState {
	/// Adopt any handle the plugin returned in `out_data`.
	pub fn absorb(&mut self, out_data: &PF_OutData) {
		if !out_data.global_data.is_null() {
			self.global_data = out_data.global_data;
		}
		if !out_data.sequence_data.is_null() {
			self.sequence_data = out_data.sequence_data;
		}
	}

	/// Drop the sequence handle so the next `SEQUENCE_SETUP` allocates from scratch.
	pub fn reset_sequence(&mut self) {
		self.sequence_data = null_mut();
	}
}

/// The frame the next command operates on.
///
/// Changed only through its methods, so derived fields such as `extent_hint`
/// can't drift out of sync with the size.
#[derive(Clone, Copy)]
pub(crate) struct RenderContext {
	width: A_long,
	height: A_long,
	extent_hint: PF_UnionableRect,
	current_time: A_long,
	time_step: A_long,
	local_time_step: A_long,
	time_scale: A_u_long,
	field: PF_Field,
	downsample_x: PF_RationalScale,
	downsample_y: PF_RationalScale,
	pixel_aspect_ratio: PF_RationalScale,
}

impl Default for RenderContext {
	fn default() -> Self {
		let one = PF_RationalScale { num: 1, den: 1 };
		let mut ctx = Self {
			width: 0,
			height: 0,
			extent_hint: PF_UnionableRect {
				left: 0,
				top: 0,
				right: 0,
				bottom: 0,
			},
			current_time: 10240,
			time_step: 1024,
			local_time_step: 0,
			time_scale: 0,
			field: PF_Field_UPPER as PF_Field,
			// Rational scales default to 1/1 to avoid division by zero.
			downsample_x: one,
			downsample_y: one,
			pixel_aspect_ratio: one,
		};
		ctx.set_size(DEFAULT_WIDTH as i32, DEFAULT_HEIGHT as i32);
		ctx
	}
}

impl RenderContext {
	/// Frame size in pixels (width, height).
	#[cfg(test)]
	pub fn size(&self) -> (A_long, A_long) {
		(self.width, self.height)
	}

	/// Set the frame size and cover it entirely with `extent_hint`.
	pub fn set_size(&mut self, width: i32, height: i32) {
		self.width = width;
		self.height = height;
		self.extent_hint = PF_UnionableRect {
			left: 0,
			top: 0,
			right: width,
			bottom: height,
		};
	}
}

/// Pointers into the calling instance, valid only for the duration of one call.
pub(crate) struct CallBindings {
	pub effect_ref: PF_ProgPtr,
	pub utils: *mut _PF_UtilCallbacks,
	pub pica: *mut SPBasicSuite,
	pub num_params: A_long,
}

/// Assemble the `PF_InData` handed to the plugin for one command.
pub(crate) fn compose(host: &HostInfo, effect: &EffectState, render: &RenderContext, bindings: CallBindings) -> PF_InData {
	let mut in_data = unsafe { std::mem::zeroed::<PF_InData>() };

	in_data.version = host.version;
	in_data.serial_num = host.serial_num;
	in_data.appl_id = host.appl_id;
	in_data.what_cpu = host.what_cpu;
	in_data.inter = host.inter;

	in_data.global_data = effect.global_data;
	in_data.sequence_data = effect.sequence_data;

	in_data.width = render.width;
	in_data.height = render.height;
	in_data.extent_hint = render.extent_hint;
	in_data.current_time = render.current_time;
	in_data.time_step = render.time_step;
	in_data.local_time_step = render.local_time_step;
	in_data.time_scale = render.time_scale;
	in_data.field = render.field;
	in_data.downsample_x = render.downsample_x;
	in_data.downsample_y = render.downsample_y;
	in_data.pixel_aspect_ratio = render.pixel_aspect_ratio;

	in_data.effect_ref = bindings.effect_ref;
	in_data.utils = bindings.utils;
	in_data.pica_basicP = bindings.pica;
	in_data.num_params = bindings.num_params;

	in_data
}
