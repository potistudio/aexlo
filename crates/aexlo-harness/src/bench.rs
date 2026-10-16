//! Benchmarks (§10): the one timing loop both `aexlo bench` and
//! `cargo bench -p aexlo-bench` use, its phase breakdown, and baselines.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aexlo::{CommandEvent, CommandPhase, ObserveLevel, Observer, PluginInstance};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::preset::BenchSpec;

/// Where one sample's time went (§10.2), in seconds.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Phases {
	/// `PF_Cmd_SMART_PRE_RENDER`.
	pub pre_render: f64,
	/// `PF_Cmd_SMART_RENDER` or `PF_Cmd_RENDER`.
	pub render: f64,
	/// `PF_Cmd_SMART_RENDER_GPU` and GPU device setup.
	pub gpu: f64,
	/// Wall time outside the plugin's entry point: input conversion, world
	/// setup, readback. What separates "the plugin got slower" from "aexlo
	/// got slower".
	pub host: f64,
}

/// The timed renders of one variant.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Samples {
	/// Wall-clock seconds per timed render, in order.
	pub wall: Vec<f64>,
	pub phases: Vec<Phases>,
}

/// Summary statistics of [`Samples`] (§10.1), in seconds.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
	pub samples: usize,
	pub median: f64,
	pub min: f64,
	pub mean: f64,
	pub p95: f64,
	/// Megapixels per second at the median.
	pub mpx_per_s: f64,
	/// The median of each phase.
	pub phases: Phases,
}

fn median(sorted: &[f64]) -> f64 {
	match sorted.len() {
		0 => 0.0,
		n if n % 2 == 1 => sorted[n / 2],
		n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0,
	}
}

fn sorted(values: impl Iterator<Item = f64>) -> Vec<f64> {
	let mut v: Vec<f64> = values.collect();
	v.sort_by(f64::total_cmp);
	v
}

impl Samples {
	/// Statistics for frames of `pixels` pixels.
	pub fn stats(&self, pixels: u64) -> Stats {
		let wall = sorted(self.wall.iter().copied());
		let n = wall.len();
		let median_wall = median(&wall);
		let phase = |f: fn(&Phases) -> f64| median(&sorted(self.phases.iter().map(f)));
		Stats {
			samples: n,
			median: median_wall,
			min: wall.first().copied().unwrap_or(0.0),
			mean: if n == 0 {
				0.0
			} else {
				wall.iter().sum::<f64>() / n as f64
			},
			p95: if n == 0 {
				0.0
			} else {
				wall[((n as f64 * 0.95).ceil() as usize).clamp(1, n) - 1]
			},
			mpx_per_s: if median_wall > 0.0 {
				pixels as f64 / median_wall / 1e6
			} else {
				0.0
			},
			phases: Phases {
				pre_render: phase(|p| p.pre_render),
				render: phase(|p| p.render),
				gpu: phase(|p| p.gpu),
				host: phase(|p| p.host),
			},
		}
	}
}

/// Collects command durations for the phase breakdown.
#[derive(Default)]
struct PhaseClock {
	ended: Mutex<Vec<(&'static str, f64)>>,
}

impl Observer for PhaseClock {
	fn command(&self, event: &CommandEvent) {
		if event.phase == CommandPhase::End
			&& let (Some(duration), Ok(mut ended)) = (event.duration, self.ended.lock())
		{
			ended.push((event.name, duration.as_secs_f64()));
		}
	}
}

/// Time `render` on a configured `fx` (§10.1): `spec.warmup` untimed
/// renders, then `spec.samples` timed ones, with the phase breakdown from
/// command events (`ObserveLevel::Commands`, the cheap level). Strict mode is
/// switched off for the duration.
pub fn measure(
	fx: &mut PluginInstance,
	spec: BenchSpec,
	mut render: impl FnMut(&mut PluginInstance) -> aexlo::Result<()>,
) -> aexlo::Result<Samples> {
	let strict = fx.strict();
	fx.set_strict(aexlo::Strict::default());
	let result = (|| {
		for _ in 0..spec.warmup {
			render(fx)?;
		}
		let clock = Arc::new(PhaseClock::default());
		fx.set_observer(Some(clock.clone()), ObserveLevel::Commands);
		let mut samples = Samples::default();
		for _ in 0..spec.samples.max(1) {
			clock.ended.lock().map(|mut e| e.clear()).ok();
			let started = Instant::now();
			render(fx)?;
			let wall = started.elapsed().as_secs_f64();
			let ended = clock.ended.lock().map(|e| e.clone()).unwrap_or_default();
			let mut phases = Phases::default();
			let mut inside = 0.0;
			for (name, seconds) in ended {
				inside += seconds;
				match name {
					"SMART_PRE_RENDER" => phases.pre_render += seconds,
					"SMART_RENDER" | "RENDER" => phases.render += seconds,
					"SMART_RENDER_GPU" | "GPU_DEVICE_SETUP" => phases.gpu += seconds,
					_ => {}
				}
			}
			phases.host = (wall - inside).max(0.0);
			samples.wall.push(wall);
			samples.phases.push(phases);
		}
		Ok(samples)
	})();
	fx.set_observer(None, ObserveLevel::Commands);
	fx.set_strict(strict);
	result
}

//==== Baselines ========================================================

/// The machine a baseline was measured on (§10.3).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint {
	pub os: String,
	pub arch: String,
	pub cpu: String,
	pub gpu: String,
	pub aexlo: String,
}

