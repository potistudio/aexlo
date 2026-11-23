//! `AEGP Layer Mask Suite` v7 (`AEGP_MaskSuite6`) and v6 (`AEGP_MaskSuite5`):
//! read access to the masks an embedder puts on the effect's layer with
//! [`PluginInstance::set_mask_paths`](crate::PluginInstance::set_mask_paths),
//! the same ones the "PF Path Query Suite" serves.
//!
//! An `AEGP_MaskRefH` names a mask by its index on the layer, so a ref taken
//! before the embedder replaces the masks reads the mask now at that index, or
//! fails if there is none. A mask's outline reaches the plugin as the value of
//! its `AEGP_MaskStream_OUTLINE` stream ([`super::stream`]): an
//! `AEGP_MaskOutlineValH` holding a copy of the path, read through
//! [`super::mask_outline`] and freed with the stream value, as a shape
//! path's is. Masks are static here, so times are ignored, and nothing can
//! change them: every setter, and creating, deleting or duplicating a mask,
//! is declined. Only the effect's own layer has masks; any other layer of a
//! project has none.
//!
//! Mask refs are kept in a list of the ones handed out: a plugin that bails
//! out of a failed call may pass an uninitialised handle, which is refused
//! instead of read or freed.

use crate::PluginInstance;
use crate::core::diagnostics::diag;
use crate::mask::MaskPath;
use crate::sys::*;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Addresses of the boxes of one handle kind that are handed out and not yet
/// disposed.
struct Handed(Mutex<Vec<usize>>);

impl Handed {
	const fn new() -> Self {
		Self(Mutex::new(Vec::new()))
	}

	fn lock(&self) -> MutexGuard<'_, Vec<usize>> {
		self.0.lock().unwrap_or_else(PoisonError::into_inner)
	}

	fn hand_out<T>(&self, value: T) -> *mut T {
		let ptr = Box::into_raw(Box::new(value));
		self.lock().push(ptr as usize);
		ptr
	}

	/// The box at `ptr`, if it was handed out and not taken back.
	fn get<'a, T>(&self, ptr: *const T) -> Option<&'a T> {
		if !self.lock().contains(&(ptr as usize)) {
			return None;
		}
		// SAFETY: a listed address is a live `Box<T>` from `hand_out`, freed only
		// by `take_back`.
		unsafe { ptr.as_ref() }
	}

	/// Take back and free the box at `ptr`; `false` if it is not handed out.
	fn take_back<T>(&self, ptr: *mut T) -> bool {
		{
			let mut handed = self.lock();
			let Some(at) = handed.iter().position(|&p| p == ptr as usize) else {
				return false;
			};
			handed.swap_remove(at);
		}
		// SAFETY: it was listed, so it is a `Box<T>` from `hand_out`, and it is
		// no longer listed.
		drop(unsafe { Box::from_raw(ptr) });
		true
	}
}

/// What an `AEGP_MaskRefH` points at.
#[derive(Clone, Copy)]
pub(crate) struct MaskRef {
	pub(crate) effect_ref: PF_ProgPtr,
	pub(crate) index: usize,
}

static MASK_REFS: Handed = Handed::new();

/// The masks on the layer of the instance behind `effect_ref`.
fn masks<'a>(effect_ref: PF_ProgPtr) -> Option<&'a [MaskPath]> {
	let instance = PluginInstance::get_instance_ptr(effect_ref)?;
	// SAFETY: a non-null effect ref that reached a suite call is the live instance.
	Some(unsafe { instance.as_ref() }.mask_paths())
}

/// The mask ref behind `mask_refH`, if aexlo handed it out.
pub(crate) fn mask_ref(mask_refH: AEGP_MaskRefH) -> Option<MaskRef> {
	MASK_REFS.get(mask_refH as *const MaskRef).copied()
}

/// The mask `mask_refH` names, if it is still on the layer.
fn mask<'a>(mask_refH: AEGP_MaskRefH) -> Option<&'a MaskPath> {
	let mask = mask_ref(mask_refH)?;
	masks(mask.effect_ref)?.get(mask.index)
}

/// A copy of the mask at `index` on the layer of `effect_ref`.
pub(crate) fn mask_path(effect_ref: PF_ProgPtr, index: usize) -> Option<MaskPath> {
	masks(effect_ref)?.get(index).cloned()
}

