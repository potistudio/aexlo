//! World Transform and Batch Sampling facts, for what the SDK leaves
//! unwritten: Dissolve's pattern, the stencil and silhouette modes, blend
//! modes on float values above 1, the quality setting, and what
//! `get_batch_func` answers.
//!
//! Every world is 8x4 and filled from fixed formulas, at 8, 16 and 32 bpc.
//! Each fact records the whole destination, so a formula can be read off
//! the numbers rather than only compared.

use after_effects::sys as ae;
use serde_json::{Value, json};

use crate::checks::fact;

const W: i32 = 8;
const H: i32 = 4;

#[derive(Clone, Copy, PartialEq)]
enum Depth {
	Eight,
	Sixteen,
	Float,
}

impl Depth {
	const ALL: [Depth; 3] = [Depth::Eight, Depth::Sixteen, Depth::Float];

	fn name(self) -> &'static str {
		match self {
			Depth::Eight => "8",
			Depth::Sixteen => "16",
			Depth::Float => "32",
		}
	}

	fn format(self) -> ae::PF_PixelFormat {
		(match self {
			Depth::Eight => ae::PF_PixelFormat_ARGB32,
			Depth::Sixteen => ae::PF_PixelFormat_ARGB64,
			Depth::Float => ae::PF_PixelFormat_ARGB128,
		}) as ae::PF_PixelFormat
	}
}

/// A unit-scale ARGB pixel the patterns are written in.
type Unit = [f64; 4];

/// The source: rows 0 and 1 opaque, row 1 above 1 in float; rows 2 and 3
/// with partial alpha.
fn source(x: i32, y: i32) -> Unit {
	let i = (y * W + x) as f64;
	let c = |k: f64| ((i * k) % 11.0) / 10.0;
	let alpha = if y < 2 { 1.0 } else { ((i * 7.0) % 9.0) / 8.0 };
	let scale = if y == 1 { 2.0 } else { 1.0 };
	[
		alpha,
		c(3.0) * scale,
		c(5.0) * scale,
		((i * 2.0 + 1.0) % 11.0) / 10.0 * scale,
	]
}

/// The destination before each call: the same layout with other values.
fn backdrop(x: i32, y: i32) -> Unit {
	let i = (y * W + x) as f64;
	let c = |k: f64, o: f64| ((i * k + o) % 13.0) / 12.0;
	let alpha = if y < 2 { 1.0 } else { ((i * 5.0 + 3.0) % 9.0) / 8.0 };
	let scale = if y == 1 { 1.5 } else { 1.0 };
	[alpha, c(4.0, 1.0) * scale, c(2.0, 5.0) * scale, c(7.0, 2.0) * scale]
}

/// The non-separable layouts: rows 1 and 3 have a half-transparent source.
fn ns_source(x: i32, y: i32) -> Unit {
	let i = x as f64;
	let alpha = if y == 1 || y == 3 { 0.5 } else { 1.0 };
	[
		alpha,
		((i * 3.0 + 1.0) % 8.0) / 7.0,
		((i * 5.0 + 2.0) % 8.0) / 7.0,
		((i * 2.0 + 5.0) % 8.0) / 7.0,
	]
}

/// Rows 2 and 3 have a half-transparent destination.
fn ns_backdrop(x: i32, y: i32) -> Unit {
	let i = (x + y) as f64;
	let alpha = if y >= 2 { 0.5 } else { 1.0 };
	[
		alpha,
		((i * 7.0 + 3.0) % 8.0) / 7.0,
		((i * 3.0 + 6.0) % 8.0) / 7.0,
		((i + 2.0) % 8.0) / 7.0,
	]
}

/// Opaque, with values up to 2.
fn hdr_source(x: i32, y: i32) -> Unit {
	let i = (y * W + x) as f64;
	[
		1.0,
		((i * 3.0) % 9.0) / 4.0,
		((i * 5.0 + 1.0) % 9.0) / 4.0,
		((i * 7.0 + 2.0) % 9.0) / 4.0,
	]
}

