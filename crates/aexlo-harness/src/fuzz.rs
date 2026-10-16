//! Parameter fuzzing (§9): variants with parameters drawn from each
//! parameter's declared range, judged only on what must hold for any value
//! (`finite`, `coverage`, `bounds`, no crash, no timeout).

use toml::Value;

use crate::input::DEFAULT_SIZE;
use crate::preset::{StrictFeature, Variant, VariantId};
use crate::worker::PluginInfo;

/// Checks a fuzzed variant runs.
pub const FUZZ_CHECKS: &[&str] = &["finite", "coverage", "bounds"];

/// One fuzzed variant and how to reproduce it.
#[derive(Clone, Debug)]
pub struct Case {
	pub variant: Variant,
	/// The case's own seed, derived from the run's seed and its index.
	pub seed: u64,
	pub index: usize,
	/// The preset the case was drawn from.
	pub base: String,
	/// The drawn values, keyed as a manifest would name them.
	pub params: Vec<(String, Value)>,
}

/// SplitMix64: small, seedable, reproducible.
struct Rng(u64);

impl Rng {
	fn next(&mut self) -> u64 {
		self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
		let mut z = self.0;
		z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
		z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
		z ^ (z >> 31)
	}

	/// Uniform in `[0, 1)`.
	fn unit(&mut self) -> f64 {
		(self.next() >> 11) as f64 / (1u64 << 53) as f64
	}

	fn range(&mut self, lo: f64, hi: f64) -> f64 {
		lo + (hi - lo) * self.unit()
	}
}

/// The seed of case `index` of a run seeded with `seed`.
pub fn case_seed(seed: u64, index: usize) -> u64 {
	Rng(seed ^ (index as u64).wrapping_mul(0x2545_f491_4f6c_dd1d)).next()
}

/// A value for one parameter, or `None` for kinds that are not fuzzed.
fn draw(rng: &mut Rng, param: &crate::worker::ParamInfo, size: (u32, u32)) -> Option<Value> {
	let round = |v: f64| (v * 1000.0).round() / 1000.0;
	Some(match param.kind.as_str() {
		"FloatSlider" | "FixedSlider" => {
			let (lo, hi) = param.range?;
			Value::Float(round(rng.range(lo, hi)))
		}
		"Slider" => {
			let (lo, hi) = param.range?;
			Value::Integer(rng.range(lo, hi + 1.0).floor().min(hi) as i64)
		}
		"Angle" => Value::Float(round(rng.range(-360.0, 360.0))),
		"Checkbox" => Value::Boolean(rng.next() & 1 == 1),
		"Popup" if !param.choices.is_empty() => Value::Integer(1 + (rng.next() % param.choices.len() as u64) as i64),
		"Point" => Value::Array(vec![
			Value::Float(round(rng.range(0.0, size.0 as f64))),
			Value::Float(round(rng.range(0.0, size.1 as f64))),
		]),
		"Color" => Value::Array((0..3).map(|_| Value::Float(round(rng.unit()))).collect()),
		_ => return None,
	})
}

/// `count` fuzzed variants of `base`, drawn from `info`'s parameters.
pub fn cases(base: &Variant, info: &PluginInfo, count: usize, seed: u64) -> Vec<Case> {
	let size = base.size.map_or(DEFAULT_SIZE, |[w, h]| (w, h));
	// Name parameters as a manifest would: by name when unique, else `#i`.
	let key = |p: &crate::worker::ParamInfo| {
		let unique = !p.name.trim().is_empty()
			&& info
				.params
				.iter()
				.filter(|q| q.name.trim().eq_ignore_ascii_case(p.name.trim()))
				.count() == 1;
		if unique {
			p.name.trim().to_string()
		} else {
			format!("#{}", p.index)
		}
	};
	(0..count)
		.map(|index| {
			let seed = case_seed(seed, index);
			let mut rng = Rng(seed);
			let params: Vec<(String, Value)> = info
				.params
				.iter()
				.filter(|p| !p.hidden)
				.filter_map(|p| draw(&mut rng, p, size).map(|v| (key(p), v)))
				.collect();
			let mut variant = base.clone();
			variant.params = params.clone();
			variant.checks = FUZZ_CHECKS.iter().map(|s| s.to_string()).collect();
			variant.strict = vec![StrictFeature::PoisonOutput, StrictFeature::GuardBands];
			variant.golden = None;
			variant.id = VariantId {
				display: match base.id.display.strip_suffix(']') {
					Some(axes) => format!("{axes},fuzz={index}]"),
					None => format!("{}[fuzz={index}]", base.id.display),
				},
				file_safe: format!("{}.fuzz-{index}", base.id.file_safe),
				golden: format!("{}.fuzz-{index}", base.id.golden),
			};
			Case {
				variant,
				seed,
				index,
				base: base.preset.clone(),
				params,
			}
		})
		.collect()
}

