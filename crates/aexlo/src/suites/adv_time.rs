//! "PF AE Adv Time Suite" callbacks: time formatting and display preferences.
//!
//! The preferences come from the installed [`AppHost`](crate::host::app::AppHost)
//! ([`TimeDisplayPref`]); aexlo formats times itself from them, the way AE's
//! Time panel would.

use super::ffi::write_c_str;
use crate::core::diagnostics::diag;
use crate::host::app::{TimeDisplayMode, TimeDisplayPref, with_app_host_ffi as with_host};
use after_effects_sys::{
	A_Boolean, A_Time, A_char, A_long, A_u_long, PF_AdvTimeSuite1, PF_AdvTimeSuite2, PF_AdvTimeSuite3,
	PF_AdvTimeSuite4, PF_Boolean, PF_EffectWorld, PF_Err, PF_Err_BAD_CALLBACK_PARAM, PF_Err_NONE, PF_InData,
	PF_MAX_TIME_LEN, PF_TimeDisplayFormatFeetFrames, PF_TimeDisplayFormatFrames, PF_TimeDisplayFormatTimecode,
	PF_TimeDisplayPref, PF_TimeDisplayPrefVersion2, PF_TimeDisplayPrefVersion3,
};

/// Convert a frame count to the frame number shown by drop-frame timecode
/// (which skips the first `drop` numbers of every minute except each tenth).
fn drop_frame_number(frames: i64, timebase: i64) -> i64 {
	let drop = timebase / 15; // 2 at 30 fps, 4 at 60 fps
	let per_min = timebase * 60 - drop;
	let per_10min = timebase * 600 - drop * 9;
	let (tens, rem) = (frames.div_euclid(per_10min), frames.rem_euclid(per_10min));
	let extra = if rem > drop { drop * ((rem - drop) / per_min) } else { 0 };
	frames + drop * 9 * tens + extra
}

/// Format `value / scale` seconds the way AE's Time panel would under `pref`.
/// Returns `None` for a zero `scale`.
///
/// Durations ignore the composition start frame and the 0/1 frame-numbering
/// base, so a one-second duration reads `0:00:01:00` / `00030`.
pub(crate) fn format_time_text(pref: &TimeDisplayPref, value: i64, scale: u32, duration: bool) -> Option<String> {
	if scale == 0 {
		return None;
	}
	let timebase = pref.timebase.max(1) as i64;
	let mut frames = (value * timebase).div_euclid(scale as i64);
	if !duration {
		frames += pref.starting_frame as i64;
	}

	let sign = if frames < 0 { "-" } else { "" };
	let text = match pref.mode {
		TimeDisplayMode::Frames if pref.use_feet_frames => {
			let per_foot = pref.frames_per_foot.max(1) as i64;
			let base = if duration { 0 } else { pref.frames_start as i64 };
			let abs = frames.abs();
			format!("{sign}{}+{:02}", abs / per_foot, abs % per_foot + base)
		}
		TimeDisplayMode::Frames => {
			let base = if duration { 0 } else { pref.frames_start as i64 };
			format!("{sign}{:05}", (frames + base).abs())
		}
		TimeDisplayMode::Timecode => {
			let drop = !pref.non_drop && (timebase == 30 || timebase == 60);
			let abs = frames.abs();
			let shown = if drop { drop_frame_number(abs, timebase) } else { abs };
			let sep = if drop { ';' } else { ':' };
			let (h, m, s, f) = (
				shown / (timebase * 3600),
				shown / (timebase * 60) % 60,
				shown / timebase % 60,
				shown % timebase,
			);
			format!("{sign}{h}{sep}{m:02}{sep}{s:02}{sep}{f:02}")
		}
	};
	Some(text)
}

/// Format into the plugin's `PF_MAX_TIME_LEN + 1` byte buffer.
///
/// # Safety
/// `time_buf` must be null or point to `PF_MAX_TIME_LEN + 1` writable bytes.
unsafe fn format_into(
	time_valueUL: A_long,
	time_scaleL: A_u_long,
	durationB: PF_Boolean,
	time_buf: *mut A_char,
) -> PF_Err {
	if time_buf.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	with_host(|host| {
		let Some(text) = format_time_text(
			&host.time_display_pref(),
			time_valueUL as i64,
			time_scaleL,
			durationB != 0,
		) else {
			return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
		};
		let buf = unsafe { std::slice::from_raw_parts_mut(time_buf, PF_MAX_TIME_LEN as usize + 1) };
		write_c_str(buf, &text);
		PF_Err_NONE as PF_Err
	})
}

