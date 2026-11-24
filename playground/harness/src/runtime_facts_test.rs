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
//! Facts fall into three groups, and this file has one test per group:
//! - Implemented suites/callbacks: asserted against independently-derived
//!   expected values (std math, the documented premultiply formula, plain
//!   pixel-rect arithmetic) -- never copied from a previous run's output.
//! - Callback behaviour: checked against the current supported
//!   semantics, including approximate Gaussian weights.
//! - SmartRender callbacks (`probe/src/smart.rs`): rectangle arithmetic on a
//!   layer of known size and the pixel formats the SDK assigns to each depth.

use std::sync::LazyLock;

use serde_json::{Value, json};

use aexlo::{Depth8, Depth16, Depth32, Layer, Pixel, RenderPath};

use crate::summary::{self, Summary};

/// The input layer of the smart frames: larger than the probe's fixed
/// 10x9 checkout, so that request lies inside it.
const SMART_LAYER_W: u32 = 32;
const SMART_LAYER_H: u32 = 24;

/// Load the probe in-process (no dlopen/cdylib needed), render one smart
/// frame at each depth, and read back the facts: `checks::run_all` writes
/// its facts during `GLOBAL_SETUP`, `smart.rs` during the smart frames.
///
/// The probe's trace sink is a process-global `OnceLock` keyed off
/// `AEXLO_PROBE_TRACE` (see `probe/src/trace.rs`): the env var only takes
/// effect on the *first* access in this process. So the probe is loaded once
/// and every test reads that load through [`FACTS`]: a second load would find
/// its trace file deleted, with the sink still writing to the old one.
static FACTS: LazyLock<Summary> = LazyLock::new(run_probe_and_summarize);

