//! Host-side checks (§7): invariants evaluated on a run's frames and strict
//! findings. Each returns pass, fail with a message, or not applicable.

use aexlo::{AnyLayer, PixelDepthKind};

use crate::exec::StrictFindings;
use crate::frame::Frame;
use crate::preset::Tolerance;
pub use crate::runner::CheckResult;
#[cfg(test)]
use crate::runner::CheckStatus;

/// Tolerance of `depth-consistency`: 8, 16 and 32 bpc agree within this.
pub const DEPTH_TOLERANCE: f32 = 2.0 / 255.0;

/// Tolerance of `iterate-parallel`: threaded and serial iteration agree.
pub const ITERATE_TOLERANCE: f32 = 1.0 / 255.0;

fn pass(id: &str) -> CheckResult {
	CheckResult::passed(id)
}

fn fail(id: &str, message: impl Into<String>) -> CheckResult {
	CheckResult::failed(id, message)
}

fn skip(id: &str, why: impl Into<String>) -> CheckResult {
	CheckResult::skipped(id, why)
}

/// `(x, y)` of linear pixel `index` in `frame`.
fn at(frame: &Frame, index: usize) -> (u32, u32) {
	let w = frame.width().max(1) as usize;
	((index % w) as u32, (index / w) as u32)
}

/// `finite`: no NaN or infinity in a 32 bpc output. Pixels `coverage`
/// claims (the poison is a NaN) are not reported twice.
pub fn finite(frame: &Frame) -> CheckResult {
	if frame.depth() != PixelDepthKind::F32 {
		return skip("finite", "integer output is always finite");
	}
	let layer = frame.layer();
	let mut bad = 0usize;
	let mut first = None;
	for (i, rgba) in frame.unit_rgba().iter().enumerate() {
		if rgba.iter().any(|c| !c.is_finite()) && !aexlo::is_unwritten(layer, i) {
			bad += 1;
			first.get_or_insert(i);
		}
	}
	match first {
		None => pass("finite"),
		Some(i) => {
			let (x, y) = at(frame, i);
			fail(
				"finite",
				format!("{bad} pixel(s) hold NaN or infinity, first at ({x}, {y})"),
			)
		}
	}
}

/// `coverage`: every output pixel was written (needs `poison_output`).
pub fn coverage(frame: &Frame) -> CheckResult {
	let layer: &AnyLayer = frame.layer();
	let total = frame.width() as usize * frame.height() as usize;
	let mut bad = 0usize;
	let mut first = None;
	for i in 0..total {
		if aexlo::is_unwritten(layer, i) {
			bad += 1;
			first.get_or_insert(i);
		}
	}
	match first {
		None => pass("coverage"),
		Some(i) => {
			let (x, y) = at(frame, i);
			fail(
				"coverage",
				format!(
					"{bad} of {total} output pixel(s) never written ({:.1}%), first at ({x}, {y})",
					bad as f64 * 100.0 / total.max(1) as f64
				),
			)
		}
	}
}

/// `deterministic`: two renders of the same variant are identical.
pub fn deterministic(frames: &[Frame]) -> CheckResult {
	let [first, second, ..] = frames else {
		return skip("deterministic", "needs two renders");
	};
	let exact = Tolerance {
		max_abs: 0.0,
		max_bad: 0.0,
		min_psnr: None,
	};
	let cmp = second.compare(first, &exact);
	if cmp.passed {
		pass("deterministic")
	} else {
		fail(
			"deterministic",
			format!("the second render differs from the first: {}", cmp.summary(&exact)),
		)
	}
}

/// A check comparing this run against another rendering of the same variant
/// (`iterate-parallel`, `depth-consistency`).
pub fn agrees(id: &str, what: &str, frame: &Frame, other: &Frame, max_abs: f32) -> CheckResult {
	let tolerance = Tolerance {
		max_abs,
		max_bad: 0.0,
		min_psnr: None,
	};
	let cmp = frame.compare(other, &tolerance);
	if cmp.passed {
		pass(id)
	} else {
		fail(id, format!("differs from {what}: {}", cmp.summary(&tolerance)))
	}
}

/// The strict checks: `bounds`, `allocations`, `checkouts`.
pub fn strict(id: &str, findings: &StrictFindings) -> CheckResult {
	let list = match id {
		"bounds" => &findings.guard_violations,
		"allocations" => &findings.leaks,
		"checkouts" => &findings.checkout_violations,
		_ => return skip(id, "not a strict check"),
	};
	match &list[..] {
		[] => pass(id),
		[one] => fail(id, one.clone()),
		[first, rest @ ..] => fail(id, format!("{first} (and {} more)", rest.len())),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn frame(kind: PixelDepthKind, rgba: [f32; 4]) -> Frame {
		Frame::new(AnyLayer::filled(kind, 4, 4, rgba))
	}

	#[test]
	fn finite_only_judges_float_output() {
		assert_eq!(
			finite(&frame(PixelDepthKind::U8, [0.5; 4])).status,
			CheckStatus::Skipped
		);
		assert_eq!(finite(&frame(PixelDepthKind::F32, [0.5; 4])).status, CheckStatus::Pass);
		let bad = finite(&frame(PixelDepthKind::F32, [f32::NAN, 0.0, 0.0, 1.0]));
		assert_eq!(bad.status, CheckStatus::Fail);
		assert!(bad.message.unwrap().starts_with("16 pixel(s)"));
	}

	#[test]
	fn coverage_and_finite_split_the_poison() {
		let poisoned = frame(PixelDepthKind::F32, [f32::from_bits(aexlo::POISON_F32_BITS); 4]);
		assert_eq!(coverage(&poisoned).status, CheckStatus::Fail);
		assert_eq!(finite(&poisoned).status, CheckStatus::Pass, "claimed by coverage");
		assert_eq!(
			coverage(&frame(PixelDepthKind::F32, [0.5; 4])).status,
			CheckStatus::Pass
		);
	}

	#[test]
	fn deterministic_needs_identical_renders() {
		let a = frame(PixelDepthKind::U16, [0.5; 4]);
		let b = frame(PixelDepthKind::U16, [0.5, 0.5, 0.5, 0.25]);
		assert_eq!(deterministic(&[a.clone(), a.clone()]).status, CheckStatus::Pass);
		assert_eq!(deterministic(&[a.clone(), b]).status, CheckStatus::Fail);
		assert_eq!(deterministic(&[a]).status, CheckStatus::Skipped);
	}
}