fn hdr_backdrop(x: i32, y: i32) -> Unit {
	let i = (y * W + x) as f64;
	[
		1.0,
		((i * 2.0 + 3.0) % 9.0) / 4.0,
		((i * 4.0 + 5.0) % 9.0) / 4.0,
		((i + 7.0) % 9.0) / 4.0,
	]
}

/// The pattern for a transform: a smooth ramp, so filtering shows.
fn ramp(x: i32, y: i32) -> Unit {
	let (u, v) = (x as f64 / (W - 1) as f64, y as f64 / (H - 1) as f64);
	[1.0, u, v, 1.0 - u * v]
}

struct World {
	world: ae::PF_EffectWorld,
	depth: Depth,
}

struct Host<'a> {
	in_data: *mut ae::PF_InData,
	worlds: &'a ae::PF_WorldSuite2,
}

impl Host<'_> {
	unsafe fn new_world(&self, depth: Depth) -> Option<World> {
		let mut world: ae::PF_EffectWorld = unsafe { std::mem::zeroed() };
		let err = unsafe { self.worlds.PF_NewWorld?((*self.in_data).effect_ref, W, H, 1, depth.format(), &mut world) };
		(err == 0 && !world.data.is_null()).then_some(World { world, depth })
	}

	unsafe fn dispose(&self, mut world: World) {
		if let Some(dispose) = self.worlds.PF_DisposeWorld {
			unsafe { dispose((*self.in_data).effect_ref, &mut world.world) };
		}
	}
}

impl World {
	unsafe fn row(&self, y: i32) -> *mut u8 {
		unsafe { (self.world.data as *mut u8).add(y as usize * self.world.rowbytes as usize) }
	}

	unsafe fn fill(&mut self, pattern: fn(i32, i32) -> Unit) {
		for y in 0..H {
			for x in 0..W {
				let [a, r, g, b] = pattern(x, y);
				unsafe {
					let row = self.row(y);
					match self.depth {
						Depth::Eight => {
							let q = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
							*(row as *mut ae::PF_Pixel).add(x as usize) = ae::PF_Pixel {
								alpha: q(a),
								red: q(r),
								green: q(g),
								blue: q(b),
							};
						}
						Depth::Sixteen => {
							let q = |v: f64| (v.clamp(0.0, 1.0) * 32768.0).round() as u16;
							*(row as *mut ae::PF_Pixel16).add(x as usize) = ae::PF_Pixel16 {
								alpha: q(a),
								red: q(r),
								green: q(g),
								blue: q(b),
							};
						}
						Depth::Float => {
							*(row as *mut ae::PF_PixelFloat).add(x as usize) = ae::PF_PixelFloat {
								alpha: a as f32,
								red: r as f32,
								green: g as f32,
								blue: b as f32,
							};
						}
					}
				}
			}
		}
	}

	unsafe fn clear(&mut self) {
		unsafe { self.fill(|_, _| [0.0; 4]) };
	}

	/// Every pixel as `[a, r, g, b]` in the depth's own numbers.
	unsafe fn pixels(&self) -> Value {
		let mut out = Vec::new();
		for y in 0..H {
			for x in 0..W {
				unsafe {
					let row = self.row(y);
					out.push(match self.depth {
						Depth::Eight => {
							let p = *(row as *const ae::PF_Pixel).add(x as usize);
							json!([p.alpha, p.red, p.green, p.blue])
						}
						Depth::Sixteen => {
							let p = *(row as *const ae::PF_Pixel16).add(x as usize);
							json!([p.alpha, p.red, p.green, p.blue])
						}
						Depth::Float => {
							let p = *(row as *const ae::PF_PixelFloat).add(x as usize);
							let r = |v: f32| (v as f64 * 1e6).round() / 1e6;
							json!([r(p.alpha), r(p.red), r(p.green), r(p.blue)])
						}
					});
				}
			}
		}
		Value::Array(out)
	}
}