fn run_probe_and_summarize() -> Summary {
	let trace_path = std::env::temp_dir().join(format!("aexlo-runtime-facts-{}.jsonl", std::process::id()));
	let _ = std::fs::remove_file(&trace_path);
	// SAFETY: only the one initialiser of `FACTS` runs this, and the tests that
	// could read the environment wait for it.
	unsafe { std::env::set_var("AEXLO_PROBE_TRACE", &trace_path) };

	let mut instance = unsafe { aexlo::Host::get().from_entry_raw(aexlo_probe::EffectMain as *const () as usize) }
		.expect("failed to load aexlo-probe in-process");

	// The in-process probe is the legacy build, which does not claim
	// SmartRender, so the smart path is forced. Its smart handlers are the
	// same code the smart build runs.
	let input = Layer::<Depth8>::from_fn(SMART_LAYER_W, SMART_LAYER_H, |x, y| Pixel {
		alpha: 255,
		red: (x * 8) as u8,
		green: (y * 10) as u8,
		blue: 64,
	});
	for bpc in [8, 16, 32] {
		match bpc {
			8 => instance.set_input_layer(input.clone()),
			16 => instance.set_input_layer(input.convert::<Depth16>()),
			_ => instance.set_input_layer(input.convert::<Depth32>()),
		}
		instance
			.render_with(RenderPath::Smart)
			.unwrap_or_else(|e| panic!("smart render at {bpc} bpc failed: {e}"));
	}
	drop(instance);

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
	let summary = &*FACTS;

	// ---- ANSI math: real libm calls (`impl_math_sys!` in suites/ansi.rs) ----
	assert_eq!(fact_f64(summary, "ansi.sin(0.5)"), 0.5f64.sin());
	assert_eq!(fact_f64(summary, "ansi.cos(1.0)"), 1.0f64.cos());
	assert_eq!(fact_f64(summary, "ansi.sqrt(2.0)"), 2.0f64.sqrt());
	assert_eq!(fact_f64(summary, "ansi.pow(2.0,0.5)"), 2.0f64.powf(0.5));
	assert_eq!(fact_f64(summary, "ansi.atan2(1.0,2.0)"), 1.0f64.atan2(2.0));
	assert_eq!(fact_f64(summary, "ansi.hypot(3.0,4.0)"), 3.0f64.hypot(4.0));
	assert_eq!(fact_f64(summary, "ansi.log(2.0)"), 2.0f64.ln());
	assert_eq!(fact_f64(summary, "ansi.log10(1000.0)"), 1000.0f64.log10());
	assert_eq!(fact_f64(summary, "ansi.fmod(7.5,2.0)"), 7.5f64 % 2.0);
	assert_eq!(fact_f64(summary, "suite.ansi.sin(0.5)"), 0.5f64.sin());
	assert_eq!(fact_f64(summary, "suite.ansi.pow(2,10)"), 2.0f64.powf(10.0));

	// ---- Host handle allocator: alloc/lock/write/read/resize cycle ----
	assert_eq!(fact_str(summary, "handle.new(24).nonnull"), "true");
	assert_eq!(fact_f64(summary, "handle.new(24).size"), 24.0);
	assert_eq!(fact_str(summary, "handle.lock.write_read_roundtrip"), "true");

	let resize = fact_json(summary, "handle.resize(24->64)");
	assert_eq!(resize["err"], 0);
	assert_eq!(resize["size"], 64);
	assert_eq!(resize["preserves_contents"], true);

	let suite_resize = fact_json(summary, "suite.handle.new(32).resize(48)");
	assert_eq!(suite_resize["size"], 32);
	assert_eq!(suite_resize["resize_err"], 0);
	assert_eq!(suite_resize["resized_size"], 48);

	// ---- Worlds: allocation, fill (rect exclusivity), copy, iterate ----
	let new_world = fact_json(summary, "world.new_world(16x8)");
	assert_eq!(new_world["width"], 16);
	assert_eq!(new_world["height"], 8);
	assert_eq!(new_world["cleared_to_zero"], true);
	assert!(
		new_world["rowbytes"].as_i64().unwrap() >= 16 * 4,
		"rowbytes must cover at least width * 4 bytes/pixel"
	);

	let full_color = json!({ "a": 255, "r": 10, "g": 20, "b": 30 });
	let full_fill = fact_json(summary, "world.fill.full(10,20,30)");
	assert_eq!(full_fill["err"], 0);
	assert_eq!(full_fill["px(0,0)"], full_color);

	// PF_Rect right/bottom are exclusive: rect (2,1)-(6,5) covers x in [2,6), y in [1,5).
	let rect_color = json!({ "a": 200, "r": 99, "g": 88, "b": 77 });
	let rect_fill = fact_json(summary, "world.fill.rect(2,1,6,5)");
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

	let copy = fact_json(summary, "world.copy.full");
	assert_eq!(copy["err"], 0);
	assert_eq!(copy["dst_equals_src"], true);

	// 16x8 = 128 pixels; both the legacy iterate callback and PF_Iterate8Suite2
	// must visit every pixel exactly once.
	assert_eq!(fact_json(summary, "world.iterate(16x8)")["invocations"], 128);
	assert_eq!(fact_json(summary, "suite.iterate8.iterate(16x8)")["invocations"], 128);

	// ---- Premultiply: `rgb' = rgb * a` (matte = 0, the colorless variant) ----
	// per the formula documented on `premultiply_world` in suites/fill_matte.rs.
	let a = 128.0 / 255.0;
	let round_to_u8 = |channel: f64| (channel / 255.0 * a * 255.0).round() as i64;
	let premultiply = fact_json(summary, "world.premultiply(a=128,rgb=255,64,1)");
	assert_eq!(premultiply["err"], 0);
	assert_eq!(premultiply["px(0,0)"]["a"], 128, "alpha passes through unchanged");
	assert_eq!(premultiply["px(0,0)"]["r"], round_to_u8(255.0));
	assert_eq!(premultiply["px(0,0)"]["g"], round_to_u8(64.0));
	assert_eq!(premultiply["px(0,0)"]["b"], round_to_u8(1.0));

	// ---- ANSI sprintf: what C's printf gives for each format ----
	// The 7.21.6.1 rules: `%5d` right-aligns in 5, `%-5d` left-aligns, `%05d`
	// zero-pads, `%f` keeps 6 decimals, `%e` keeps 6 and a 2-digit exponent,
	// `%g` keeps 6 significant digits. Only the text is checked: aexlo returns
	// `PF_Err_NONE`, not the length C's sprintf would.
	for (name, expected) in [
		("ansi.sprintf.int", "42|   42|42   |00042|+42"),
		("ansi.sprintf.uint", "255|ff|FF|377"),
		("ansi.sprintf.float", "3.141593|3.14|     3.142|3.1       |"),
		("ansi.sprintf.exp", "1.234568e+04|1.234568E+04|12345.7"),
		("ansi.sprintf.str", "[ae][      ae][ae      ]"),
		("ansi.sprintf.char", "A%"),
		("suite.ansi.sprintf", "7/x/1.50"),
	] {
		assert_eq!(fact_json(summary, name)["out"], expected, "{name}");
	}
}