impl Fingerprint {
	/// This machine.
	pub fn current() -> Self {
		Self {
			os: std::env::consts::OS.to_string(),
			arch: std::env::consts::ARCH.to_string(),
			cpu: cpu_model(),
			gpu: aexlo::gpu_device_name().unwrap_or_else(|| "none".to_string()),
			aexlo: env!("CARGO_PKG_VERSION").to_string(),
		}
	}

	/// What differs from `other`, as `field: ours vs theirs`.
	pub fn differences(&self, other: &Fingerprint) -> Vec<String> {
		[
			("os", &self.os, &other.os),
			("arch", &self.arch, &other.arch),
			("cpu", &self.cpu, &other.cpu),
			("gpu", &self.gpu, &other.gpu),
			("aexlo", &self.aexlo, &other.aexlo),
		]
		.into_iter()
		.filter(|(_, a, b)| a != b)
		.map(|(field, a, b)| format!("{field}: {b} then, {a} now"))
		.collect()
	}
}

fn cpu_model() -> String {
	let output = |program: &str, args: &[&str]| {
		std::process::Command::new(program)
			.args(args)
			.output()
			.ok()
			.filter(|o| o.status.success())
			.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
			.filter(|s| !s.is_empty())
	};
	let model = if cfg!(target_os = "macos") {
		output("sysctl", &["-n", "machdep.cpu.brand_string"])
	} else if cfg!(target_os = "windows") {
		std::env::var("PROCESSOR_IDENTIFIER").ok()
	} else {
		std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|info| {
			info.lines()
				.find(|l| l.starts_with("model name"))
				.and_then(|l| l.split_once(':'))
				.map(|(_, v)| v.trim().to_string())
		})
	};
	model.unwrap_or_else(|| "unknown".to_string())
}

/// One variant's stored timings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Record {
	pub id: String,
	pub stats: Stats,
}

/// A saved set of timings later runs are compared against (§10.3).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Baseline {
	pub name: String,
	pub fingerprint: Fingerprint,
	pub records: Vec<Record>,
}

/// `.aexlo/baselines/<name>.json` next to the manifest.
pub fn baseline_path(manifest_dir: &Path, name: &str) -> PathBuf {
	manifest_dir
		.join(".aexlo")
		.join("baselines")
		.join(format!("{name}.json"))
}

/// Check a baseline name is usable as a file name.
pub fn validate_name(name: &str) -> Result<()> {
	if name.is_empty()
		|| !name
			.chars()
			.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
	{
		return Err(Error::invalid(format!(
			"baseline name '{name}' must be letters, digits, '-', '_' or '.'"
		)));
	}
	Ok(())
}

impl Baseline {
	pub fn save(&self, path: &Path) -> Result<()> {
		if let Some(dir) = path.parent() {
			std::fs::create_dir_all(dir).map_err(|e| Error::harness(format!("creating {}: {e}", dir.display())))?;
		}
		let text = serde_json::to_string_pretty(self).map_err(|e| Error::harness(e.to_string()))?;
		std::fs::write(path, text + "\n").map_err(|e| Error::harness(format!("writing {}: {e}", path.display())))
	}

	/// # Errors
	/// A missing baseline is invalid (exit 2); one that exists but cannot be
	/// read or decoded is a harness error (exit 3).
	pub fn load(path: &Path) -> Result<Self> {
		if !path.exists() {
			return Err(Error::invalid(format!(
				"no baseline at {} (save one with --save-baseline)",
				path.display()
			)));
		}
		let text =
			std::fs::read_to_string(path).map_err(|e| Error::harness(format!("reading {}: {e}", path.display())))?;
		serde_json::from_str(&text).map_err(|e| Error::harness(format!("decoding {}: {e}", path.display())))
	}

