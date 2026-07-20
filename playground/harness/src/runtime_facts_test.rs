//! Regression tests for aexlo's own host/runtime implementation -- the
//! `PF_*` suites and callbacks aexlo emulates for whatever plugin it loads --
//! as opposed to any specific third-party plugin's business logic.
//!
//! `aexlo-probe` (`playground/probe`) is a purpose-built effect whose only
//! job is to feed every host service *fixed inputs* and record the *exact
//! output* as a `fact` during `GLOBAL_SETUP` (see `probe/src/checks.rs`).
//! Loading it once and reading its trace back lets us assert aexlo's runtime
//! is correct without a real After Effects install to diff against (that's
//! what `playground -- diff` is for, and it's a manual, not-CI step).
//!
//! Facts fall into two groups, and this file has one test per group:
//! - Implemented suites/callbacks: asserted against independently-derived
//!   expected values (std math, the documented premultiply formula, plain
//!   pixel-rect arithmetic) -- never copied from a previous run's output.
//! - Known gaps: a few `PF_UtilCallbacks` entries (`blend`, `gaussian_kernel`,
//!   ...) are still `stub_log!` no-ops, and the ANSI `sprintf` emulation only
//!   substitutes a bare first `%d`/`%s` (no width/precision/other
//!   conversions). These assertions pin down *today's* behavior so a silent
//!   regression is caught -- they are expected to start failing, on purpose,
//!   the day each gap gets a real implementation.

use aexlo::PluginInstance;
use serde_json::{Value, json};

use crate::summary::{self, Summary};

/// Load the probe in-process (no dlopen/cdylib needed) and read back the
/// facts `checks::run_all` wrote during `GLOBAL_SETUP`.
///
/// The probe's trace sink is a process-global `OnceLock` keyed off
/// `AEXLO_PROBE_TRACE` (see `probe/src/trace.rs`): the env var only takes
/// effect on the *first* access in this process. Each of this file's tests
/// calls this once, so keep it that way -- don't add a second call within the
/// same test binary, or the second load's env var will be silently ignored
/// and its facts will land in the first call's trace file instead.
fn run_probe_and_summarize() -> Summary {
	let trace_path = std::env::temp_dir().join(format!("aexlo-runtime-facts-{}.jsonl", std::process::id()));
	let _ = std::fs::remove_file(&trace_path);
	// SAFETY: single-threaded at this point, matching `playground run`'s own use of this var.
	unsafe { std::env::set_var("AEXLO_PROBE_TRACE", &trace_path) };

	let _instance = unsafe { PluginInstance::from_entry_raw(aexlo_probe::EffectMain as usize) }
		.expect("failed to load aexlo-probe in-process");

	let events = summary::load_events(&trace_path).expect("failed to read probe trace");
	summary::summarize(&events)
}

fn fact_str<'a>(summary: &'a Summary, name: &str) -> &'a str {
	summary
		.get(&format!("fact/{name}"))
		.unwrap_or_else(|| panic!("missing fact '{name}' in probe trace"))
}

fn fact_f64(summary: &Summary, name: &str) -> f64 {
	fact_str(summary, name)
		.parse()
		.unwrap_or_else(|e| panic!("fact '{name}' ({}) is not a plain number: {e}", fact_str(summary, name)))
}

fn fact_json(summary: &Summary, name: &str) -> Value {
	let raw = fact_str(summary, name);
	serde_json::from_str(raw).unwrap_or_else(|e| panic!("fact '{name}' ({raw}) is not valid JSON: {e}"))
}

