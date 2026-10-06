//! `aexlo test`'s verdict on a variant: render it as its checks need, run
//! the checks (§7), then compare against (or bless) its golden (§8).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::check;
use crate::exec::{Executor, RunOptions};
use crate::golden::{self, Verdict};
use crate::preset::{GoldenSource, Iterate, StrictFeature, Variant};
use crate::runner::{CheckStatus, Outcome, Run};

/// Checks `--strict` turns on, with every strict feature.
pub const STRICT_CHECKS: &[&str] = &["bounds", "allocations", "checkouts", "flags"];

/// Judges variants for `aexlo test`.
pub struct Judge {
	/// The manifest's directory: goldens and artifacts live under it.
	pub manifest_dir: PathBuf,
	/// Write missing or mismatched goldens instead of failing (§8.3).
	pub bless: bool,
	/// `--strict`: every strict feature and check on every variant.
	pub strict: bool,
	/// Goldens written during this run, so variants sharing one compare
	/// against it rather than rewriting it.
	golden_lock: Mutex<HashSet<PathBuf>>,
}

impl Judge {
	pub fn new(manifest_dir: impl Into<PathBuf>, bless: bool, strict: bool) -> Self {
		Self {
			manifest_dir: manifest_dir.into(),
			bless,
			strict,
			golden_lock: Mutex::new(HashSet::new()),
		}
	}

	/// `variant` with `--strict` applied.
	pub fn prepare(&self, variant: &Variant) -> Variant {
		let mut variant = variant.clone();
		if self.strict {
			variant.strict = StrictFeature::ALL.to_vec();
			for id in STRICT_CHECKS {
				if !variant.has_check(id) {
					variant.checks.push(id.to_string());
				}
			}
		}
		// A strict feature turns on the check that reports it.
		for feature in variant.strict.clone() {
			let id = feature.check();
			if feature != StrictFeature::PoisonOutput && !variant.has_check(id) {
				variant.checks.push(id.to_string());
			}
		}
		variant
	}

	/// How `variant` must run for its checks.
	pub fn options(variant: &Variant) -> RunOptions {
		RunOptions {
			renders: if variant.has_check("deterministic") { 2 } else { 1 },
			strict: variant.strict.clone(),
			calls: false,
		}
	}

	/// Render, check and compare `variant`.
	pub fn test(&self, executor: &mut dyn Executor, variant: &Variant) -> Run {
		let variant = self.prepare(variant);
		let mut run = match executor.run(&variant, &Self::options(&variant)) {
			Ok(output) => Run::rendered(variant.clone(), output),
			Err(failure) => {
				let mut run = Run::from_failure(variant.clone(), failure);
				self.explain_flags(&mut run);
				return run;
			}
		};
		if run.outcome != Outcome::Pass {
			return run;
		}
		let Some(frame) = run.frame.clone() else {
			run.harness_error("the run produced no frame");
			return run;
		};

		// `coverage` first: it claims the poison `finite` would also see.
		let findings = run.strict.clone();
		for id in [
			"coverage",
			"finite",
			"deterministic",
			"bounds",
			"allocations",
			"checkouts",
		] {
			if !variant.has_check(id) {
				continue;
			}
			let result = match id {
				"coverage" if !variant.has_strict(StrictFeature::PoisonOutput) => continue,
				"coverage" => check::coverage(&frame),
				"finite" => check::finite(&frame),
				"deterministic" => check::deterministic(&run.frames),
				strict => check::strict(strict, &findings),
			};
			run.checks.push(result);
		}
		if variant.has_check("iterate-parallel") {
			run.checks.push(self.iterate_parallel(executor, &variant, &frame));
		}
		if variant.has_check("flags") {
			run.checks.push(check::CheckResult::passed("flags"));
		}
		if let Some(failed) = run.checks.iter().find(|c| c.status == CheckStatus::Fail) {
			let reason = format!("{}: {}", failed.id, failed.message.as_deref().unwrap_or("failed"));
			run.fail(reason);
		}

		self.golden(&mut run, &frame);
		run
	}

