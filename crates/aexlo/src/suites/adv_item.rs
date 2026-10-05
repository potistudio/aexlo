//! "PF AE Adv Item Suite" callbacks: current time and re-render requests.
//!
//! Every callback delegates to the installed [`AppHost`](crate::host::app::AppHost)
//! (headless by default: time moves are no-ops and effects report as enabled).

use crate::core::diagnostics::diag;
use crate::host::app::with_app_host_ffi as with_host;
use after_effects_sys::{
	A_long, PF_AdvItemSuite1, PF_Boolean, PF_ContextH, PF_EffectWorld, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE,
	PF_InData, PF_Step, PF_Step_BACKWARD,
};

/// Fold `PF_Step` + a step count into a signed frame delta.
fn signed_steps(time_dir: PF_Step, num_stepsL: A_long) -> i32 {
	#[allow(clippy::unnecessary_cast)] // `PF_Step_*` is `u32` on macOS only.
	if time_dir as i64 == PF_Step_BACKWARD as i64 {
		-num_stepsL
	} else {
		num_stepsL
	}
}

/// The raw `effect_ref` of `in_data`, or `0` if `in_data` is null.
///
/// # Safety
/// `in_data` must be null or point to a valid `PF_InData`.
unsafe fn effect_of(in_data: *const PF_InData) -> usize {
	unsafe { in_data.as_ref() }.map_or(0, |d| d.effect_ref as usize)
}

unsafe extern "C" fn move_time_step(
	in_data: *mut PF_InData,
	world: *mut PF_EffectWorld,
	time_dir: PF_Step,
	num_stepsL: A_long,
) -> PF_Err {
	diag!("PF_AdvItemSuite/PF_MoveTimeStep",
		"in_data" => format!("{:#x}", in_data as usize),
		"world" => format!("{:#x}", world as usize),
		"time_dir" => time_dir,
		"num_stepsL" => num_stepsL,
	);
	let _ = world;

	with_host(|host| {
		host.move_time_step(unsafe { effect_of(in_data) }, signed_steps(time_dir, num_stepsL));
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn move_time_step_active_item(time_dir: PF_Step, num_stepsL: A_long) -> PF_Err {
	diag!("PF_AdvItemSuite/PF_MoveTimeStepActiveItem",
		"time_dir" => time_dir,
		"num_stepsL" => num_stepsL,
	);

	with_host(|host| {
		host.move_time_step(0, signed_steps(time_dir, num_stepsL));
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn touch_active_item() -> PF_Err {
	diag!("PF_AdvItemSuite/PF_TouchActiveItem");

	with_host(|host| {
		host.touch_active_item();
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn force_rerender(in_data: *mut PF_InData, world: *mut PF_EffectWorld) -> PF_Err {
	diag!("PF_AdvItemSuite/PF_ForceRerender",
		"in_data" => format!("{:#x}", in_data as usize),
		"world" => format!("{:#x}", world as usize),
	);
	let _ = world;

	with_host(|host| {
		host.force_rerender(unsafe { effect_of(in_data) });
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn effect_is_active_or_enabled(contextH: PF_ContextH, enabledPB: *mut PF_Boolean) -> PF_Err {
	diag!("PF_AdvItemSuite/PF_EffectIsActiveOrEnabled",
		"contextH" => format!("{:#x}", contextH as usize),
		"enabledPB" => format!("{:#x}", enabledPB as usize),
	);

	if enabledPB.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		unsafe { *enabledPB = host.effect_is_active_or_enabled(contextH as usize) as PF_Boolean };
		PF_Err_NONE as PF_Err
	})
}

/// Build the `PF_AdvItemSuite1` table.
pub(super) const fn create_adv_item_suite_1() -> PF_AdvItemSuite1 {
	PF_AdvItemSuite1 {
		PF_MoveTimeStep: Some(move_time_step),
		PF_MoveTimeStepActiveItem: Some(move_time_step_active_item),
		PF_TouchActiveItem: Some(touch_active_item),
		PF_ForceRerender: Some(force_rerender),
		PF_EffectIsActiveOrEnabled: Some(effect_is_active_or_enabled),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::host::app::{AppHost, TEST_APP_HOST};
	use after_effects_sys::PF_Step_FORWARD;
	use std::sync::{Arc, Mutex};

	#[derive(Clone, Default)]
	struct Steps(Arc<Mutex<Vec<i32>>>);
	impl AppHost for Steps {
		fn move_time_step(&self, _: usize, steps: i32) {
			self.0.lock().unwrap().push(steps);
		}
		fn effect_is_active_or_enabled(&self, _: usize) -> bool {
			false
		}
	}

	#[test]
	fn steps_are_signed_and_enabled_comes_from_host() {
		let rec = Steps::default();
		TEST_APP_HOST.set(Some(Box::new(rec.clone())));
		let suite = create_adv_item_suite_1();
		let mut enabled: PF_Boolean = 1;
		unsafe {
			suite.PF_MoveTimeStepActiveItem.unwrap()(PF_Step_FORWARD as PF_Step, 3);
			suite.PF_MoveTimeStepActiveItem.unwrap()(PF_Step_BACKWARD as PF_Step, 2);
			suite.PF_EffectIsActiveOrEnabled.unwrap()(std::ptr::null_mut(), &mut enabled);
		}
		TEST_APP_HOST.set(None);
		assert_eq!(*rec.0.lock().unwrap(), [3, -2]);
		assert_eq!(enabled, 0);
	}
}
