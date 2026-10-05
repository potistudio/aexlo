//! `PF_ChannelSuite1` ("PF AE Channel Suite"): auxiliary layer channels
//! (depth, normals, object IDs, ...) from 3D file formats.
//!
//! aexlo's layers are plain 2D images, so every layer reports zero auxiliary
//! channels: lookups answer "not found" and there is nothing to check out.

use crate::core::diagnostics::diag;
use after_effects_sys::{
	A_long, A_u_long, PF_ChannelChunk, PF_ChannelDesc, PF_ChannelIndex, PF_ChannelRef, PF_ChannelRefPtr,
	PF_ChannelSuite1, PF_ChannelType, PF_DataType, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_ParamIndex,
	PF_ProgPtr,
};

unsafe extern "C" fn get_layer_channel_count(
	effect_ref: PF_ProgPtr,
	param_index: PF_ParamIndex,
	num_channelsPL: *mut A_long,
) -> PF_Err {
	diag!("PF_ChannelSuite1/PF_GetLayerChannelCount",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"param_index" => param_index,
		"num_channelsPL" => format!("{:#x}", num_channelsPL as usize),
	);

	let _ = (effect_ref, param_index);
	let Some(out) = (unsafe { num_channelsPL.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	*out = 0;
	PF_Err_NONE as PF_Err
}

/// Answer "not found", clearing the optional out-params.
///
/// # Safety
/// Each pointer must be null or writable.
unsafe fn not_found(
	foundPB: *mut after_effects_sys::PF_Boolean,
	refP: *mut PF_ChannelRef,
	descP: *mut PF_ChannelDesc,
) -> PF_Err {
	let Some(found) = (unsafe { foundPB.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	*found = 0;
	unsafe {
		if let Some(r) = refP.as_mut() {
			*r = std::mem::zeroed();
		}
		if let Some(d) = descP.as_mut() {
			*d = std::mem::zeroed();
		}
	}
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn get_layer_channel_indexed_ref_and_desc(
	effect_ref: PF_ProgPtr,
	param_index: PF_ParamIndex,
	channel_index: PF_ChannelIndex,
	foundPB: *mut after_effects_sys::PF_Boolean,
	channel_refP: *mut PF_ChannelRef,
	channel_descP: *mut PF_ChannelDesc,
) -> PF_Err {
	diag!("PF_ChannelSuite1/PF_GetLayerChannelIndexedRefAndDesc",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"param_index" => param_index,
		"channel_index" => channel_index,
	);
	let _ = (effect_ref, param_index, channel_index);
	unsafe { not_found(foundPB, channel_refP, channel_descP) }
}

unsafe extern "C" fn get_layer_channel_typed_ref_and_desc(
	effect_ref: PF_ProgPtr,
	param_index: PF_ParamIndex,
	channel_type: PF_ChannelType,
	foundPB: *mut after_effects_sys::PF_Boolean,
	channel_refP: *mut PF_ChannelRef,
	channel_descP: *mut PF_ChannelDesc,
) -> PF_Err {
	diag!("PF_ChannelSuite1/PF_GetLayerChannelTypedRefAndDesc",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"param_index" => param_index,
		"channel_type" => format!("{:#x}", channel_type),
	);
	let _ = (effect_ref, param_index, channel_type);
	unsafe { not_found(foundPB, channel_refP, channel_descP) }
}

/// No channel ref can be valid, since none was ever handed out.
unsafe extern "C" fn checkout_layer_channel(
	effect_ref: PF_ProgPtr,
	channel_refP: PF_ChannelRefPtr,
	what_time: A_long,
	duration: A_long,
	time_scale: A_u_long,
	data_type: PF_DataType,
	channel_chunkP: *mut PF_ChannelChunk,
) -> PF_Err {
	diag!("PF_ChannelSuite1/PF_CheckoutLayerChannel",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"channel_refP" => format!("{:#x}", channel_refP as usize),
		"what_time" => what_time,
		"duration" => duration,
		"time_scale" => time_scale,
		"data_type" => format!("{:#x}", data_type),
	);
	let _ = (effect_ref, channel_refP, what_time, duration, time_scale, data_type);

	if let Some(chunk) = unsafe { channel_chunkP.as_mut() } {
		*chunk = unsafe { std::mem::zeroed() };
	}
	PF_Err_BAD_CALLBACK_PARAM as PF_Err
}

unsafe extern "C" fn checkin_layer_channel(
	effect_ref: PF_ProgPtr,
	channel_refP: PF_ChannelRefPtr,
	channel_chunkP: *mut PF_ChannelChunk,
) -> PF_Err {
	diag!("PF_ChannelSuite1/PF_CheckinLayerChannel",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"channel_refP" => format!("{:#x}", channel_refP as usize),
		"channel_chunkP" => format!("{:#x}", channel_chunkP as usize),
	);
	let _ = (effect_ref, channel_refP, channel_chunkP);
	PF_Err_NONE as PF_Err
}

pub(super) const fn create_channel_suite_1() -> PF_ChannelSuite1 {
	PF_ChannelSuite1 {
		PF_GetLayerChannelCount: Some(get_layer_channel_count),
		PF_GetLayerChannelIndexedRefAndDesc: Some(get_layer_channel_indexed_ref_and_desc),
		PF_GetLayerChannelTypedRefAndDesc: Some(get_layer_channel_typed_ref_and_desc),
		PF_CheckoutLayerChannel: Some(checkout_layer_channel),
		PF_CheckinLayerChannel: Some(checkin_layer_channel),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn layers_have_no_aux_channels() {
		let suite = create_channel_suite_1();
		let (mut count, mut found) = (7, 1);
		let mut desc: PF_ChannelDesc = unsafe { std::mem::zeroed() };
		desc.dimension = 3;
		unsafe {
			suite.PF_GetLayerChannelCount.unwrap()(std::ptr::null_mut(), 0, &mut count);
			suite.PF_GetLayerChannelTypedRefAndDesc.unwrap()(
				std::ptr::null_mut(),
				0,
				0,
				&mut found,
				std::ptr::null_mut(),
				&mut desc,
			);
		}
		assert_eq!((count, found, desc.dimension), (0, 0, 0));
	}
}