	/// `iterate-parallel`: render again with serial iteration and compare.
	fn iterate_parallel(
		&self,
		executor: &mut dyn Executor,
		variant: &Variant,
		frame: &crate::Frame,
	) -> check::CheckResult {
		if variant.iterate == Iterate::Serial {
			return check::CheckResult::skipped("iterate-parallel", "the variant already iterates serially");
		}
		let mut serial = variant.clone();
		serial.iterate = Iterate::Serial;
		let options = RunOptions {
			renders: 1,
			..Self::options(variant)
		};
		match executor.run(&serial, &options) {
			Ok(output) => match output.frames.first() {
				Some(other) => check::agrees(
					"iterate-parallel",
					"the serial render",
					frame,
					other,
					check::ITERATE_TOLERANCE,
				),
				None => check::CheckResult::skipped("iterate-parallel", "the serial render was skipped"),
			},
			Err(failure) => check::CheckResult::failed(
				"iterate-parallel",
				format!("the serial render failed: {}", failure.message()),
			),
		}
	}

	/// `flags`: an error at a depth the plugin declares contradicts its flags.
	fn explain_flags(&self, run: &mut Run) {
		if run.outcome == Outcome::Error
			&& run.fault.is_none()
			&& run.variant.has_check("flags")
			&& run.variant.depth > 8
		{
			run.checks.push(check::CheckResult::failed(
				"flags",
				format!(
					"the plugin declares {} bpc but errors at {} bpc",
					run.variant.depth, run.variant.depth
				),
			));
		}
	}

	fn golden(&self, run: &mut Run, frame: &crate::Frame) {
		let Some(spec) = run.variant.golden.clone() else {
			return;
		};
		let id = if spec.source == GoldenSource::Ae {
			"parity"
		} else {
			"golden"
		};
		let path = golden::path(&run.variant, &spec, &self.manifest_dir);
		let shown = path
			.strip_prefix(&self.manifest_dir)
			.unwrap_or(&path)
			.display()
			.to_string();
		// Bless only what passed its checks: a flaky frame is no reference.
		let bless = self.bless && run.outcome == Outcome::Pass;

		let verdict = {
			let mut written = self.golden_lock.lock().expect("golden lock");
			// A golden blessed earlier in this run is compared, not rewritten.
			let bless = bless && !written.contains(&path);
			let verdict = golden::judge(frame, &path, &spec, bless);
			if matches!(verdict, Ok((Verdict::Blessed { .. }, _))) {
				written.insert(path.clone());
			}
			verdict
		};

		let artifacts = golden::artifacts_dir(&self.manifest_dir, &run.variant.id.file_safe);
		match verdict {
			Err(e) => run.harness_error(e.message),
			Ok((Verdict::Matched(_), _)) => run.checks.push(check::CheckResult::passed(id)),
			Ok((Verdict::Blessed { created }, _)) => {
				run.checks.push(check::CheckResult::passed(id));
				run.notes.push(format!(
					"{} {shown}",
					if created { "blessed new" } else { "re-blessed" }
				));
			}
			Ok((Verdict::Missing, _)) => {
				let saved = golden::write_artifacts(&artifacts, frame, None, &spec.tolerance);
				let hint = if spec.source == GoldenSource::Ae {
					"render it in After Effects".to_string()
				} else {
					"bless it with `aexlo test --bless`".to_string()
				};
				let message = match saved {
					Ok(dir) => format!("missing {shown} ({hint}; actual frame in {})", dir.display()),
					Err(e) => format!("missing {shown} ({hint}; {})", e.message),
				};
				run.checks.push(check::CheckResult::failed(id, message.clone()));
				run.fail(format!("{id}: {message}"));
			}
			Ok((Verdict::Mismatched(cmp), expected)) => {
				let summary = cmp.summary(&spec.tolerance);
				let message = match golden::write_artifacts(&artifacts, frame, expected.as_ref(), &spec.tolerance) {
					Ok(dir) => format!("differs from {shown}: {summary}; see {}", dir.display()),
					Err(e) => format!("differs from {shown}: {summary} ({})", e.message),
				};
				run.checks.push(check::CheckResult::failed(id, message.clone()));
				run.fail(format!("{id}: {message}"));
			}
		}
	}
}