/// The effect behind a layer handle and the masks on that layer: the
/// effect's own, or none for another layer of the project.
fn layer_masks<'a>(aegp_layerH: AEGP_LayerH) -> Option<(PF_ProgPtr, &'a [MaskPath])> {
	let timeline = super::layer::Timeline::of(aegp_layerH.cast())?;
	if !timeline.hosts_effect {
		return Some((timeline.effect, &[]));
	}
	Some((timeline.effect, masks(timeline.effect)?))
}

/// Whether `index` is a mask on the layer of `effect_ref`.
pub(crate) fn has_mask(effect_ref: PF_ProgPtr, index: usize) -> bool {
	masks(effect_ref).is_some_and(|masks| index < masks.len())
}

fn declined(name: &'static str) -> A_Err {
	crate::observe::unsupported(
		crate::observe::UnsupportedKind::Declined,
		name,
		"aexlo's masks are read-only",
	);
	A_Err_GENERIC
}

/// Write `value` through `out`, or refuse a null `out`.
fn answer<T>(out: *mut T, value: T) -> A_Err {
	match unsafe { out.as_mut() } {
		Some(out) => {
			*out = value;
			PF_Err_NONE
		}
		None => PF_Err_BAD_CALLBACK_PARAM,
	}
}

/// Answer `out` from the mask `mask_refH` names.
fn from_mask<T>(mask_refH: AEGP_MaskRefH, out: *mut T, value: impl FnOnce(&MaskPath) -> T) -> A_Err {
	match mask(mask_refH) {
		Some(mask) => answer(out, value(mask)),
		None => PF_Err_BAD_CALLBACK_PARAM,
	}
}

// ============================================================================
// AEGP Layer Mask Suite
// ============================================================================

unsafe extern "C" fn get_layer_num_masks(aegp_layerH: AEGP_LayerH, num_masksPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("AEGP_GetLayerNumMasks", || {
		diag!("AEGP_MaskSuite/AEGP_GetLayerNumMasks", "aegp_layerH" => format!("{:#x}", aegp_layerH as usize));
		let Some((_, masks)) = layer_masks(aegp_layerH) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		answer(num_masksPL, masks.len() as A_long)
	})
}

