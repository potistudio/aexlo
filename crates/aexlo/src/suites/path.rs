//! `PF_PathQuerySuite1` and `PF_PathDataSuite1`: read access to the layer's
//! masks, which embedders attach with
//! [`PluginInstance::set_mask_paths`](crate::PluginInstance::set_mask_paths).
//!
//! A checked-out `PF_PathOutlinePtr` points at the instance's [`MaskPath`]
//! (paths are static, so `what_time` is ignored). Segment-length preps are boxed
//! [`ArcTable`]s owned by the plugin until `PF_PathCleanupSegLength`; calls
//! without a prep build a temporary one.

use super::ffi::write_c_str;
use crate::PluginInstance;
use crate::core::diagnostics::diag;
use crate::mask::{ArcTable, MaskPath, bezier_deriv, bezier_point};
use after_effects_sys::{
	A_char, A_long, A_u_long, PF_Boolean, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_INVALID_INDEX, PF_Err_NONE,
	PF_FpLong, PF_MAX_PATH_NAME_LEN, PF_MaskMode, PF_PathDataSuite1, PF_PathID, PF_PathOutlinePtr, PF_PathQuerySuite1,
	PF_PathSegPrepPtr, PF_PathVertex, PF_ProgPtr,
};
use std::borrow::Cow;

/// Arc-length samples per segment when the plugin passes no prep (or a
/// non-positive frequency).
const DEFAULT_SAMPLES: usize = 64;
const MAX_SAMPLES: usize = 1 << 16;

/// A prepared segment: which path/segment it measures plus its table.
struct SegPrep {
	path: usize,
	seg: usize,
	table: ArcTable,
}

/// The masks of the instance behind `effect_ref`.
///
/// # Safety
/// `effect_ref` must be null or the instance currently being called into.
unsafe fn masks<'a>(effect_ref: PF_ProgPtr) -> Option<&'a [MaskPath]> {
	PluginInstance::get_instance_ptr(effect_ref).map(|i| unsafe { i.as_ref() }.mask_paths())
}

/// The mask with `id` on the instance behind `effect_ref`.
///
/// # Safety
/// See [`masks`].
unsafe fn mask_by_id<'a>(effect_ref: PF_ProgPtr, id: PF_PathID) -> Option<&'a MaskPath> {
	unsafe { masks(effect_ref) }?.iter().find(|m| m.id == id)
}

/// # Safety
/// `pathP` must be null or a pointer returned by `PF_CheckoutPath` whose
/// instance's masks have not been replaced since.
unsafe fn outline<'a>(pathP: PF_PathOutlinePtr) -> Option<&'a MaskPath> {
	unsafe { (pathP as *const MaskPath).as_ref() }
}

/// The arc table for `seg`: the plugin's prep when it matches, else a fresh one.
///
/// # Safety
/// `prepPP0` must be null or point to null or a prep from `PF_PathPrepareSegLength`.
unsafe fn table<'a>(path: &MaskPath, seg: usize, prepPP0: *mut PF_PathSegPrepPtr) -> Option<Cow<'a, ArcTable>> {
	let prep = unsafe { prepPP0.as_ref() }.and_then(|p| unsafe { (*p as *const SegPrep).as_ref() });
	match prep {
		Some(p) if p.path == path as *const MaskPath as usize && p.seg == seg => Some(Cow::Borrowed(&p.table)),
		_ => Some(Cow::Owned(ArcTable::new(&path.segment(seg)?, DEFAULT_SAMPLES))),
	}
}

// ---- PF Path Query Suite ---------------------------------------------------