/// A TOML value as it would be written in a manifest.
fn toml_value(value: &Value) -> String {
	match value {
		Value::String(s) => format!("{s:?}"),
		Value::Array(items) => format!("[{}]", items.iter().map(toml_value).collect::<Vec<_>>().join(", ")),
		other => other.to_string(),
	}
}

/// A ready-to-paste `[[preset]]` reproducing a failing case.
pub fn preset_block(case: &Case, why: &str) -> String {
	let params: Vec<String> = case
		.params
		.iter()
		.map(|(k, v)| {
			let key = if k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
				k.clone()
			} else {
				format!("{k:?}")
			};
			format!("{key} = {}", toml_value(v))
		})
		.collect();
	format!(
		"# aexlo test --fuzz: case {} of '{}' (seed {}): {why}\n[[preset]]\nname = \"{}_fuzz_{}\"\ninherits = \"{}\"\nparams = {{ {} }}\n",
		case.index,
		case.base,
		case.seed,
		case.base,
		case.index,
		case.base,
		params.join(", ")
	)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::worker::ParamInfo;

	fn info() -> PluginInfo {
		let param = |index, name: &str, kind: &str, range, choices: &[&str]| ParamInfo {
			index,
			name: name.into(),
			kind: kind.into(),
			range,
			choices: choices.iter().map(|s| s.to_string()).collect(),
			hidden: false,
		};
		PluginInfo {
			params: vec![
				param(1, "Mode", "Popup", None, &["A", "B", "C"]),
				param(2, "Gain", "FloatSlider", Some((0.0, 4.0)), &[]),
				param(3, "Map", "Layer", None, &[]),
				param(4, "Blend Mode", "Checkbox", None, &[]),
				param(5, "Gain", "Slider", Some((1.0, 3.0)), &[]),
			],
			..PluginInfo::default()
		}
	}

	fn base() -> Variant {
		let manifest = crate::Manifest::parse(
			"[plugin]\nartifact = 'x'\n[[preset]]\nname = 'p'\ndepth = [8, 16]\n",
			std::path::Path::new("/m/aexlo.toml"),
		)
		.unwrap();
		manifest.variants().unwrap().remove(1)
	}

	#[test]
	fn cases_are_reproducible_and_in_range() {
		let a = cases(&base(), &info(), 20, 7);
		let b = cases(&base(), &info(), 20, 7);
		assert_eq!(a.len(), 20);
		for (x, y) in a.iter().zip(&b) {
			assert_eq!(x.params, y.params);
		}
		assert_eq!(a[3].variant.id.display, "p[depth=16,fuzz=3]");
		assert_eq!(a[3].variant.id.file_safe, "p.d16.fuzz-3");
		for case in &a {
			let keys: Vec<&str> = case.params.iter().map(|(k, _)| k.as_str()).collect();
			// The layer is not fuzzed; the duplicated name falls back to indices.
			assert_eq!(keys, ["Mode", "#2", "Blend Mode", "#5"]);
			let gain = case.params[1].1.as_float().unwrap();
			assert!((0.0..=4.0).contains(&gain));
			let mode = case.params[0].1.as_integer().unwrap();
			assert!((1..=3).contains(&mode));
			let slider = case.params[3].1.as_integer().unwrap();
			assert!((1..=3).contains(&slider));
			assert!(case.variant.golden.is_none());
		}
		assert_ne!(cases(&base(), &info(), 1, 8)[0].params, a[0].params);
	}

	#[test]
	fn failing_cases_print_as_presets() {
		let case = &cases(&base(), &info(), 1, 1)[0];
		let block = preset_block(case, "crash");
		assert!(block.starts_with("# aexlo test --fuzz: case 0 of 'p'"), "{block}");
		assert!(
			block.contains("name = \"p_fuzz_0\"\ninherits = \"p\"\nparams = { Mode = "),
			"{block}"
		);
		assert!(block.contains("\"Blend Mode\" = "), "{block}");
		// It parses back as a manifest preset.
		let text = format!("[plugin]\nartifact = 'x'\n[[preset]]\nname = 'p'\n{block}");
		crate::Manifest::parse(&text, std::path::Path::new("/m/aexlo.toml")).unwrap();
	}
}