unsafe extern "C" fn format_time_active_item(
	time_valueUL: A_long,
	time_scaleL: A_u_long,
	durationB: PF_Boolean,
	time_buf: *mut A_char,
) -> PF_Err {
	diag!("PF_AdvTimeSuite/PF_FormatTimeActiveItem",
		"time_valueUL" => time_valueUL,
		"time_scaleL" => time_scaleL,
		"durationB" => durationB,
		"time_buf" => format!("{:#x}", time_buf as usize),
	);
	unsafe { format_into(time_valueUL, time_scaleL, durationB, time_buf) }
}

unsafe extern "C" fn format_time(
	in_data: *mut PF_InData,
	world: *mut PF_EffectWorld,
	time_valueUL: A_long,
	time_scaleL: A_u_long,
	durationB: PF_Boolean,
	time_buf: *mut A_char,
) -> PF_Err {
	diag!("PF_AdvTimeSuite/PF_FormatTime",
		"in_data" => format!("{:#x}", in_data as usize),
		"world" => format!("{:#x}", world as usize),
		"time_valueUL" => time_valueUL,
		"time_scaleL" => time_scaleL,
		"durationB" => durationB,
		"time_buf" => format!("{:#x}", time_buf as usize),
	);
	let _ = (in_data, world);
	unsafe { format_into(time_valueUL, time_scaleL, durationB, time_buf) }
}

/// aexlo has a single timeline, so layer and comp time coincide and
/// `comp_timeB` does not change the result.
unsafe extern "C" fn format_time_plus(
	in_data: *mut PF_InData,
	world: *mut PF_EffectWorld,
	time_valueUL: A_long,
	time_scaleL: A_u_long,
	comp_timeB: PF_Boolean,
	durationB: PF_Boolean,
	time_buf: *mut A_char,
) -> PF_Err {
	diag!("PF_AdvTimeSuite/PF_FormatTimePlus",
		"in_data" => format!("{:#x}", in_data as usize),
		"world" => format!("{:#x}", world as usize),
		"time_valueUL" => time_valueUL,
		"time_scaleL" => time_scaleL,
		"comp_timeB" => comp_timeB,
		"durationB" => durationB,
		"time_buf" => format!("{:#x}", time_buf as usize),
	);
	let _ = (in_data, world, comp_timeB);
	unsafe { format_into(time_valueUL, time_scaleL, durationB, time_buf) }
}

/// Write the host's preferences through `write` (one per struct version) and
/// the starting frame number into `starting_frame_num`.
fn get_pref(starting_frame_num: *mut A_long, write: impl FnOnce(&TimeDisplayPref)) -> PF_Err {
	with_host(|host| {
		let pref = host.time_display_pref();
		write(&pref);
		if let Some(out) = unsafe { starting_frame_num.as_mut() } {
			*out = pref.starting_frame;
		}
		PF_Err_NONE as PF_Err
	})
}

/// Clamp into the `A_char` fields of the pre-v3 preference structs.
fn to_char(v: i32) -> A_char {
	v.clamp(A_char::MIN as i32, A_char::MAX as i32) as A_char
}

fn display_mode(pref: &TimeDisplayPref) -> A_char {
	match pref.mode {
		TimeDisplayMode::Timecode => PF_TimeDisplayFormatTimecode as A_char,
		TimeDisplayMode::Frames => PF_TimeDisplayFormatFrames as A_char,
	}
}