fn rect(right: i32, bottom: i32) -> ae::PF_Rect {
	ae::PF_Rect {
		left: 0,
		top: 0,
		right,
		bottom,
	}
}

fn mode(xfer: i32, opacity: u8, seed: i32, rgb_only: bool) -> ae::PF_CompositeMode {
	ae::PF_CompositeMode {
		xfer: xfer as ae::PF_TransferMode,
		rand_seed: seed,
		opacity,
		rgb_only: rgb_only as ae::PF_Boolean,
		// The same opacity on the 16-bit scale.
		opacitySu: ((opacity as u32 * 32768 + 127) / 255) as u16,
	}
}

/// Every transfer mode the SDK names, with its name for the fact.
const MODES: &[(&str, i32)] = &[
	("COPY", ae::PF_Xfer_COPY),
	("BEHIND", ae::PF_Xfer_BEHIND),
	("IN_FRONT", ae::PF_Xfer_IN_FRONT),
	("DISSOLVE", ae::PF_Xfer_DISSOLVE),
	("ADD", ae::PF_Xfer_ADD),
	("MULTIPLY", ae::PF_Xfer_MULTIPLY),
	("SCREEN", ae::PF_Xfer_SCREEN),
	("OVERLAY", ae::PF_Xfer_OVERLAY),
	("SOFT_LIGHT", ae::PF_Xfer_SOFT_LIGHT),
	("HARD_LIGHT", ae::PF_Xfer_HARD_LIGHT),
	("DARKEN", ae::PF_Xfer_DARKEN),
	("LIGHTEN", ae::PF_Xfer_LIGHTEN),
	("DIFFERENCE", ae::PF_Xfer_DIFFERENCE),
	("HUE", ae::PF_Xfer_HUE),
	("SATURATION", ae::PF_Xfer_SATURATION),
	("COLOR", ae::PF_Xfer_COLOR),
	("LUMINOSITY", ae::PF_Xfer_LUMINOSITY),
	("MULTIPLY_ALPHA", ae::PF_Xfer_MULTIPLY_ALPHA),
	("MULTIPLY_ALPHA_LUMA", ae::PF_Xfer_MULTIPLY_ALPHA_LUMA),
	("MULTIPLY_NOT_ALPHA", ae::PF_Xfer_MULTIPLY_NOT_ALPHA),
	("MULTIPLY_NOT_ALPHA_LUMA", ae::PF_Xfer_MULTIPLY_NOT_ALPHA_LUMA),
	("ADDITIVE_PREMUL", ae::PF_Xfer_ADDITIVE_PREMUL),
	("ALPHA_ADD", ae::PF_Xfer_ALPHA_ADD),
	("COLOR_DODGE", ae::PF_Xfer_COLOR_DODGE),
	("COLOR_BURN", ae::PF_Xfer_COLOR_BURN),
	("EXCLUSION", ae::PF_Xfer_EXCLUSION),
	("DIFFERENCE2", ae::PF_Xfer_DIFFERENCE2),
	("COLOR_DODGE2", ae::PF_Xfer_COLOR_DODGE2),
	("COLOR_BURN2", ae::PF_Xfer_COLOR_BURN2),
	("LINEAR_DODGE", ae::PF_Xfer_LINEAR_DODGE),
	("LINEAR_BURN", ae::PF_Xfer_LINEAR_BURN),
	("LINEAR_LIGHT", ae::PF_Xfer_LINEAR_LIGHT),
	("VIVID_LIGHT", ae::PF_Xfer_VIVID_LIGHT),
	("PIN_LIGHT", ae::PF_Xfer_PIN_LIGHT),
	("HARD_MIX", ae::PF_Xfer_HARD_MIX),
	("LIGHTER_COLOR", ae::PF_Xfer_LIGHTER_COLOR),
	("DARKER_COLOR", ae::PF_Xfer_DARKER_COLOR),
	("SUBTRACT", ae::PF_Xfer_SUBTRACT),
	("DIVIDE", ae::PF_Xfer_DIVIDE),
];

