//! `PF_CacheOnLoadSuite1` ("PF Cache On Load Suite").
//!
//! A plugin calls `PF_SetNoCacheOnLoad` during global setup to tell AE not to
//! cache its load-time availability (e.g. licensing that can change between
//! launches). aexlo never caches plugin loads, so the request is satisfied by
//! construction and only acknowledged.

use crate::core::diagnostics::diag;
use after_effects_sys::{A_long, PF_CacheOnLoadSuite1, PF_Err, PF_Err_NONE, PF_ProgPtr};

unsafe extern "C" fn set_no_cache_on_load(effect_ref: PF_ProgPtr, effectAvailable: A_long) -> PF_Err {
	diag!("PF_CacheOnLoadSuite1/PF_SetNoCacheOnLoad",
		"effect_ref" => format!("{:#x}", effect_ref as usize),
		"effectAvailable" => effectAvailable,
	);
	let _ = (effect_ref, effectAvailable);
	PF_Err_NONE as PF_Err
}

pub(super) const fn create_cache_on_load_suite_1() -> PF_CacheOnLoadSuite1 {
	PF_CacheOnLoadSuite1 {
		PF_SetNoCacheOnLoad: Some(set_no_cache_on_load),
	}
}