/// Exercise callback output.
#[test]
fn probe_facts_document_known_runtime_gaps() {
	let summary = &*FACTS;

	// The midpoint of 100 and 201 rounds to 151, including the opaque alpha.
	let blend = fact_json(summary, "world.blend(100,201,ratio=0.5)");
	assert_eq!(blend["err"], 0);
	assert_eq!(blend["px(0,0)"], json!({ "a": 255, "r": 151, "g": 151, "b": 151 }));

	// Approximate normalised weights have the requested diameter and scale.
	let kernel = fact_json(summary, "kernel.gaussian(r=2,normalized,long)");
	assert_eq!(kernel["err"], 0);
	assert_eq!(kernel["diameter"], 5);
	assert!((kernel["sum"].as_i64().unwrap() - 65536).abs() <= 25);
}

/// `PF_PixelFormat` as the probe prints it: the four bytes, high byte first.
fn fourcc(format: aexlo::PixelFormat) -> String {
	format.fourcc().to_be_bytes().iter().map(|&b| b as char).collect()
}

#[test]
fn probe_smart_render_facts_match_known_correct_values() {
	let summary = &*FACTS;

	// ---- Pre-render checkouts against the 32x24 input layer ----
	// A request inside the layer comes back as requested.
	let inner = fact_json(summary, "smart.checkout_layer.inner(2,3,10,9)");
	assert_eq!(inner["err"], 0);
	assert_eq!(inner["result_rect"], json!({ "l": 2, "t": 3, "r": 10, "b": 9 }));
	assert_eq!(inner["max_matches_main"], true, "one layer has one max_result_rect");

	// A request 4 pixels past every edge is clipped to the layer: the result
	// is the layer's max_result_rect.
	let overhang = fact_json(summary, "smart.checkout_layer.overhang(max+-4)");
	assert_eq!(overhang["err"], 0);
	assert_eq!(overhang["result_minus_max"], json!([0, 0, 0, 0]));

	// An empty request covers no pixels.
	let empty = fact_json(summary, "smart.checkout_layer.empty(5,5,5,5)");
	let r = &empty["result_rect"];
	let area =
		(r["r"].as_i64().unwrap() - r["l"].as_i64().unwrap()) * (r["b"].as_i64().unwrap() - r["t"].as_i64().unwrap());
	assert_eq!(area, 0, "{empty}");

	// The probe has 7 parameters; index 99 is not one of them.
	assert_ne!(fact_json(summary, "smart.checkout_layer.invalid_index(99)")["err"], 0);

	assert_eq!(fact_json(summary, "smart.guid_mix_in(4 bytes)")["err"], 0);
	assert_eq!(fact_json(summary, "smart.guid_mix_in(0 bytes,null)")["err"], 0);

	// ---- Render ----
	// SMART_RENDER receives the block SMART_PRE_RENDER returned.
	assert_eq!(
		fact_json(summary, "smart.pre_render_data"),
		json!({ "nonnull": true, "magic_ok": true })
	);

	// Pre-render never checked out id 77, so there are no pixels for it.
	let undeclared = fact_json(summary, "smart.checkout_layer_pixels.undeclared_id(77)");
	assert_ne!(undeclared["err"], 0);
	assert_ne!(undeclared["pixels"], "a world");

	let inner_pixels = fact_json(summary, "smart.checkout_layer_pixels.inner(2,3,10,9)");
	assert_eq!(inner_pixels["err"], 0);
	assert_eq!(inner_pixels["nonnull"], true);
	// Checking in a checked-out id succeeds. What a second checkin returns
	// is not specified, so only the first is asserted.
	assert_eq!(fact_json(summary, "smart.checkin_layer_pixels.twice")["first"], 0);

	// Each project depth has one SDK pixel format, for input and output alike;
	// 16 bpc worlds carry PF_WorldFlag_DEEP and 8 bpc worlds do not.
	let deep = aexlo::sys::PF_WorldFlag_DEEP as i64;
	for (bpc, format) in [
		(8, aexlo::PixelFormat::ARGB32),
		(16, aexlo::PixelFormat::ARGB64),
		(32, aexlo::PixelFormat::ARGB128),
	] {
		for which in ["checkout_layer_pixels[main]", "checkout_output"] {
			let name = format!("smart.{bpc}.{which}");
			let layout = fact_json(summary, &name);
			assert_eq!(layout["err"], 0, "{name}");
			assert_eq!(layout["nonnull"], true, "{name}");
			assert_eq!(layout["format_err"], 0, "{name}");
			assert_eq!(layout["format"], fourcc(format), "{name}");
			let flags = layout["world_flags"].as_i64().unwrap();
			match bpc {
				8 => assert_eq!(flags & deep, 0, "{name}"),
				16 => assert_eq!(flags & deep, deep, "{name}"),
				_ => {}
			}
		}
	}
}
