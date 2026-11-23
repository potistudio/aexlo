//! `AEGP Text Layer Suite` v1: the glyph outlines of the effect's layer when
//! the embedder made it a text layer
//! ([`PluginInstance::set_text_layer`](crate::PluginInstance::set_text_layer)).
//!
//! `AEGP_GetNewTextOutlines` snapshots the layer's outlines into a boxed list
//! the plugin owns until `AEGP_DisposeTextOutlines`. Each outline it hands out
//! is a `PF_PathOutlinePtr` the "PF Path Data Suite" reads, valid until that
//! dispose, with its tangents given as absolute points (see `path`). The time
//! is layer time, and null means now.

use crate::core::diagnostics::diag;
use crate::mask::MaskPath;
use crate::sys::*;

/// What an `AEGP_TextOutlinesH` points at.
struct TextOutlines {
	paths: Vec<MaskPath>,
}

/// The outline lists handed out and not yet disposed, by address. A plugin
/// that bails out of a failed call may pass an uninitialised handle.
static LIVE: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

fn live() -> std::sync::MutexGuard<'static, Vec<usize>> {
	LIVE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The list behind `outlinesH`, or `None` unless `AEGP_GetNewTextOutlines`
/// handed it out and it has not been disposed.
fn outlines<'a>(outlinesH: AEGP_TextOutlinesH) -> Option<&'a TextOutlines> {
	if !live().contains(&(outlinesH as usize)) {
		return None;
	}
	// SAFETY: a live handle is a `Box<TextOutlines>` this module leaked, freed
	// only by `dispose_text_outlines`.
	unsafe { (outlinesH as *const TextOutlines).as_ref() }
}

