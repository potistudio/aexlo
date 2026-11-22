//! `AEGP Mask Outline Suite` (wire versions 2, 3 and 5): reading the outline
//! of a path stream's value (`ADBE Vector Shape`).
//!
//! `AEGP_GetNewStreamValue` publishes a copy of the path at the time asked
//! for, and `AEGP_DisposeStreamValue` drops it, so an outline cannot change
//! under the plugin reading it. Tangents are offsets from their vertex, as
//! `AEGP_MaskVertex` has them. The outlines are read-only.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::MaskPath;
use crate::core::diagnostics::diag;
use crate::observe::{UnsupportedKind, unsupported};
use crate::sys::*;

static OUTLINES: LazyLock<Mutex<HashMap<usize, MaskPath>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// A handle to a copy of `path`, until [`dispose`].
pub(super) fn publish(path: MaskPath) -> AEGP_MaskOutlineValH {
	let handle = super::value_handle();
	OUTLINES.lock().unwrap().insert(handle, path);
	handle as AEGP_MaskOutlineValH
}

/// Drop the outline behind `handle`, if it is one.
pub(super) fn dispose(handle: usize) -> bool {
	OUTLINES.lock().unwrap().remove(&handle).is_some()
}

fn with<T>(handle: AEGP_MaskOutlineValH, f: impl FnOnce(&MaskPath) -> T) -> Option<T> {
	OUTLINES.lock().unwrap().get(&(handle as usize)).map(f)
}

fn read_only(name: &str) -> A_Err {
	unsupported(
		UnsupportedKind::Declined,
		format!("AEGP_MaskOutlineSuite/{name}"),
		"the project is read-only",
	);
	A_Err_GENERIC
}

unsafe extern "C" fn is_open(outline: AEGP_MaskOutlineValH, openPB: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("is_mask_outline_open", || {
		diag!("AEGP_MaskOutlineSuite/AEGP_IsMaskOutlineOpen");
		let (Some(closed), Some(out)) = (with(outline, |path| path.closed), unsafe { openPB.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = !closed as A_Boolean;
		PF_Err_NONE
	})
}

unsafe extern "C" fn num_segments(outline: AEGP_MaskOutlineValH, num_segmentsPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_mask_outline_num_segments", || {
		diag!("AEGP_MaskOutlineSuite/AEGP_GetMaskOutlineNumSegments");
		let (Some(count), Some(out)) = (with(outline, MaskPath::segment_count), unsafe {
			num_segmentsPL.as_mut()
		}) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = count as A_long;
		PF_Err_NONE
	})
}

/// Points run `0..=segments`; a closed outline's last is its first.
unsafe extern "C" fn vertex_info(
	outline: AEGP_MaskOutlineValH,
	which_pointL: AEGP_VertexIndex,
	vertexP: *mut AEGP_MaskVertex,
) -> A_Err {
	crate::core::ffi::guard("get_mask_outline_vertex_info", || {
		diag!("AEGP_MaskOutlineSuite/AEGP_GetMaskOutlineVertexInfo", "which_pointL" => which_pointL);
		let (Some(vertex), Some(out)) = (
			with(outline, |path| {
				usize::try_from(which_pointL)
					.ok()
					.and_then(|at| path.vertex(at))
					.copied()
			}),
			unsafe { vertexP.as_mut() },
		) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(v) = vertex else {
			return PF_Err_INVALID_INDEX;
		};
		*out = PF_PathVertex {
			x: v.x,
			y: v.y,
			tan_in_x: v.tan_in_x,
			tan_in_y: v.tan_in_y,
			tan_out_x: v.tan_out_x,
			tan_out_y: v.tan_out_y,
		};
		PF_Err_NONE
	})
}

/// Shape paths have no variable-width feather.
unsafe extern "C" fn num_feathers(outline: AEGP_MaskOutlineValH, num_feathersPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_mask_outline_num_feathers", || {
		diag!("AEGP_MaskOutlineSuite/AEGP_GetMaskOutlineNumFeathers");
		let (Some(()), Some(out)) = (with(outline, |_| ()), unsafe { num_feathersPL.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = 0;
		PF_Err_NONE
	})
}

unsafe extern "C" fn feather_info(
	outline: AEGP_MaskOutlineValH,
	_which: AEGP_FeatherIndex,
	_featherP: *mut AEGP_MaskFeather,
) -> A_Err {
	crate::core::ffi::guard("get_mask_outline_feather_info", || {
		diag!("AEGP_MaskOutlineSuite/AEGP_GetMaskOutlineFeatherInfo");
		match with(outline, |_| ()) {
			Some(()) => PF_Err_INVALID_INDEX,
			None => PF_Err_BAD_CALLBACK_PARAM,
		}
	})
}

macro_rules! refuse {
	($($fn_name:ident => $name:literal ($($arg:ident : $ty:ty),* $(,)?);)*) => {
		$(unsafe extern "C" fn $fn_name($($arg: $ty),*) -> A_Err {
			crate::core::ffi::guard($name, || {
				$(let _ = $arg;)*
				diag!(concat!("AEGP_MaskOutlineSuite/", $name));
				read_only($name)
			})
		})*
	};
}