	pub fn record(&self, id: &str) -> Option<&Record> {
		self.records.iter().find(|r| r.id == id)
	}
}

/// How a run compares with its baseline record.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Delta {
	/// `median / baseline median - 1`.
	pub median: f64,
	/// `min / baseline min - 1`.
	pub min: f64,
	/// Both slower than the baseline by more than the threshold: noise in
	/// one alone (a one-off stall, a lucky run) does not count.
	pub regressed: bool,
}

/// Compare `now` with `then` at `threshold` (`0.05` for 5%).
pub fn compare(now: &Stats, then: &Stats, threshold: f64) -> Delta {
	let ratio = |a: f64, b: f64| if b > 0.0 { a / b - 1.0 } else { 0.0 };
	let median = ratio(now.median, then.median);
	let min = ratio(now.min, then.min);
	Delta {
		median,
		min,
		regressed: median > threshold && min > threshold,
	}
}

/// Parse `5%`, `5` (percent) or `0.05`... as a fraction. Values above 1
/// without `%` are read as percentages too.
pub fn parse_threshold(text: &str) -> Result<f64> {
	let trimmed = text.trim();
	let (number, percent) = match trimmed.strip_suffix('%') {
		Some(n) => (n, true),
		None => (trimmed, false),
	};
	let value: f64 = number
		.trim()
		.parse()
		.map_err(|_| Error::invalid(format!("threshold '{text}' is not a percentage")))?;
	if value.is_nan() || value < 0.0 {
		return Err(Error::invalid(format!("threshold '{text}' must be positive")));
	}
	Ok(if percent || value > 1.0 { value / 100.0 } else { value })
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn stats_are_order_statistics() {
		let samples = Samples {
			wall: vec![0.004, 0.001, 0.003, 0.002, 0.010],
			phases: vec![
				Phases {
					render: 0.001,
					host: 0.0005,
					..Phases::default()
				};
				5
			],
		};
		let stats = samples.stats(1_000_000);
		assert_eq!(stats.samples, 5);
		assert_eq!(stats.median, 0.003);
		assert_eq!(stats.min, 0.001);
		assert_eq!(stats.p95, 0.010);
		assert!((stats.mean - 0.004).abs() < 1e-12);
		assert!((stats.mpx_per_s - 1.0 / 0.003).abs() < 1e-6);
		assert_eq!(stats.phases.render, 0.001);
	}

	#[test]
	fn a_regression_needs_median_and_min_slower() {
		let then = Stats {
			median: 0.010,
			min: 0.008,
			..Stats::default()
		};
		let slower = Stats {
			median: 0.012,
			min: 0.009,
			..Stats::default()
		};
		assert!(compare(&slower, &then, 0.05).regressed);
		let noisy = Stats {
			median: 0.012,
			min: 0.008,
			..Stats::default()
		};
		assert!(!compare(&noisy, &then, 0.05).regressed, "a fast min means noise");
		assert!(!compare(&slower, &then, 0.5).regressed);
	}

	#[test]
	fn thresholds_parse_as_fractions() {
		assert_eq!(parse_threshold("5%").unwrap(), 0.05);
		assert_eq!(parse_threshold("5").unwrap(), 0.05);
		assert_eq!(parse_threshold("0.1").unwrap(), 0.1);
		assert!(parse_threshold("fast").is_err());
	}

	#[test]
	fn baselines_round_trip_and_explain_missing_files() {
		let dir = std::env::temp_dir().join(format!("aexlo-baseline-{}", std::process::id()));
		let path = baseline_path(&dir, "main");
		let baseline = Baseline {
			name: "main".into(),
			fingerprint: Fingerprint::current(),
			records: vec![Record {
				id: "p[depth=8]".into(),
				stats: Stats::default(),
			}],
		};
		baseline.save(&path).unwrap();
		assert_eq!(Baseline::load(&path).unwrap(), baseline);
		assert_eq!(
			Baseline::load(&baseline_path(&dir, "nope")).unwrap_err().kind,
			crate::ErrorKind::Invalid
		);
		std::fs::write(&path, "{").unwrap();
		assert_eq!(Baseline::load(&path).unwrap_err().kind, crate::ErrorKind::Harness);
		std::fs::remove_dir_all(&dir).unwrap();
		assert!(validate_name("main-2").is_ok());
		assert!(validate_name("../x").is_err());
	}
}