#[test]
fn probe_facts_match_known_correct_values() {
	let summary = run_probe_and_summarize();

	// ---- ANSI math: real libm calls (`impl_math_sys!` in suites/ansi.rs) ----
	assert_eq!(fact_f64(&summary, "ansi.sin(0.5)"), 0.5f64.sin());
	assert_eq!(fact_f64(&summary, "ansi.cos(1.0)"), 1.0f64.cos());
	assert_eq!(fact_f64(&summary, "ansi.sqrt(2.0)"), 2.0f64.sqrt());
	assert_eq!(fact_f64(&summary, "ansi.pow(2.0,0.5)"), 2.0f64.powf(0.5));
	assert_eq!(fact_f64(&summary, "ansi.atan2(1.0,2.0)"), 1.0f64.atan2(2.0));
	assert_eq!(fact_f64(&summary, "ansi.hypot(3.0,4.0)"), 3.0f64.hypot(4.0));
	assert_eq!(fact_f64(&summary, "ansi.log(2.0)"), 2.0f64.ln());
	assert_eq!(fact_f64(&summary, "ansi.log10(1000.0)"), 1000.0f64.log10());
	assert_eq!(fact_f64(&summary, "ansi.fmod(7.5,2.0)"), 7.5f64 % 2.0);
	assert_eq!(fact_f64(&summary, "suite.ansi.sin(0.5)"), 0.5f64.sin());
	assert_eq!(fact_f64(&summary, "suite.ansi.pow(2,10)"), 2.0f64.powf(10.0));

	// ---- Host handle allocator: alloc/lock/write/read/resize cycle ----
	assert_eq!(fact_str(&summary, "handle.new(24).nonnull"), "true");
	assert_eq!(fact_f64(&summary, "handle.new(24).size"), 24.0);
	assert_eq!(fact_str(&summary, "handle.lock.write_read_roundtrip"), "true");

	let resize = fact_json(&summary, "handle.resize(24->64)");
	assert_eq!(resize["err"], 0);
	assert_eq!(resize["size"], 64);
	assert_eq!(resize["preserves_contents"], true);

	let suite_resize = fact_json(&summary, "suite.handle.new(32).resize(48)");
	assert_eq!(suite_resize["size"], 32);
	assert_eq!(suite_resize["resize_err"], 0);
	assert_eq!(suite_resize["resized_size"], 48);

	// ---- Worlds: allocation, fill (rect exclusivity), copy, iterate ----
	let new_world = fact_json(&summary, "world.new_world(16x8)");
	assert_eq!(new_world["width"], 16);
	assert_eq!(new_world["height"], 8);
	assert_eq!(new_world["cleared_to_zero"], true);
	assert!(
		new_world["rowbytes"].as_i64().unwrap() >= 16 * 4,
		"rowbytes must cover at least width * 4 bytes/pixel"
	);

	let full_color = json!({ "a": 255, "r": 10, "g": 20, "b": 30 });
	let full_fill = fact_json(&summary, "world.fill.full(10,20,30)");
	assert_eq!(full_fill["err"], 0);
	assert_eq!(full_fill["px(0,0)"], full_color);

	// PF_Rect right/bottom are exclusive: rect (2,1)-(6,5) covers x in [2,6), y in [1,5).
	let rect_color = json!({ "a": 200, "r": 99, "g": 88, "b": 77 });
	let rect_fill = fact_json(&summary, "world.fill.rect(2,1,6,5)");
	assert_eq!(rect_fill["inside(2,1)"], rect_color, "top-left corner is inclusive");
	assert_eq!(
		rect_fill["inside(5,4)"], rect_color,
		"(right-1, bottom-1) is the last covered pixel"
	);
	assert_eq!(
		rect_fill["outside(6,5)"], full_color,
		"right/bottom themselves are exclusive"
	);
	assert_eq!(
		rect_fill["outside(0,0)"], full_color,
		"outside the rect keeps the base fill"
	);

	let copy = fact_json(&summary, "world.copy.full");
	assert_eq!(copy["err"], 0);
	assert_eq!(copy["dst_equals_src"], true);

	// 16x8 = 128 pixels; both the legacy iterate callback and PF_Iterate8Suite2
	// must visit every pixel exactly once.
	assert_eq!(fact_json(&summary, "world.iterate(16x8)")["invocations"], 128);
	assert_eq!(fact_json(&summary, "suite.iterate8.iterate(16x8)")["invocations"], 128);

	// ---- Premultiply: `rgb' = rgb * a` (matte = 0, the colorless variant) ----
	// per the formula documented on `premultiply_world` in suites/fill_matte.rs.
	let a = 128.0 / 255.0;
	let round_to_u8 = |channel: f64| (channel / 255.0 * a * 255.0).round() as i64;
	let premultiply = fact_json(&summary, "world.premultiply(a=128,rgb=255,64,1)");
	assert_eq!(premultiply["err"], 0);
	assert_eq!(premultiply["px(0,0)"]["a"], 128, "alpha passes through unchanged");
	assert_eq!(premultiply["px(0,0)"]["r"], round_to_u8(255.0));
	assert_eq!(premultiply["px(0,0)"]["g"], round_to_u8(64.0));
	assert_eq!(premultiply["px(0,0)"]["b"], round_to_u8(1.0));
}

/// A handful of `PF_UtilCallbacks` entries are still `stub_log!` no-ops
/// (`blend`, `gaussian_kernel`, `convolve`, ...), and the ANSI `sprintf`
/// emulation only substitutes a bare first `%d`/`%s` -- no width, precision,
/// or any other conversion. This test pins down that *current* behavior so a
/// silent regression is caught; it's meant to start failing, on purpose, the
/// day each gap gets a real implementation -- see `crates/aexlo/src/suites/macros.rs`
/// (`stub_log!`) and `crates/aexlo/src/suites/ansi.rs` (`sprintf_sys`).
#[test]
fn probe_facts_document_known_runtime_gaps() {
	let summary = run_probe_and_summarize();

	// `blend` is a stub: it reports success but never touches `dst`, so a
	// freshly-cleared destination world stays all-zero instead of holding the
	// blend of (100,201) at ratio 0.5.
	let blend = fact_json(&summary, "world.blend(100,201,ratio=0.5)");
	assert_eq!(blend["err"], 0);
	assert_eq!(blend["px(0,0)"], json!({ "a": 0, "r": 0, "g": 0, "b": 0 }));

	// `gaussian_kernel` is a stub: it reports success but never writes a
	// diameter or kernel weights.
	let kernel = fact_json(&summary, "kernel.gaussian(r=2,normalized,long)");
	assert_eq!(kernel["err"], 0);
	assert_eq!(kernel["diameter"], 0);
	assert_eq!(kernel["sum"], 0);

	// sprintf: only a bare first `%d`/`%s` (no flags/width) gets substituted;
	// everything else in a multi-specifier format is left untouched, and
	// `%u`/`%x`/`%X`/`%o`/`%f`/`%e`/`%E`/`%g`/`%c` are not substituted at all.
	assert_eq!(fact_json(&summary, "ansi.sprintf.int")["out"], "42|%5d|%-5d|%05d|%+d");
	assert_eq!(fact_json(&summary, "ansi.sprintf.uint")["out"], "%u|%x|%X|%o");
	assert_eq!(
		fact_json(&summary, "ansi.sprintf.float")["out"],
		"%f|%.2f|%10.3f|%-10.1f|"
	);
	assert_eq!(fact_json(&summary, "ansi.sprintf.str")["out"], "[ae][%8s][%-8s]");
	assert_eq!(fact_json(&summary, "suite.ansi.sprintf")["out"], "7/x/%.2f");
}