refuse! {
	set_open => "AEGP_SetMaskOutlineOpen"(outline: AEGP_MaskOutlineValH, open: A_Boolean);
	set_vertex => "AEGP_SetMaskOutlineVertexInfo"(outline: AEGP_MaskOutlineValH, at: AEGP_VertexIndex, vertex: *const AEGP_MaskVertex);
	create_vertex => "AEGP_CreateVertex"(outline: AEGP_MaskOutlineValH, at: AEGP_VertexIndex);
	delete_vertex => "AEGP_DeleteVertex"(outline: AEGP_MaskOutlineValH, at: AEGP_VertexIndex);
	set_feather => "AEGP_SetMaskOutlineFeatherInfo"(outline: AEGP_MaskOutlineValH, at: AEGP_VertexIndex, feather: *const AEGP_MaskFeather);
	create_feather => "AEGP_CreateMaskOutlineFeather"(outline: AEGP_MaskOutlineValH, feather: *const AEGP_MaskFeather, at: *mut AEGP_FeatherIndex);
	delete_feather => "AEGP_DeleteMaskOutlineFeather"(outline: AEGP_MaskOutlineValH, at: AEGP_FeatherIndex);
}

/// Wire version 2.
pub(super) const fn suite_1() -> AEGP_MaskOutlineSuite1 {
	AEGP_MaskOutlineSuite1 {
		AEGP_IsMaskOutlineOpen: Some(is_open),
		AEGP_GetMaskOutlineNumSegments: Some(num_segments),
		AEGP_GetMaskOutlineVertexInfo: Some(vertex_info),
		AEGP_SetMaskOutlineVertexInfo: Some(set_vertex),
		AEGP_CreateVertex: Some(create_vertex),
		AEGP_DeleteVertex: Some(delete_vertex),
	}
}

/// Wire version 3: adds `AEGP_SetMaskOutlineOpen`.
pub(super) const fn suite_2() -> AEGP_MaskOutlineSuite2 {
	AEGP_MaskOutlineSuite2 {
		AEGP_IsMaskOutlineOpen: Some(is_open),
		AEGP_SetMaskOutlineOpen: Some(set_open),
		AEGP_GetMaskOutlineNumSegments: Some(num_segments),
		AEGP_GetMaskOutlineVertexInfo: Some(vertex_info),
		AEGP_SetMaskOutlineVertexInfo: Some(set_vertex),
		AEGP_CreateVertex: Some(create_vertex),
		AEGP_DeleteVertex: Some(delete_vertex),
	}
}

/// Wire version 5: adds variable-width feather.
pub(super) const fn suite_3() -> AEGP_MaskOutlineSuite3 {
	AEGP_MaskOutlineSuite3 {
		AEGP_IsMaskOutlineOpen: Some(is_open),
		AEGP_SetMaskOutlineOpen: Some(set_open),
		AEGP_GetMaskOutlineNumSegments: Some(num_segments),
		AEGP_GetMaskOutlineVertexInfo: Some(vertex_info),
		AEGP_SetMaskOutlineVertexInfo: Some(set_vertex),
		AEGP_CreateVertex: Some(create_vertex),
		AEGP_DeleteVertex: Some(delete_vertex),
		AEGP_GetMaskOutlineNumFeathers: Some(num_feathers),
		AEGP_GetMaskOutlineFeatherInfo: Some(feather_info),
		AEGP_SetMaskOutlineFeatherInfo: Some(set_feather),
		AEGP_CreateMaskOutlineFeather: Some(create_feather),
		AEGP_DeleteMaskOutlineFeather: Some(delete_feather),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::MaskVertex;

	#[test]
	fn an_outline_reads_back_until_disposed() {
		let handle = publish(MaskPath {
			vertices: vec![
				MaskVertex {
					x: 1.0,
					y: 2.0,
					tan_out_x: 3.0,
					..Default::default()
				},
				MaskVertex::corner(9.0, 2.0),
			],
			closed: true,
			..Default::default()
		});
		let suite = suite_3();
		let (mut open, mut segments) = (9, 0);
		// SAFETY: every out-pointer is a local of the type the SDK asks for.
		unsafe {
			assert_eq!(suite.AEGP_IsMaskOutlineOpen.unwrap()(handle, &mut open), PF_Err_NONE);
			assert_eq!(
				suite.AEGP_GetMaskOutlineNumSegments.unwrap()(handle, &mut segments),
				PF_Err_NONE
			);
			assert_eq!((open, segments), (0, 2));
			let mut v: PF_PathVertex = std::mem::zeroed();
			let info = suite.AEGP_GetMaskOutlineVertexInfo.unwrap();
			assert_eq!(info(handle, 2, &mut v), PF_Err_NONE);
			assert_eq!(
				(v.x, v.tan_out_x),
				(1.0, 3.0),
				"the last point is the first, its tangent an offset"
			);
			assert_eq!(info(handle, 3, &mut v), PF_Err_INVALID_INDEX);
			assert!(dispose(handle as usize));
			assert_eq!(info(handle, 0, &mut v), PF_Err_BAD_CALLBACK_PARAM);
		}
	}
}