unsafe extern "C" fn get_new_text_outlines(
	layerH: AEGP_LayerH,
	layer_timePT: *const A_Time,
	outlinesPH: *mut AEGP_TextOutlinesH,
) -> A_Err {
	crate::core::ffi::guard("get_new_text_outlines", || {
		diag!("AEGP_TextLayerSuite1/AEGP_GetNewTextOutlines",
			"layerH" => format!("{:#x}", layerH as usize),
			"layer_timePT" => unsafe { layer_timePT.as_ref() }
				.map_or_else(|| "null".to_string(), |t| format!("{}/{}", t.value, t.scale)),
		);

		let Some(out) = (unsafe { outlinesPH.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = std::ptr::null_mut();
		if super::layer::Timeline::of(layerH.cast()).is_none() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let time = unsafe { layer_timePT.as_ref() }
			.copied()
			.and_then(crate::animation::TimePoint::from_sys);
		let Some(text) = super::layer::text_at(layerH, time) else {
			log::warn!("AEGP_TextLayerSuite1/AEGP_GetNewTextOutlines: not a text layer");
			return A_Err_GENERIC;
		};
		let boxed = Box::new(TextOutlines {
			paths: text.outlines().to_vec(),
		});
		*out = Box::into_raw(boxed) as AEGP_TextOutlinesH;
		live().push(*out as usize);
		PF_Err_NONE
	})
}

unsafe extern "C" fn dispose_text_outlines(outlinesH: AEGP_TextOutlinesH) -> A_Err {
	crate::core::ffi::guard("dispose_text_outlines", || {
		diag!("AEGP_TextLayerSuite1/AEGP_DisposeTextOutlines",
			"outlinesH" => format!("{:#x}", outlinesH as usize),
		);
		{
			let mut live = live();
			let Some(at) = live.iter().position(|&h| h == outlinesH as usize) else {
				log::warn!(
					"AEGP_TextLayerSuite1/AEGP_DisposeTextOutlines: {outlinesH:?} was not handed out, or was disposed"
				);
				return PF_Err_BAD_CALLBACK_PARAM;
			};
			live.swap_remove(at);
		}
		// SAFETY: it was live, so it is a `Box<TextOutlines>` from
		// `get_new_text_outlines`, and it is no longer listed.
		let list = unsafe { Box::from_raw(outlinesH as *mut TextOutlines) };
		drop(list);
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_num_text_outlines(outlinesH: AEGP_TextOutlinesH, num_outlinesPL: *mut A_long) -> A_Err {
	crate::core::ffi::guard("get_num_text_outlines", || {
		diag!("AEGP_TextLayerSuite1/AEGP_GetNumTextOutlines",
			"outlinesH" => format!("{:#x}", outlinesH as usize),
		);
		let (Some(list), Some(out)) = (outlines(outlinesH), unsafe { num_outlinesPL.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = list.paths.len() as A_long;
		PF_Err_NONE
	})
}

unsafe extern "C" fn get_indexed_text_outline(
	outlinesH: AEGP_TextOutlinesH,
	path_indexL: A_long,
	pathPP: *mut PF_PathOutlinePtr,
) -> A_Err {
	crate::core::ffi::guard("get_indexed_text_outline", || {
		diag!("AEGP_TextLayerSuite1/AEGP_GetIndexedTextOutline",
			"outlinesH" => format!("{:#x}", outlinesH as usize),
			"path_indexL" => path_indexL,
		);
		let (Some(list), Some(out)) = (outlines(outlinesH), unsafe { pathPP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(path) = usize::try_from(path_indexL).ok().and_then(|i| list.paths.get(i)) else {
			*out = std::ptr::null_mut();
			return PF_Err_INVALID_INDEX;
		};
		// The Path Data Suite reads a `PF_PathOutlinePtr` as a `MaskPath`.
		*out = path as *const MaskPath as PF_PathOutlinePtr;
		PF_Err_NONE
	})
}

pub(super) const fn suite_1() -> AEGP_TextLayerSuite1 {
	AEGP_TextLayerSuite1 {
		AEGP_GetNewTextOutlines: Some(get_new_text_outlines),
		AEGP_DisposeTextOutlines: Some(dispose_text_outlines),
		AEGP_GetNumTextOutlines: Some(get_num_text_outlines),
		AEGP_GetIndexedTextOutline: Some(get_indexed_text_outline),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mask::MaskVertex;

	#[test]
	fn outlines_are_counted_indexed_and_disposed() {
		let square = MaskPath {
			vertices: vec![
				MaskVertex::corner(0.0, 0.0),
				MaskVertex::corner(4.0, 0.0),
				MaskVertex::corner(4.0, 4.0),
			],
			closed: true,
			..Default::default()
		};
		let handle = Box::into_raw(Box::new(TextOutlines {
			paths: vec![square.clone(), square],
		})) as AEGP_TextOutlinesH;
		live().push(handle as usize);
		let suite = suite_1();
		unsafe {
			let mut count = 0;
			assert_eq!(suite.AEGP_GetNumTextOutlines.unwrap()(handle, &mut count), PF_Err_NONE);
			assert_eq!(count, 2);

			let mut path: PF_PathOutlinePtr = std::ptr::null_mut();
			assert_eq!(
				suite.AEGP_GetIndexedTextOutline.unwrap()(handle, 1, &mut path),
				PF_Err_NONE
			);
			assert_eq!((*(path as *const MaskPath)).segment_count(), 3);
			assert_eq!(
				suite.AEGP_GetIndexedTextOutline.unwrap()(handle, 2, &mut path),
				PF_Err_INVALID_INDEX
			);
			assert!(path.is_null());

			assert_eq!(suite.AEGP_DisposeTextOutlines.unwrap()(handle), PF_Err_NONE);
			// Disposed: no longer read or freed.
			assert_eq!(
				suite.AEGP_GetNumTextOutlines.unwrap()(handle, &mut count),
				PF_Err_BAD_CALLBACK_PARAM
			);
			assert_eq!(
				suite.AEGP_DisposeTextOutlines.unwrap()(handle),
				PF_Err_BAD_CALLBACK_PARAM
			);
		}
	}

	#[test]
	fn a_handle_that_was_never_handed_out_is_refused() {
		// What an uninitialised `AEGP_TextOutlinesH` local might hold.
		let garbage = 0xdead_beef_usize as AEGP_TextOutlinesH;
		let suite = suite_1();
		unsafe {
			assert_eq!(
				suite.AEGP_DisposeTextOutlines.unwrap()(garbage),
				PF_Err_BAD_CALLBACK_PARAM
			);
			assert_eq!(
				suite.AEGP_DisposeTextOutlines.unwrap()(std::ptr::null_mut()),
				PF_Err_BAD_CALLBACK_PARAM
			);
			let mut count = 0;
			assert_eq!(
				suite.AEGP_GetNumTextOutlines.unwrap()(garbage, &mut count),
				PF_Err_BAD_CALLBACK_PARAM
			);
		}
	}

	#[test]
	fn a_layer_that_is_not_an_instance_has_no_outlines() {
		let mut out: AEGP_TextOutlinesH = 1 as AEGP_TextOutlinesH;
		let err =
			unsafe { suite_1().AEGP_GetNewTextOutlines.unwrap()(std::ptr::null_mut(), std::ptr::null(), &mut out) };
		assert_eq!(err, PF_Err_BAD_CALLBACK_PARAM);
		assert!(out.is_null());
	}
}