/// `depth-consistency` (§7): within each group of runs that differ only in
/// depth, every depth's frame must match the shallowest one's within
/// [`check::DEPTH_TOLERANCE`]. Returns the indices of runs it changed.
pub fn depth_consistency(runs: &mut [Run]) -> Vec<usize> {
	use crate::preset::Axis;
	let key = |run: &Run| {
		let axes: Vec<String> = run
			.variant
			.axes
			.iter()
			.filter(|a| !matches!(a, Axis::Depth(_)))
			.map(|a| format!("{a:?}"))
			.collect();
		(run.variant.preset.clone(), axes)
	};
	// (preset, the non-depth axes) -> indices of the runs that share them.
	type Group = ((String, Vec<String>), Vec<usize>);
	let mut groups: Vec<Group> = Vec::new();
	for (i, run) in runs.iter().enumerate() {
		if !run.variant.has_check("depth-consistency") {
			continue;
		}
		let k = key(run);
		match groups.iter_mut().find(|(g, _)| *g == k) {
			Some((_, members)) => members.push(i),
			None => groups.push((k, vec![i])),
		}
	}

	let mut changed = Vec::new();
	for (_, members) in groups {
		let rendered: Vec<usize> = members
			.iter()
			.copied()
			.filter(|&i| runs[i].outcome == Outcome::Pass && runs[i].frame.is_some())
			.collect();
		let depths: HashSet<u32> = rendered.iter().map(|&i| runs[i].variant.depth).collect();
		if depths.len() < 2 {
			for &i in &members {
				runs[i].checks.push(check::CheckResult::skipped(
					"depth-consistency",
					"needs two depths that rendered",
				));
			}
			continue;
		}
		let Some(&reference) = rendered.iter().min_by_key(|&&i| runs[i].variant.depth) else {
			continue;
		};
		let expected = runs[reference].frame.clone().expect("rendered");
		let shallow = runs[reference].variant.depth;
		for &i in &rendered {
			if i == reference {
				runs[i].checks.push(check::CheckResult::passed("depth-consistency"));
				continue;
			}
			let frame = runs[i].frame.clone().expect("rendered");
			let result = check::agrees(
				"depth-consistency",
				&format!("the {shallow} bpc render"),
				&frame,
				&expected,
				check::DEPTH_TOLERANCE,
			);
			if result.status == CheckStatus::Fail {
				let reason = format!("depth-consistency: {}", result.message.as_deref().unwrap_or("differs"));
				runs[i].fail(reason);
				changed.push(i);
			}
			runs[i].checks.push(result);
		}
	}
	changed
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::exec::RunOutput;
	use crate::frame::Frame;
	use aexlo::{AnyLayer, PixelDepthKind};

	fn run(variant: &Variant, rgba: [f32; 4]) -> Run {
		let frame = Frame::new(AnyLayer::filled(variant.depth_kind(), 4, 4, rgba));
		Run::rendered(
			variant.clone(),
			RunOutput {
				frames: vec![frame],
				..RunOutput::default()
			},
		)
	}

	#[test]
	fn depth_consistency_compares_depths_of_one_variant() {
		let manifest = crate::Manifest::parse(
			"[plugin]\nartifact = 'x'\n[[preset]]\nname = 'p'\ndepth = [8, 16, 32]\nrender = ['smart', 'legacy']\nchecks = ['depth-consistency']\n",
			std::path::Path::new("/m/aexlo.toml"),
		)
		.unwrap();
		let variants = manifest.variants().unwrap();
		assert_eq!(variants[0].depth_kind(), PixelDepthKind::U8);
		// depth 8 / 16 / 32 for smart (0, 2, 4) and legacy (1, 3, 5).
		let mut runs: Vec<Run> = variants
			.iter()
			.enumerate()
			.map(|(i, v)| {
				let off = if i == 5 { 0.1 } else { 0.0 };
				run(v, [0.5 + off, 0.25, 0.75, 1.0])
			})
			.collect();
		let changed = depth_consistency(&mut runs);
		assert_eq!(changed, [5]);
		assert_eq!(runs[5].outcome, Outcome::Fail);
		assert!(
			runs[5]
				.message
				.as_deref()
				.unwrap()
				.starts_with("depth-consistency: differs from the 8 bpc render")
		);
		assert!(runs[..5].iter().all(|r| r.outcome == Outcome::Pass));
	}
}