unsafe extern "C" fn num_paths(effect_ref: PF_ProgPtr, num_pathsPL: *mut A_long) -> PF_Err {
	diag!("PF_PathQuerySuite1/PF_NumPaths",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
	);

	let (Some(masks), Some(out)) = (unsafe { masks(effect_ref) }, unsafe { num_pathsPL.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	*out = masks.len() as A_long;
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn path_info(effect_ref: PF_ProgPtr, indexL: A_long, unique_idP: *mut PF_PathID) -> PF_Err {
	diag!("PF_PathQuerySuite1/PF_PathInfo",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"indexL" => indexL,
	);

	let (Some(masks), Some(out)) = (unsafe { masks(effect_ref) }, unsafe { unique_idP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	let Some(mask) = usize::try_from(indexL).ok().and_then(|i| masks.get(i)) else {
		return PF_Err_INVALID_INDEX as PF_Err;
	};
	*out = mask.id as PF_PathID;
	PF_Err_NONE as PF_Err
}

/// Yields null (and no error) for an unknown id, per the SDK.
unsafe extern "C" fn checkout_path(
	effect_ref: PF_ProgPtr,
	unique_id: PF_PathID,
	what_time: A_long,
	time_step: A_long,
	time_scale: A_u_long,
	pathPP: *mut PF_PathOutlinePtr,
) -> PF_Err {
	diag!("PF_PathQuerySuite1/PF_CheckoutPath",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"unique_id" => unique_id,
		"what_time" => what_time,
		"time_step" => time_step,
		"time_scale" => time_scale,
	);
	let _ = (what_time, time_step, time_scale);

	let Some(out) = (unsafe { pathPP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	*out = unsafe { mask_by_id(effect_ref, unique_id) }
		.map_or(std::ptr::null_mut(), |m| m as *const MaskPath as PF_PathOutlinePtr);
	PF_Err_NONE as PF_Err
}

/// Paths are read-only to plugins here; `changedB` is ignored.
unsafe extern "C" fn checkin_path(
	effect_ref: PF_ProgPtr,
	unique_id: PF_PathID,
	changedB: PF_Boolean,
	pathP: PF_PathOutlinePtr,
) -> PF_Err {
	diag!("PF_PathQuerySuite1/PF_CheckinPath",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"unique_id" => unique_id,
		"changedB" => changedB,
		"pathP" => format!("{:#x}", pathP as usize),
	);
	let _ = (effect_ref, unique_id, changedB, pathP);
	PF_Err_NONE as PF_Err
}

pub(super) const fn create_path_query_suite_1() -> PF_PathQuerySuite1 {
	PF_PathQuerySuite1 {
		PF_NumPaths: Some(num_paths),
		PF_PathInfo: Some(path_info),
		PF_CheckoutPath: Some(checkout_path),
		PF_CheckinPath: Some(checkin_path),
	}
}

// ---- PF Path Data Suite ----------------------------------------------------

unsafe extern "C" fn path_is_open(
	_effect_ref0: PF_ProgPtr,
	pathP: PF_PathOutlinePtr,
	openPB: *mut PF_Boolean,
) -> PF_Err {
	let (Some(path), Some(out)) = (unsafe { outline(pathP) }, unsafe { openPB.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	*out = !path.closed as PF_Boolean;
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn path_num_segments(
	_effect_ref0: PF_ProgPtr,
	pathP: PF_PathOutlinePtr,
	num_segmentsPL: *mut A_long,
) -> PF_Err {
	let (Some(path), Some(out)) = (unsafe { outline(pathP) }, unsafe { num_segmentsPL.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	*out = path.segment_count() as A_long;
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn path_vertex_info(
	_effect_ref0: PF_ProgPtr,
	pathP: PF_PathOutlinePtr,
	which_pointL: A_long,
	vertexP: *mut PF_PathVertex,
) -> PF_Err {
	let (Some(path), Some(out)) = (unsafe { outline(pathP) }, unsafe { vertexP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	let Some(v) = usize::try_from(which_pointL).ok().and_then(|i| path.vertex(i)) else {
		return PF_Err_INVALID_INDEX as PF_Err;
	};
	*out = PF_PathVertex {
		x: v.x,
		y: v.y,
		tan_in_x: v.tan_in_x,
		tan_in_y: v.tan_in_y,
		tan_out_x: v.tan_out_x,
		tan_out_y: v.tan_out_y,
	};
	PF_Err_NONE as PF_Err
}

/// `frequencyL` is the number of arc-length samples (defaulted when non-positive).
unsafe extern "C" fn path_prepare_seg_length(
	_effect_ref0: PF_ProgPtr,
	pathP: PF_PathOutlinePtr,
	which_segL: A_long,
	frequencyL: A_long,
	lengthPrepPP: *mut PF_PathSegPrepPtr,
) -> PF_Err {
	let (Some(path), Some(out)) = (unsafe { outline(pathP) }, unsafe { lengthPrepPP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	let Some((seg, points)) = usize::try_from(which_segL)
		.ok()
		.and_then(|s| Some((s, path.segment(s)?)))
	else {
		return PF_Err_INVALID_INDEX as PF_Err;
	};
	let samples = usize::try_from(frequencyL)
		.ok()
		.filter(|&f| f > 0)
		.map_or(DEFAULT_SAMPLES, |f| f.min(MAX_SAMPLES));
	let prep = Box::new(SegPrep {
		path: path as *const MaskPath as usize,
		seg,
		table: ArcTable::new(&points, samples),
	});
	*out = Box::into_raw(prep) as PF_PathSegPrepPtr;
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn path_get_seg_length(
	_effect_ref0: PF_ProgPtr,
	pathP: PF_PathOutlinePtr,
	which_segL: A_long,
	lengthPrepP0: *mut PF_PathSegPrepPtr,
	lengthPF: *mut PF_FpLong,
) -> PF_Err {
	let (Some(path), Some(out)) = (unsafe { outline(pathP) }, unsafe { lengthPF.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	let Some(table) = usize::try_from(which_segL)
		.ok()
		.and_then(|s| unsafe { table(path, s, lengthPrepP0) })
	else {
		return PF_Err_INVALID_INDEX as PF_Err;
	};
	*out = table.length();
	PF_Err_NONE as PF_Err
}

/// Shared body of the two evaluators: the point (and derivative) at distance
/// `lengthF` along segment `which_segL`.
///
/// # Safety
/// Pointers as for the FFI entry points; `deriv` outputs may be null.
#[allow(clippy::too_many_arguments)]
unsafe fn eval(
	pathP: PF_PathOutlinePtr,
	prepPP0: *mut PF_PathSegPrepPtr,
	which_segL: A_long,
	lengthF: PF_FpLong,
	x: *mut PF_FpLong,
	y: *mut PF_FpLong,
	dx: *mut PF_FpLong,
	dy: *mut PF_FpLong,
) -> PF_Err {
	let (Some(path), Some(x), Some(y)) = (unsafe { outline(pathP) }, unsafe { x.as_mut() }, unsafe { y.as_mut() })
	else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	let Some(seg) = usize::try_from(which_segL).ok().filter(|&s| s < path.segment_count()) else {
		return PF_Err_INVALID_INDEX as PF_Err;
	};
	let points = path.segment(seg).unwrap();
	let t = unsafe { table(path, seg, prepPP0) }.unwrap().t_at(lengthF);
	(*x, *y) = bezier_point(&points, t);
	let (d0, d1) = bezier_deriv(&points, t);
	unsafe {
		if let Some(dx) = dx.as_mut() {
			*dx = d0;
		}
		if let Some(dy) = dy.as_mut() {
			*dy = d1;
		}
	}
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn path_eval_seg_length(
	_effect_ref0: PF_ProgPtr,
	pathP: PF_PathOutlinePtr,
	lengthPrepPP0: *mut PF_PathSegPrepPtr,
	which_segL: A_long,
	lengthF: PF_FpLong,
	x: *mut PF_FpLong,
	y: *mut PF_FpLong,
) -> PF_Err {
	let null = std::ptr::null_mut();
	unsafe { eval(pathP, lengthPrepPP0, which_segL, lengthF, x, y, null, null) }
}

unsafe extern "C" fn path_eval_seg_length_deriv1(
	_effect_ref0: PF_ProgPtr,
	pathP: PF_PathOutlinePtr,
	lengthPrepPP0: *mut PF_PathSegPrepPtr,
	which_segL: A_long,
	lengthF: PF_FpLong,
	x: *mut PF_FpLong,
	y: *mut PF_FpLong,
	deriv1x: *mut PF_FpLong,
	deriv1y: *mut PF_FpLong,
) -> PF_Err {
	unsafe { eval(pathP, lengthPrepPP0, which_segL, lengthF, x, y, deriv1x, deriv1y) }
}

unsafe extern "C" fn path_cleanup_seg_length(
	_effect_ref0: PF_ProgPtr,
	_pathP: PF_PathOutlinePtr,
	_which_segL: A_long,
	lengthPrepPP: *mut PF_PathSegPrepPtr,
) -> PF_Err {
	let Some(prep) = (unsafe { lengthPrepPP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	if !prep.is_null() {
		drop(unsafe { Box::from_raw(*prep as *mut SegPrep) });
		*prep = std::ptr::null_mut();
	}
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn path_is_inverted(
	effect_ref: PF_ProgPtr,
	unique_id: PF_PathID,
	invertedB: *mut PF_Boolean,
) -> PF_Err {
	let (Some(mask), Some(out)) = (unsafe { mask_by_id(effect_ref, unique_id) }, unsafe {
		invertedB.as_mut()
	}) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	*out = mask.inverted as PF_Boolean;
	PF_Err_NONE as PF_Err
}

unsafe extern "C" fn path_get_mask_mode(
	effect_ref: PF_ProgPtr,
	unique_id: PF_PathID,
	modeP: *mut PF_MaskMode,
) -> PF_Err {
	let (Some(mask), Some(out)) = (unsafe { mask_by_id(effect_ref, unique_id) }, unsafe { modeP.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	*out = mask.mode.to_sdk();
	PF_Err_NONE as PF_Err
}

/// `nameZ` must hold `PF_MAX_PATH_NAME_LEN + 1` bytes.
unsafe extern "C" fn path_get_name(effect_ref: PF_ProgPtr, unique_id: PF_PathID, nameZ: *mut A_char) -> PF_Err {
	let Some(mask) = (unsafe { mask_by_id(effect_ref, unique_id) }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	if nameZ.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let buf = unsafe { std::slice::from_raw_parts_mut(nameZ, PF_MAX_PATH_NAME_LEN as usize + 1) };
	write_c_str(buf, &mask.name);
	PF_Err_NONE as PF_Err
}

pub(super) const fn create_path_data_suite_1() -> PF_PathDataSuite1 {
	PF_PathDataSuite1 {
		PF_PathIsOpen: Some(path_is_open),
		PF_PathNumSegments: Some(path_num_segments),
		PF_PathVertexInfo: Some(path_vertex_info),
		PF_PathPrepareSegLength: Some(path_prepare_seg_length),
		PF_PathGetSegLength: Some(path_get_seg_length),
		PF_PathEvalSegLength: Some(path_eval_seg_length),
		PF_PathEvalSegLengthDeriv1: Some(path_eval_seg_length_deriv1),
		PF_PathCleanupSegLength: Some(path_cleanup_seg_length),
		PF_PathIsInverted: Some(path_is_inverted),
		PF_PathGetMaskMode: Some(path_get_mask_mode),
		PF_PathGetName: Some(path_get_name),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mask::MaskVertex;

	fn line() -> MaskPath {
		MaskPath {
			id: 7,
			vertices: vec![MaskVertex::corner(0.0, 0.0), MaskVertex::corner(0.0, 20.0)],
			..Default::default()
		}
	}

	#[test]
	fn data_suite_walks_an_outline() {
		let path = line();
		let p = &path as *const MaskPath as PF_PathOutlinePtr;
		let suite = create_path_data_suite_1();
		let null = std::ptr::null_mut();
		unsafe {
			let (mut open, mut segs) = (0, 0);
			suite.PF_PathIsOpen.unwrap()(null, p, &mut open);
			suite.PF_PathNumSegments.unwrap()(null, p, &mut segs);
			assert_eq!((open, segs), (1, 1));

			let mut prep: PF_PathSegPrepPtr = std::ptr::null_mut();
			suite.PF_PathPrepareSegLength.unwrap()(null, p, 0, 16, &mut prep);
			assert!(!prep.is_null());

			let mut len = 0.0;
			suite.PF_PathGetSegLength.unwrap()(null, p, 0, &mut prep, &mut len);
			assert!((len - 20.0).abs() < 1e-9);

			let (mut x, mut y, mut dx, mut dy) = (0.0, 0.0, 0.0, 0.0);
			suite.PF_PathEvalSegLengthDeriv1.unwrap()(null, p, &mut prep, 0, 5.0, &mut x, &mut y, &mut dx, &mut dy);
			assert!(x.abs() < 1e-9 && (y - 5.0).abs() < 1e-6, "{x},{y}");
			assert!(dx.abs() < 1e-9 && dy > 0.0);

			suite.PF_PathCleanupSegLength.unwrap()(null, p, 0, &mut prep);
			assert!(prep.is_null());

			let err = suite.PF_PathEvalSegLength.unwrap()(null, p, std::ptr::null_mut(), 1, 0.0, &mut x, &mut y);
			assert_eq!(err, PF_Err_INVALID_INDEX as PF_Err);
		}
	}
}