unsafe extern "C" fn get_time_display_pref_3(
	tdp: *mut PF_TimeDisplayPrefVersion3,
	starting_frame_num: *mut A_long,
) -> PF_Err {
	diag!("PF_AdvTimeSuite/PF_GetTimeDisplayPref",
		"tdp" => format!("{:#x}", tdp as usize),
		"starting_frame_num" => format!("{:#x}", starting_frame_num as usize),
	);

	let Some(tdp) = (unsafe { tdp.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	get_pref(starting_frame_num, |p| {
		*tdp = PF_TimeDisplayPrefVersion3 {
			display_mode: display_mode(p),
			framemax: p.timebase,
			frames_per_foot: p.frames_per_foot,
			frames_start: to_char(p.frames_start),
			nondrop30B: p.non_drop as A_Boolean,
			honor_source_timecodeB: p.honor_source_timecode as A_Boolean,
			use_feet_framesB: p.use_feet_frames as A_Boolean,
		};
	})
}

unsafe extern "C" fn get_time_display_pref_2(
	tdp: *mut PF_TimeDisplayPrefVersion2,
	starting_frame_num: *mut A_long,
) -> PF_Err {
	diag!("PF_AdvTimeSuite2/PF_GetTimeDisplayPref",
		"tdp" => format!("{:#x}", tdp as usize),
		"starting_frame_num" => format!("{:#x}", starting_frame_num as usize),
	);

	let Some(tdp) = (unsafe { tdp.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	get_pref(starting_frame_num, |p| {
		*tdp = PF_TimeDisplayPrefVersion2 {
			display_mode: display_mode(p),
			framemax: to_char(p.timebase),
			frames_per_foot: to_char(p.frames_per_foot),
			frames_start: to_char(p.frames_start),
			nondrop30B: p.non_drop as A_Boolean,
			honor_source_timecodeB: p.honor_source_timecode as A_Boolean,
			use_feet_framesB: p.use_feet_frames as A_Boolean,
		};
	})
}

/// Version 1 folds feet+frames into the display format itself.
unsafe extern "C" fn get_time_display_pref_1(tdp: *mut PF_TimeDisplayPref, starting_frame_num: *mut A_long) -> PF_Err {
	diag!("PF_AdvTimeSuite1/PF_GetTimeDisplayPref",
		"tdp" => format!("{:#x}", tdp as usize),
		"starting_frame_num" => format!("{:#x}", starting_frame_num as usize),
	);

	let Some(tdp) = (unsafe { tdp.as_mut() }) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	get_pref(starting_frame_num, |p| {
		let format = if p.mode == TimeDisplayMode::Frames && p.use_feet_frames {
			PF_TimeDisplayFormatFeetFrames as A_char
		} else {
			display_mode(p)
		};
		*tdp = PF_TimeDisplayPref {
			time_display_format: format,
			framemax: to_char(p.timebase),
			nondrop30: p.non_drop as A_char,
			frames_per_foot: to_char(p.frames_per_foot),
		};
	})
}

/// Number of whole `time_step`s in `start_time`, plus one for a trailing
/// partial step when `include_partial_frameB` is set.
unsafe extern "C" fn time_count_frames(
	start_timeTP: *const A_Time,
	time_stepTP: *const A_Time,
	include_partial_frameB: A_Boolean,
	frame_countL: *mut A_long,
) -> PF_Err {
	diag!("PF_AdvTimeSuite/PF_TimeCountFrames",
		"start_timeTP" => format!("{:#x}", start_timeTP as usize),
		"time_stepTP" => format!("{:#x}", time_stepTP as usize),
		"include_partial_frameB" => include_partial_frameB,
		"frame_countL" => format!("{:#x}", frame_countL as usize),
	);

	let (Some(start), Some(step), Some(out)) = (
		unsafe { start_timeTP.as_ref() },
		unsafe { time_stepTP.as_ref() },
		unsafe { frame_countL.as_mut() },
	) else {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	};
	// start / step = (start.value * step.scale) / (start.scale * step.value)
	let num = start.value as i64 * step.scale as i64;
	let den = start.scale as i64 * step.value as i64;
	if den == 0 {
		return PF_Err_BAD_CALLBACK_PARAM as PF_Err;
	}
	let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
	let mut count = num.div_euclid(den);
	if include_partial_frameB != 0 && num.rem_euclid(den) != 0 {
		count += 1;
	}
	*out = count.clamp(A_long::MIN as i64, A_long::MAX as i64) as A_long;
	PF_Err_NONE as PF_Err
}

// The four versions share the formatting entries; only the preference struct
// (and v4's `PF_TimeCountFrames`) differ, so each gets an exactly-shaped table.

pub(super) const fn create_adv_time_suite_1() -> PF_AdvTimeSuite1 {
	PF_AdvTimeSuite1 {
		PF_FormatTimeActiveItem: Some(format_time_active_item),
		PF_FormatTime: Some(format_time),
		PF_FormatTimePlus: Some(format_time_plus),
		PF_GetTimeDisplayPref: Some(get_time_display_pref_1),
	}
}

pub(super) const fn create_adv_time_suite_2() -> PF_AdvTimeSuite2 {
	PF_AdvTimeSuite2 {
		PF_FormatTimeActiveItem: Some(format_time_active_item),
		PF_FormatTime: Some(format_time),
		PF_FormatTimePlus: Some(format_time_plus),
		PF_GetTimeDisplayPref: Some(get_time_display_pref_2),
	}
}

pub(super) const fn create_adv_time_suite_3() -> PF_AdvTimeSuite3 {
	PF_AdvTimeSuite3 {
		PF_FormatTimeActiveItem: Some(format_time_active_item),
		PF_FormatTime: Some(format_time),
		PF_FormatTimePlus: Some(format_time_plus),
		PF_GetTimeDisplayPref: Some(get_time_display_pref_3),
	}
}

pub(super) const fn create_adv_time_suite_4() -> PF_AdvTimeSuite4 {
	PF_AdvTimeSuite4 {
		PF_FormatTimeActiveItem: Some(format_time_active_item),
		PF_FormatTime: Some(format_time),
		PF_FormatTimePlus: Some(format_time_plus),
		PF_GetTimeDisplayPref: Some(get_time_display_pref_3),
		PF_TimeCountFrames: Some(time_count_frames),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::ffi::CStr;

	#[test]
	fn formats_timecode_frames_and_drop_frame() {
		let tc = TimeDisplayPref::default();
		assert_eq!(format_time_text(&tc, 65, 30, false).as_deref(), Some("0:00:02:05"));
		assert_eq!(
			format_time_text(&tc, 30 * 3661, 30, true).as_deref(),
			Some("1:01:01:00")
		);
		assert_eq!(format_time_text(&tc, 1, 0, false), None);

		let frames = TimeDisplayPref {
			mode: TimeDisplayMode::Frames,
			frames_start: 1,
			..tc
		};
		assert_eq!(format_time_text(&frames, 2, 1, false).as_deref(), Some("00061"));
		assert_eq!(format_time_text(&frames, 2, 1, true).as_deref(), Some("00060"));

		let feet = TimeDisplayPref {
			use_feet_frames: true,
			frames_start: 0,
			..frames
		};
		assert_eq!(format_time_text(&feet, 35, 30, false).as_deref(), Some("2+03"));

		// 29.97 drop-frame: frame 1800 is the first frame of minute 1, shown as ;02.
		let df = TimeDisplayPref { non_drop: false, ..tc };
		assert_eq!(format_time_text(&df, 1800, 30, false).as_deref(), Some("0;01;00;02"));
		assert_eq!(format_time_text(&df, 17982, 30, false).as_deref(), Some("0;10;00;00"));
	}

	#[test]
	fn suite_writes_buffer_and_counts_frames() {
		let suite = create_adv_time_suite_4();
		let mut buf = [0 as A_char; PF_MAX_TIME_LEN as usize + 1];
		let err = unsafe { suite.PF_FormatTimeActiveItem.unwrap()(3, 30, 0, buf.as_mut_ptr()) };
		assert_eq!(err, PF_Err_NONE as PF_Err);
		assert_eq!(unsafe { CStr::from_ptr(buf.as_ptr()) }.to_str().unwrap(), "0:00:00:03");

		let start = A_Time { value: 5, scale: 2 };
		let step = A_Time { value: 1, scale: 1 };
		let mut n = 0;
		unsafe { suite.PF_TimeCountFrames.unwrap()(&start, &step, 0, &mut n) };
		assert_eq!(n, 2);
		unsafe { suite.PF_TimeCountFrames.unwrap()(&start, &step, 1, &mut n) };
		assert_eq!(n, 3);
	}
}