unsafe extern "C" fn get_layer_mask_by_index(
	aegp_layerH: AEGP_LayerH,
	mask_indexL: AEGP_MaskIndex,
	maskPH: *mut AEGP_MaskRefH,
) -> A_Err {
	crate::core::ffi::guard("AEGP_GetLayerMaskByIndex", || {
		diag!("AEGP_MaskSuite/AEGP_GetLayerMaskByIndex",
			"aegp_layerH" => format!("{:#x}", aegp_layerH as usize),
			"mask_indexL" => mask_indexL,
		);
		let Some(out) = (unsafe { maskPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = std::ptr::null_mut();
		let Some((effect_ref, masks)) = layer_masks(aegp_layerH) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(index) = usize::try_from(mask_indexL).ok().filter(|&i| i < masks.len()) else {
			return PF_Err_INVALID_INDEX;
		};
		*out = MASK_REFS.hand_out(MaskRef { effect_ref, index }) as AEGP_MaskRefH;
		PF_Err_NONE
	})
}

unsafe extern "C" fn dispose_mask(mask_refH: AEGP_MaskRefH) -> A_Err {
	crate::core::ffi::guard("AEGP_DisposeMask", || {
		diag!("AEGP_MaskSuite/AEGP_DisposeMask", "mask_refH" => format!("{:#x}", mask_refH as usize));
		if !MASK_REFS.take_back(mask_refH as *mut MaskRef) {
			log::warn!("AEGP_MaskSuite/AEGP_DisposeMask: {mask_refH:?} was not handed out, or was disposed");
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_mask_invert(mask_refH: AEGP_MaskRefH, invertPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("AEGP_GetMaskInvert", || {
		diag!("AEGP_MaskSuite/AEGP_GetMaskInvert", "mask_refH" => format!("{:#x}", mask_refH as usize));
		from_mask(mask_refH, invertPB, |m| m.inverted as A_Boolean)
	})
}

unsafe extern "C" fn get_mask_mode(mask_refH: AEGP_MaskRefH, modeP: *mut PF_MaskMode) -> A_Err {
	crate::core::ffi::guard("AEGP_GetMaskMode", || {
		diag!("AEGP_MaskSuite/AEGP_GetMaskMode", "mask_refH" => format!("{:#x}", mask_refH as usize));
		from_mask(mask_refH, modeP, |m| m.mode.to_sys())
	})
}

/// Masks follow the layer's motion blur switch.
unsafe extern "C" fn get_mask_motion_blur_state(mask_refH: AEGP_MaskRefH, blur_stateP: *mut AEGP_MaskMBlur) -> A_Err {
	crate::core::ffi::guard("AEGP_GetMaskMotionBlurState", || {
		diag!("AEGP_MaskSuite/AEGP_GetMaskMotionBlurState", "mask_refH" => format!("{:#x}", mask_refH as usize));
		from_mask(mask_refH, blur_stateP, |_| AEGP_MaskMBlur_SAME_AS_LAYER)
	})
}

/// After Effects' default falloff.
unsafe extern "C" fn get_mask_feather_falloff(
	mask_refH: AEGP_MaskRefH,
	feather_falloffP: *mut AEGP_MaskFeatherFalloff,
) -> A_Err {
	crate::core::ffi::guard("AEGP_GetMaskFeatherFalloff", || {
		diag!("AEGP_MaskSuite/AEGP_GetMaskFeatherFalloff", "mask_refH" => format!("{:#x}", mask_refH as usize));
		from_mask(mask_refH, feather_falloffP, |_| AEGP_MaskFeatherFalloff_SMOOTH)
	})
}

/// The id a `PF_Param_PATH` parameter refers to the mask by.
unsafe extern "C" fn get_mask_id(mask_refH: AEGP_MaskRefH, id_valP: *mut AEGP_MaskIDVal) -> A_Err {
	crate::core::ffi::guard("AEGP_GetMaskID", || {
		diag!("AEGP_MaskSuite/AEGP_GetMaskID", "mask_refH" => format!("{:#x}", mask_refH as usize));
		from_mask(mask_refH, id_valP, |m| m.id as AEGP_MaskIDVal)
	})
}

/// After Effects' first mask colour, a light blue.
const MASK_COLOR: AEGP_ColorVal = AEGP_ColorVal {
	alphaF: 1.0,
	redF: 0.69,
	greenF: 0.78,
	blueF: 0.98,
};

unsafe extern "C" fn get_mask_color(mask_refH: AEGP_MaskRefH, colorP: *mut AEGP_ColorVal) -> A_Err {
	crate::core::ffi::guard("AEGP_GetMaskColor", || {
		diag!("AEGP_MaskSuite/AEGP_GetMaskColor", "mask_refH" => format!("{:#x}", mask_refH as usize));
		from_mask(mask_refH, colorP, |_| MASK_COLOR)
	})
}

unsafe extern "C" fn get_mask_lock_state(mask_refH: AEGP_MaskRefH, is_lockedPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("AEGP_GetMaskLockState", || {
		diag!("AEGP_MaskSuite/AEGP_GetMaskLockState", "mask_refH" => format!("{:#x}", mask_refH as usize));
		from_mask(mask_refH, is_lockedPB, |_| 0)
	})
}

unsafe extern "C" fn get_mask_is_roto_bezier(mask_refH: AEGP_MaskRefH, is_roto_bezierPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("AEGP_GetMaskIsRotoBezier", || {
		diag!("AEGP_MaskSuite/AEGP_GetMaskIsRotoBezier", "mask_refH" => format!("{:#x}", mask_refH as usize));
		from_mask(mask_refH, is_roto_bezierPB, |_| 0)
	})
}

/// A setter, or a call that adds or removes masks: declined.
macro_rules! declined {
	($name:ident, $function:literal, ($($arg:ident: $ty:ty),*)) => {
		unsafe extern "C" fn $name($($arg: $ty),*) -> A_Err {
			crate::core::ffi::guard($function, || {
				$(let _ = $arg;)*
				diag!(concat!("AEGP_MaskSuite/", $function));
				declined(concat!("AEGP_MaskSuite/", $function))
			})
		}
	};
}

declined!(set_mask_invert, "AEGP_SetMaskInvert", (mask_refH: AEGP_MaskRefH, invertB: A_Boolean));
declined!(set_mask_mode, "AEGP_SetMaskMode", (maskH: AEGP_MaskRefH, mode: PF_MaskMode));
declined!(set_mask_motion_blur_state, "AEGP_SetMaskMotionBlurState", (mask_refH: AEGP_MaskRefH, blur_state: AEGP_MaskMBlur));
declined!(set_mask_feather_falloff, "AEGP_SetMaskFeatherFalloff", (mask_refH: AEGP_MaskRefH, falloff: AEGP_MaskFeatherFalloff));
declined!(create_new_mask, "AEGP_CreateNewMask", (layerH: AEGP_LayerH, mask_refPH: *mut AEGP_MaskRefH, mask_indexPL0: *mut A_long));
declined!(delete_mask_from_layer, "AEGP_DeleteMaskFromLayer", (mask_refH: AEGP_MaskRefH));
declined!(set_mask_color, "AEGP_SetMaskColor", (mask_refH: AEGP_MaskRefH, colorP: *const AEGP_ColorVal));
declined!(set_mask_lock_state, "AEGP_SetMaskLockState", (mask_refH: AEGP_MaskRefH, lockB: A_Boolean));
declined!(set_mask_is_roto_bezier, "AEGP_SetMaskIsRotoBezier", (mask_refH: AEGP_MaskRefH, is_roto_bezierB: A_Boolean));
declined!(duplicate_mask, "AEGP_DuplicateMask", (orig_mask_refH: AEGP_MaskRefH, duplicate_mask_refPH: *mut AEGP_MaskRefH));

/// Wire version 7.
pub(super) const fn mask_suite_6() -> AEGP_MaskSuite6 {
	AEGP_MaskSuite6 {
		AEGP_GetLayerNumMasks: Some(get_layer_num_masks),
		AEGP_GetLayerMaskByIndex: Some(get_layer_mask_by_index),
		AEGP_DisposeMask: Some(dispose_mask),
		AEGP_GetMaskInvert: Some(get_mask_invert),
		AEGP_SetMaskInvert: Some(set_mask_invert),
		AEGP_GetMaskMode: Some(get_mask_mode),
		AEGP_SetMaskMode: Some(set_mask_mode),
		AEGP_GetMaskMotionBlurState: Some(get_mask_motion_blur_state),
		AEGP_SetMaskMotionBlurState: Some(set_mask_motion_blur_state),
		AEGP_GetMaskFeatherFalloff: Some(get_mask_feather_falloff),
		AEGP_SetMaskFeatherFalloff: Some(set_mask_feather_falloff),
		AEGP_GetMaskID: Some(get_mask_id),
		AEGP_CreateNewMask: Some(create_new_mask),
		AEGP_DeleteMaskFromLayer: Some(delete_mask_from_layer),
		AEGP_GetMaskColor: Some(get_mask_color),
		AEGP_SetMaskColor: Some(set_mask_color),
		AEGP_GetMaskLockState: Some(get_mask_lock_state),
		AEGP_SetMaskLockState: Some(set_mask_lock_state),
		AEGP_GetMaskIsRotoBezier: Some(get_mask_is_roto_bezier),
		AEGP_SetMaskIsRotoBezier: Some(set_mask_is_roto_bezier),
		AEGP_DuplicateMask: Some(duplicate_mask),
	}
}

/// Wire version 6: version 7 without the feather falloff calls.
pub(super) const fn mask_suite_5() -> AEGP_MaskSuite5 {
	AEGP_MaskSuite5 {
		AEGP_GetLayerNumMasks: Some(get_layer_num_masks),
		AEGP_GetLayerMaskByIndex: Some(get_layer_mask_by_index),
		AEGP_DisposeMask: Some(dispose_mask),
		AEGP_GetMaskInvert: Some(get_mask_invert),
		AEGP_SetMaskInvert: Some(set_mask_invert),
		AEGP_GetMaskMode: Some(get_mask_mode),
		AEGP_SetMaskMode: Some(set_mask_mode),
		AEGP_GetMaskMotionBlurState: Some(get_mask_motion_blur_state),
		AEGP_SetMaskMotionBlurState: Some(set_mask_motion_blur_state),
		AEGP_GetMaskID: Some(get_mask_id),
		AEGP_CreateNewMask: Some(create_new_mask),
		AEGP_DeleteMaskFromLayer: Some(delete_mask_from_layer),
		AEGP_GetMaskColor: Some(get_mask_color),
		AEGP_SetMaskColor: Some(set_mask_color),
		AEGP_GetMaskLockState: Some(get_mask_lock_state),
		AEGP_SetMaskLockState: Some(set_mask_lock_state),
		AEGP_GetMaskIsRotoBezier: Some(get_mask_is_roto_bezier),
		AEGP_SetMaskIsRotoBezier: Some(set_mask_is_roto_bezier),
		AEGP_DuplicateMask: Some(duplicate_mask),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mask::MaskVertex;

	fn triangle() -> MaskPath {
		MaskPath {
			id: 7,
			vertices: vec![
				MaskVertex {
					tan_out_x: 2.0,
					..MaskVertex::corner(0.0, 0.0)
				},
				MaskVertex::corner(4.0, 0.0),
				MaskVertex::corner(4.0, 4.0),
			],
			closed: true,
			..Default::default()
		}
	}

	#[test]
	fn an_outline_is_read_and_freed_once() {
		let outline = super::super::mask_outline::publish(triangle());
		let suite = super::super::mask_outline::suite_3();
		unsafe {
			let mut open: A_Boolean = 1;
			assert_eq!(suite.AEGP_IsMaskOutlineOpen.unwrap()(outline, &mut open), PF_Err_NONE);
			assert_eq!(open, 0);
			let mut segments = 0;
			assert_eq!(
				suite.AEGP_GetMaskOutlineNumSegments.unwrap()(outline, &mut segments),
				PF_Err_NONE
			);
			assert_eq!(segments, 3);

			let mut vertex: AEGP_MaskVertex = std::mem::zeroed();
			let info = suite.AEGP_GetMaskOutlineVertexInfo.unwrap();
			assert_eq!(
				info(outline, 3, &mut vertex),
				PF_Err_NONE,
				"closed: the last index wraps"
			);
			assert_eq!((vertex.x, vertex.y, vertex.tan_out_x), (0.0, 0.0, 2.0));
			assert_eq!(info(outline, 4, &mut vertex), PF_Err_INVALID_INDEX);

			let mut feathers = -1;
			assert_eq!(
				suite.AEGP_GetMaskOutlineNumFeathers.unwrap()(outline, &mut feathers),
				PF_Err_NONE
			);
			assert_eq!(feathers, 0);
			assert_eq!(suite.AEGP_SetMaskOutlineOpen.unwrap()(outline, 1), A_Err_GENERIC);
		}
		assert!(super::super::mask_outline::dispose(outline as usize));
		assert!(!super::super::mask_outline::dispose(outline as usize), "freed once");
		let mut open: A_Boolean = 0;
		assert_eq!(
			unsafe { suite.AEGP_IsMaskOutlineOpen.unwrap()(outline, &mut open) },
			PF_Err_BAD_CALLBACK_PARAM
		);
	}

	#[test]
	fn handles_that_were_never_handed_out_are_refused() {
		// What an uninitialised local might hold.
		let garbage = 0xdead_beef_usize;
		let masks = mask_suite_6();
		unsafe {
			assert_eq!(
				masks.AEGP_DisposeMask.unwrap()(garbage as AEGP_MaskRefH),
				PF_Err_BAD_CALLBACK_PARAM
			);
			let mut id = 0;
			assert_eq!(
				masks.AEGP_GetMaskID.unwrap()(garbage as AEGP_MaskRefH, &mut id),
				PF_Err_BAD_CALLBACK_PARAM
			);
			let mut count = 0;
			assert_eq!(
				masks.AEGP_GetLayerNumMasks.unwrap()(std::ptr::null_mut(), &mut count),
				PF_Err_BAD_CALLBACK_PARAM
			);
			let mut segments = 0;
			assert_eq!(
				super::super::mask_outline::suite_3()
					.AEGP_GetMaskOutlineNumSegments
					.unwrap()(garbage as AEGP_MaskOutlineValH, &mut segments),
				PF_Err_BAD_CALLBACK_PARAM
			);
		}
		assert!(!super::super::mask_outline::dispose(garbage));
	}

	#[test]
	fn masks_cannot_be_changed() {
		let masks = mask_suite_6();
		unsafe {
			assert_eq!(
				masks.AEGP_SetMaskInvert.unwrap()(std::ptr::null_mut(), 1),
				A_Err_GENERIC
			);
			assert_eq!(
				masks.AEGP_CreateNewMask.unwrap()(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()),
				A_Err_GENERIC
			);
		}
	}
}