pub unsafe fn run(in_data: *mut ae::PF_InData) {
	let effect_ref = unsafe { (*in_data).effect_ref };
	let Some(worlds) = (unsafe { crate::checks::acquire(in_data, ae::kPFWorldSuite, ae::kPFWorldSuiteVersion2) })
	else {
		fact("xform.acquire", json!("no world suite"));
		return;
	};
	let host = Host {
		in_data,
		worlds: unsafe { &*(worlds.ptr as *const ae::PF_WorldSuite2) },
	};

	unsafe { batch_checks(in_data, &host) };

	let Some(guard) =
		(unsafe { crate::checks::acquire(in_data, ae::kPFWorldTransformSuite, ae::kPFWorldTransformSuiteVersion1) })
	else {
		fact("xform.acquire", json!("unavailable"));
		return;
	};
	let suite = unsafe { &*(guard.ptr as *const ae::PF_WorldTransformSuite1) };

	for depth in Depth::ALL {
		let (Some(mut src), Some(mut dst)) = (unsafe { host.new_world(depth) }, unsafe { host.new_world(depth) })
		else {
			fact(&format!("xform.{}.new_world", depth.name()), json!("failed"));
			continue;
		};
		unsafe { src.fill(source) };

		let mut transfer = |name: String, how: ae::PF_CompositeMode, flags: ae::PF_ModeFlags| {
			let Some(transfer_rect) = suite.transfer_rect else {
				return;
			};
			unsafe { dst.fill(backdrop) };
			let full = rect(W, H);
			let err = unsafe {
				transfer_rect(
					effect_ref,
					ae::PF_Quality_HI as ae::PF_Quality,
					flags,
					ae::PF_Field_FRAME as ae::PF_Field,
					&full,
					&src.world,
					&how,
					std::ptr::null(),
					0,
					0,
					&mut dst.world,
				)
			};
			fact(&name, json!({ "err": err, "px": unsafe { dst.pixels() } }));
		};

		let d = depth.name();
		let straight = ae::PF_MF_Alpha_STRAIGHT as ae::PF_ModeFlags;
		for &(name, xfer) in MODES {
			transfer(
				format!("xform.{d}.transfer[{name}]"),
				mode(xfer, 255, 0, false),
				straight,
			);
		}
		for &(name, xfer) in &MODES[..7] {
			transfer(
				format!("xform.{d}.transfer[{name},o128]"),
				mode(xfer, 128, 0, false),
				straight,
			);
		}
		for seed in [0, 1, 12345] {
			transfer(
				format!("xform.{d}.transfer[DISSOLVE,o128,seed{seed}]"),
				mode(ae::PF_Xfer_DISSOLVE, 128, seed, false),
				straight,
			);
			transfer(
				format!("xform.{d}.transfer[DISSOLVE,o255,seed{seed}]"),
				mode(ae::PF_Xfer_DISSOLVE, 255, seed, false),
				straight,
			);
		}
		transfer(
			format!("xform.{d}.transfer[COPY,rgb_only]"),
			mode(ae::PF_Xfer_COPY, 255, 0, true),
			straight,
		);
		for &(name, xfer) in &[
			("COPY", ae::PF_Xfer_COPY),
			("ADD", ae::PF_Xfer_ADD),
			("MULTIPLY", ae::PF_Xfer_MULTIPLY),
		] {
			transfer(
				format!("xform.{d}.transfer[{name},premul]"),
				mode(xfer, 255, 0, false),
				ae::PF_MF_Alpha_PREMUL as ae::PF_ModeFlags,
			);
		}

		// composite_rect, which takes the opacity on its own.
		if let Some(composite_rect) = suite.composite_rect {
			for &(name, xfer) in &[
				("COPY", ae::PF_Xfer_COPY),
				("BEHIND", ae::PF_Xfer_BEHIND),
				("IN_FRONT", ae::PF_Xfer_IN_FRONT),
				("DISSOLVE", ae::PF_Xfer_DISSOLVE),
				("ADD", ae::PF_Xfer_ADD),
				("MULTIPLY", ae::PF_Xfer_MULTIPLY),
				("SCREEN", ae::PF_Xfer_SCREEN),
				("MULTIPLY_ALPHA", ae::PF_Xfer_MULTIPLY_ALPHA),
			] {
				for opacity in [255, 128, 0] {
					unsafe { dst.fill(backdrop) };
					let mut full = rect(W, H);
					let err = unsafe {
						composite_rect(
							effect_ref,
							&mut full,
							opacity,
							&mut src.world,
							0,
							0,
							ae::PF_Field_FRAME as ae::PF_Field,
							xfer as ae::PF_XferMode,
							&mut dst.world,
						)
					};
					fact(
						&format!("xform.{d}.composite_rect[{name},o{opacity}]"),
						json!({ "err": err, "px": unsafe { dst.pixels() } }),
					);
				}
			}
		}

		// The non-separable modes, on layouts that keep one alpha at 1 at a
		// time (row 0 both opaque, row 1 the source half, row 2 the
		// destination half, row 3 both half), and opaque values up to 2.
		for (layout, over, under) in [
			(
				"ns",
				ns_source as fn(i32, i32) -> Unit,
				ns_backdrop as fn(i32, i32) -> Unit,
			),
			("hdr", hdr_source, hdr_backdrop),
		] {
			if layout == "hdr" && depth != Depth::Float {
				continue;
			}
			unsafe { src.fill(over) };
			for &(name, xfer) in &[
				("HUE", ae::PF_Xfer_HUE),
				("SATURATION", ae::PF_Xfer_SATURATION),
				("COLOR", ae::PF_Xfer_COLOR),
				("LUMINOSITY", ae::PF_Xfer_LUMINOSITY),
				("LIGHTER_COLOR", ae::PF_Xfer_LIGHTER_COLOR),
				("DARKER_COLOR", ae::PF_Xfer_DARKER_COLOR),
			] {
				let Some(transfer_rect) = suite.transfer_rect else {
					break;
				};
				unsafe { dst.fill(under) };
				let full = rect(W, H);
				let how = mode(xfer, 255, 0, false);
				let err = unsafe {
					transfer_rect(
						effect_ref,
						ae::PF_Quality_HI as ae::PF_Quality,
						straight as ae::PF_ModeFlags,
						ae::PF_Field_FRAME as ae::PF_Field,
						&full,
						&src.world,
						&how,
						std::ptr::null(),
						0,
						0,
						&mut dst.world,
					)
				};
				fact(
					&format!("xform.{d}.{layout}[{name}]"),
					json!({ "err": err, "px": unsafe { dst.pixels() } }),
				);
			}
		}

		// The quality setting: the same transform at LO and HI.
		unsafe { src.fill(ramp) };
		if let Some(transform_world) = suite.transform_world {
			let matrix = ae::PF_FloatMatrix {
				mat: [[1.3, 0.2, 0.0], [-0.15, 1.1, 0.0], [0.4, 0.3, 1.0]],
			};
			for (q, quality) in [("LO", ae::PF_Quality_LO), ("HI", ae::PF_Quality_HI)] {
				unsafe { dst.clear() };
				let full = rect(W, H);
				let how = mode(ae::PF_Xfer_COPY, 255, 0, false);
				let err = unsafe {
					transform_world(
						effect_ref,
						quality as ae::PF_Quality,
						straight as ae::PF_ModeFlags,
						ae::PF_Field_FRAME as ae::PF_Field,
						&src.world,
						&how,
						std::ptr::null(),
						&matrix,
						1,
						1,
						&full,
						&mut dst.world,
					)
				};
				fact(
					&format!("xform.{d}.transform_world[{q}]"),
					json!({ "err": err, "px": unsafe { dst.pixels() } }),
				);
			}
		}
		// copy and copy_hq into a smaller rectangle.
		for (name, copy) in [("copy", suite.copy), ("copy_hq", suite.copy_hq)] {
			let Some(copy) = copy else { continue };
			unsafe { dst.clear() };
			let (mut from, mut to) = (rect(W, H), rect(5, 3));
			let err = unsafe { copy(effect_ref, &mut src.world, &mut dst.world, &mut from, &mut to) };
			fact(
				&format!("xform.{d}.{name}[8x4->5x3]"),
				json!({ "err": err, "px": unsafe { dst.pixels() } }),
			);
		}

		unsafe {
			host.dispose(src);
			host.dispose(dst);
		}
	}
}

/// What `get_batch_func` and `get_batch_func16` answer, and what they leave
/// in the out pointer, which starts out holding a marker.
unsafe fn batch_checks(in_data: *mut ae::PF_InData, host: &Host) {
	let Some(guard) =
		(unsafe { crate::checks::acquire(in_data, ae::kPFBatchSamplingSuite, ae::kPFBatchSamplingSuiteVersion1) })
	else {
		fact("batch.acquire", json!("unavailable"));
		return;
	};
	let suite = unsafe { &*(guard.ptr as *const ae::PF_BatchSamplingSuite1) };
	let effect_ref = unsafe { (*in_data).effect_ref };
	const MARKER: usize = 0x5A5A_1234;
	for depth in [Depth::Eight, Depth::Sixteen] {
		let Some(mut world) = (unsafe { host.new_world(depth) }) else {
			continue;
		};
		unsafe { world.fill(ramp) };
		let mut params: ae::PF_SampPB = unsafe { std::mem::zeroed() };
		params.src = &mut world.world;
		params.x_radius = 1 << 16;
		params.y_radius = 1 << 16;
		params.area = 1 << 16;
		params.comp_mode = mode(ae::PF_Xfer_COPY, 255, 0, false);
		let begin = suite.begin_sampling.map(|begin| unsafe {
			begin(
				effect_ref,
				ae::PF_Quality_HI as _,
				ae::PF_MF_Alpha_STRAIGHT as _,
				&mut params,
			)
		});
		let (err, out) = match depth {
			Depth::Eight => match suite.get_batch_func {
				Some(get) => {
					let mut out = MARKER as ae::PF_BatchSampleFunc;
					let err = unsafe {
						get(
							effect_ref,
							ae::PF_Quality_HI as _,
							ae::PF_MF_Alpha_STRAIGHT as _,
							&params,
							&mut out,
						)
					};
					(json!(err), json!(out_value(out as usize, MARKER)))
				}
				None => (json!("null entry"), json!(null)),
			},
			_ => match suite.get_batch_func16 {
				Some(get) => {
					let mut out = MARKER as ae::PF_BatchSample16Func;
					let err = unsafe {
						get(
							effect_ref,
							ae::PF_Quality_HI as _,
							ae::PF_MF_Alpha_STRAIGHT as _,
							&params,
							&mut out,
						)
					};
					(json!(err), json!(out_value(out as usize, MARKER)))
				}
				None => (json!("null entry"), json!(null)),
			},
		};
		let end = suite.end_sampling.map(|end| unsafe {
			end(
				effect_ref,
				ae::PF_Quality_HI as _,
				ae::PF_MF_Alpha_STRAIGHT as _,
				&mut params,
			)
		});
		let name = if depth == Depth::Eight {
			"get_batch_func"
		} else {
			"get_batch_func16"
		};
		fact(
			&format!("batch.{name}"),
			json!({ "begin": begin, "err": err, "out": out, "end": end, "reserved": params.reserved }),
		);
		unsafe { host.dispose(world) };
	}
}

fn out_value(out: usize, marker: usize) -> &'static str {
	if out == marker {
		"untouched"
	} else if out == 0 {
		"null"
	} else {
		"a function"
	}
}
