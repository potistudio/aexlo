//! The world transforms at every depth: deep results are the 8-bit ones at a
//! finer step, 32 bpc keeps what is out of range, and nothing leaves a world.

use std::sync::{Arc, Mutex};

use super::super::fill_matte::{fill_8_sys, premultiply_sys};
use super::super::world::{alloc_world, free_world};
use super::*;

const W: i32 = 9;
const H: i32 = 7;

trait Deep: Channels {
	const FORMAT: u32;
}

impl Deep for PF_Pixel8 {
	const FORMAT: u32 = PF_PixelFormat_ARGB32 as u32;
}

impl Deep for PF_Pixel16 {
	const FORMAT: u32 = PF_PixelFormat_ARGB64 as u32;
}

impl Deep for PF_PixelFloat {
	const FORMAT: u32 = PF_PixelFormat_ARGB128 as u32;
}

/// A world as `PF_NewWorld` makes one, freed on drop.
struct Owned<P: Deep> {
	world: PF_EffectWorld,
	pixel: std::marker::PhantomData<P>,
}

impl<P: Deep> Owned<P> {
	/// Filled with `f`'s channels, given out of 255.
	fn new(width: i32, height: i32, f: impl Fn(i32, i32) -> [f64; 4]) -> Self {
		let world = alloc_world(width, height, std::mem::size_of::<P>() as i32, P::FORMAT);
		let raw = RawWorld::<P>::from(&world);
		for y in 0..height {
			for x in 0..width {
				// SAFETY: inside the world just made.
				unsafe { raw.write(x, y, P::store(f(x, y).map(|v| v / 255.0 * P::MAX))) };
			}
		}
		Self {
			world,
			pixel: std::marker::PhantomData,
		}
	}

	/// Out of 255.
	fn get(&self, x: i32, y: i32) -> [f64; 4] {
		// SAFETY: `read` checks its bounds.
		unsafe { RawWorld::<P>::from(&self.world).read(x, y) }
			.channels()
			.map(|v| v / P::MAX * 255.0)
	}

	fn pixels(&self) -> Vec<[f64; 4]> {
		(0..self.world.height)
			.flat_map(|y| (0..self.world.width).map(move |x| self.get(x, y)))
			.collect()
	}
}

impl<P: Deep> Drop for Owned<P> {
	fn drop(&mut self) {
		// SAFETY: made by `alloc_world`, and freed once.
		unsafe { free_world(&mut self.world) };
	}
}

fn picture(seed: i32) -> impl Fn(i32, i32) -> [f64; 4] {
	move |x, y| {
		let of = |k: i32| f64::from((x * 31 + y * 57 + seed * 13 + k * 71).rem_euclid(256));
		// Enough alpha that a step of rounding is not magnified.
		[
			f64::from(96 + (x * 23 + y * 11 + seed).rem_euclid(160)),
			of(1),
			of(2),
			of(3),
		]
	}
}

fn mode(xfer: i32, opacity: u8) -> PF_CompositeMode {
	PF_CompositeMode {
		xfer: xfer as _,
		rand_seed: 0,
		opacity,
		rgb_only: 0,
		opacitySu: (f64::from(opacity) / 255.0 * 32768.0).round() as _,
	}
}

#[derive(Debug, Clone, Copy)]
enum Op {
	Composite(i32),
	Blend,
	Convolve,
	Copy,
	CopyHq,
	Transfer(i32),
	MaskedTransfer,
	Transform,
}

fn run<P: Deep>(op: Op) -> Vec<[f64; 4]> {
	let null = std::ptr::null_mut();
	let mut a = Owned::<P>::new(W, H, picture(1));
	let mut b = Owned::<P>::new(W, H, picture(2));
	let mut out = Owned::<P>::new(W, H, picture(3));
	let masked = Owned::<PF_Pixel8>::new(W, H, picture(4));
	let mask = PF_MaskWorld {
		mask: masked.world,
		offset: PF_Point { v: 1, h: 2 },
		what_is_mask: 0,
	};
	let (a, b, d) = (&raw mut a.world, &raw mut b.world, &raw mut out.world);
	let straight = PF_MF_Alpha_STRAIGHT as PF_ModeFlags;
	let frame = PF_Field_FRAME as PF_Field;
	// SAFETY: every pointer is a live local or null.
	let err = unsafe {
		match op {
			Op::Composite(x) => composite_rect_sys(null, std::ptr::null_mut(), 200, a, 1, -1, frame, x as _, d),
			Op::Blend => blend_sys(null, a, b, 20000, d),
			Op::Convolve => {
				let mut kernel = [1i32, 2, 1, 2, 4, 2, 1, 2, 1];
				let k = kernel.as_mut_ptr().cast::<c_void>();
				convolve_sys(null, a, std::ptr::null(), 0, 3, k, k, k, k, d)
			}
			Op::Copy => copy_sys(null, a, d, std::ptr::null_mut(), std::ptr::null_mut()),
			Op::CopyHq => {
				let mut to = PF_Rect {
					left: 0,
					top: 0,
					right: 6,
					bottom: 5,
				};
				copy_hq_sys(null, a, d, std::ptr::null_mut(), &mut to)
			}
			Op::Transfer(x) => transfer_rect_sys(
				null,
				0,
				straight,
				frame,
				std::ptr::null(),
				a,
				&mode(x, 180),
				std::ptr::null(),
				-2,
				1,
				d,
			),
			Op::MaskedTransfer => transfer_rect_sys(
				null,
				0,
				straight,
				frame,
				std::ptr::null(),
				a,
				&mode(PF_Xfer_IN_FRONT, 255),
				&mask,
				0,
				0,
				d,
			),
			Op::Transform => {
				let matrix = PF_FloatMatrix {
					mat: [[0.9, 0.2, 0.0], [-0.2, 0.9, 0.0], [1.5, 0.5, 1.0]],
				};
				transform_world_sys(
					null,
					0,
					straight,
					frame,
					a,
					&mode(PF_Xfer_IN_FRONT, 230),
					std::ptr::null(),
					&matrix,
					1,
					1,
					std::ptr::null(),
					d,
				)
			}
		}
	};
	assert_eq!(err, PF_Err_NONE, "{op:?}");
	out.pixels()
}

#[test]
fn deep_results_are_the_8_bit_ones_at_a_finer_step() {
	let ops = [
		Op::Composite(PF_Xfer_IN_FRONT),
		Op::Composite(PF_Xfer_MULTIPLY),
		Op::Blend,
		Op::Convolve,
		Op::Copy,
		Op::CopyHq,
		Op::Transfer(PF_Xfer_IN_FRONT),
		Op::Transfer(PF_Xfer_BEHIND),
		Op::Transfer(PF_Xfer_SCREEN),
		Op::Transfer(PF_Xfer_DIFFERENCE),
		Op::MaskedTransfer,
		Op::Transform,
	];
	for op in ops {
		let eight = run::<PF_Pixel8>(op);
		let untouched = Owned::<PF_Pixel8>::new(W, H, picture(3)).pixels();
		assert_ne!(eight, untouched, "{op:?} wrote something");
		for (depth, deep) in [(16, run::<PF_Pixel16>(op)), (32, run::<PF_PixelFloat>(op))] {
			for (at, (a, b)) in eight.iter().zip(&deep).enumerate() {
				let near = a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 1.01);
				assert!(near, "{op:?} at {depth}, pixel {at}: 8-bit {a:?}, deep {b:?}");
			}
		}
	}
}

#[test]
fn float_worlds_keep_what_is_out_of_range() {
	// Out of 255: red 2, green -0.25, blue 4.
	let bright = [255.0, 510.0, -63.75, 1020.0];
	let mut src = Owned::<PF_PixelFloat>::new(4, 4, |_, _| bright);
	let mut dst = Owned::<PF_PixelFloat>::new(4, 4, |_, _| [255.0; 4]);
	let transfer = |src: &mut Owned<PF_PixelFloat>, dst: &mut Owned<PF_PixelFloat>, xfer: i32| {
		// SAFETY: every pointer is a live local or null.
		let err = unsafe {
			transfer_rect_sys(
				std::ptr::null_mut(),
				0,
				PF_MF_Alpha_STRAIGHT as PF_ModeFlags,
				PF_Field_FRAME as PF_Field,
				std::ptr::null(),
				&src.world,
				&mode(xfer, 255),
				std::ptr::null(),
				0,
				0,
				&mut dst.world,
			)
		};
		assert_eq!(err, PF_Err_NONE);
	};
	transfer(&mut src, &mut dst, PF_Xfer_IN_FRONT);
	assert_eq!(dst.get(1, 1), bright);
	transfer(&mut src, &mut dst, PF_Xfer_ADD);
	assert_eq!(dst.get(1, 1), [255.0, 1020.0, -127.5, 2040.0]);
}

#[test]
fn deep_opacity_is_read_from_opacity_su() {
	let mut dst = Owned::<PF_Pixel16>::new(2, 2, |_, _| [255.0, 0.0, 0.0, 0.0]);
	let src = Owned::<PF_Pixel16>::new(2, 2, |_, _| [255.0; 4]);
	let mut half = mode(PF_Xfer_IN_FRONT, 255);
	half.opacitySu = 16384;
	// SAFETY: every pointer is a live local or null.
	let err = unsafe {
		transfer_rect_sys(
			std::ptr::null_mut(),
			0,
			PF_MF_Alpha_STRAIGHT as PF_ModeFlags,
			PF_Field_FRAME as PF_Field,
			std::ptr::null(),
			&src.world,
			&half,
			std::ptr::null(),
			0,
			0,
			&mut dst.world,
		)
	};
	assert_eq!(err, PF_Err_NONE);
	assert_eq!(dst.get(0, 0)[1], 127.5);
}

#[test]
fn a_world_over_part_of_another_has_its_depth() {
	let mut whole = Owned::<PF_PixelFloat>::new(8, 8, picture(5));
	let mut part = whole.world;
	let pixel = std::mem::size_of::<PF_PixelFloat>();
	part.data = (whole.world.data as usize + 2 * whole.world.rowbytes as usize + 3 * pixel) as PF_PixelPtr;
	(part.width, part.height) = (4, 4);
	assert_eq!(depth_of(&part), Ok(Depth::Float));

	let mut out = Owned::<PF_PixelFloat>::new(4, 4, |_, _| [0.0; 4]);
	let null = std::ptr::null_mut();
	// SAFETY: `part` is inside `whole`, which outlives the call.
	let err = unsafe {
		copy_sys(
			null,
			&mut part,
			&mut out.world,
			std::ptr::null_mut(),
			std::ptr::null_mut(),
		)
	};
	assert_eq!(err, PF_Err_NONE);
	assert_eq!(out.get(0, 0), whole.get(3, 2));
	assert_eq!(out.get(3, 3), whole.get(6, 5));

	// Nothing past the end is taken for part of it.
	part.data = (whole.world.data as usize + 8 * whole.world.rowbytes as usize) as PF_PixelPtr;
	assert_ne!(
		super::super::world::allocated_format(&part),
		Some(PF_PixelFormat_ARGB128 as u32)
	);
	whole.world.height = 8;
}

#[test]
fn copy_stays_inside_both_worlds_and_stretches() {
	let rect = |left, top, right, bottom| PF_Rect {
		left,
		top,
		right,
		bottom,
	};
	let null = std::ptr::null_mut();
	let mut src = Owned::<PF_Pixel16>::new(4, 3, picture(6));
	let mut dst = Owned::<PF_Pixel16>::new(8, 6, |_, _| [0.0; 4]);

	// A rectangle wider than the source, landing left of the destination.
	let (mut from, mut to) = (rect(0, 0, 20, 3), rect(-2, 0, 18, 3));
	// SAFETY: every pointer is a live local.
	unsafe {
		assert_eq!(
			copy_sys(null, &mut src.world, &mut dst.world, &mut from, &mut to),
			PF_Err_NONE
		)
	};
	assert_eq!(dst.get(0, 1), src.get(2, 1));
	assert_eq!(dst.get(1, 1), src.get(3, 1));
	assert_eq!(dst.get(2, 1), [0.0; 4], "past the source nothing is written");

	let (mut from, mut to) = (rect(0, 0, 4, 3), rect(0, 0, 8, 6));
	// SAFETY: as above.
	unsafe {
		assert_eq!(
			copy_sys(null, &mut src.world, &mut dst.world, &mut from, &mut to),
			PF_Err_NONE
		)
	};
	assert_eq!(dst.get(7, 5), src.get(3, 2));
	assert_eq!(dst.get(2, 2), src.get(1, 1));
}

#[test]
fn copy_within_one_buffer_takes_the_source_first() {
	let mut whole = Owned::<PF_PixelFloat>::new(6, 5, picture(7));
	let before = whole.pixels();
	let mut view = whole.world;
	let rect = |left, top, right, bottom| PF_Rect {
		left,
		top,
		right,
		bottom,
	};
	let (mut from, mut to) = (rect(0, 0, 5, 4), rect(1, 1, 6, 5));
	// SAFETY: two world structs over one live buffer.
	let err = unsafe { copy_sys(std::ptr::null_mut(), &mut whole.world, &mut view, &mut from, &mut to) };
	assert_eq!(err, PF_Err_NONE);
	for y in 0..4 {
		for x in 0..5 {
			assert_eq!(whole.get(x + 1, y + 1), before[(y * 6 + x) as usize], "({x}, {y})");
		}
	}
}

#[test]
fn fill_and_premultiply_follow_a_deep_worlds_depth() {
	let null = std::ptr::null_mut();
	let mut world = Owned::<PF_PixelFloat>::new(5, 2, |_, _| [127.5, 255.0, 255.0, 255.0]);
	// SAFETY: a live world.
	unsafe { assert_eq!(premultiply_sys(null, 1, &mut world.world), PF_Err_NONE) };
	assert_eq!(world.get(4, 1), [127.5; 4], "to the last pixel");
	// SAFETY: as above, with null for transparent black.
	unsafe {
		assert_eq!(
			fill_8_sys(null, std::ptr::null(), std::ptr::null(), &mut world.world),
			PF_Err_NONE
		)
	};
	assert_eq!(world.pixels(), vec![[0.0; 4]; 10], "every pixel cleared");
	let white = PF_Pixel8 {
		alpha: 255,
		red: 255,
		green: 255,
		blue: 255,
	};
	// SAFETY: as above.
	unsafe {
		assert_eq!(
			fill_8_sys(null, &white, std::ptr::null(), &mut world.world),
			PF_Err_NONE
		)
	};
	assert_eq!(world.pixels(), vec![[255.0; 4]; 10]);
}

/// What the observer was told.
#[derive(Default)]
struct Heard(Mutex<Vec<(String, String)>>);

impl crate::Observer for Heard {
	fn unsupported(&self, event: &crate::UnsupportedEvent) {
		self.0.lock().unwrap().push((event.name.clone(), event.detail.clone()));
	}
}

unsafe extern "C" fn mixes_depths(
	cmd: PF_Cmd,
	input: *mut PF_InData,
	_: *mut PF_OutData,
	_: PF_ParamList,
	_: *mut PF_LayerDef,
	_: *mut c_void,
) -> PF_Err {
	if cmd == PF_Cmd_ABOUT as PF_Cmd {
		let mut deep = Owned::<PF_Pixel16>::new(2, 2, picture(0));
		let mut float = Owned::<PF_PixelFloat>::new(2, 2, picture(0));
		// SAFETY: the host's `in_data`, and two live worlds.
		let err = unsafe {
			let effect = (*input).effect_ref;
			blend_sys(effect, &deep.world, &deep.world, 0, &mut float.world)
		};
		deep.world.height = 2;
		if err != PF_Err_BAD_CALLBACK_PARAM {
			return PF_Err_INTERNAL_STRUCT_DAMAGED;
		}
	}
	PF_Err_NONE
}

#[test]
fn a_refusal_is_reported() {
	// SAFETY: `mixes_depths` has the effect entry point's signature.
	let mut fx = unsafe { crate::Host::get().entry_loader(mixes_depths) }
		.size(8, 8)
		.load()
		.unwrap();
	let heard = Arc::new(Heard::default());
	fx.set_observer(Some(heard.clone()), crate::ObserveLevel::Commands);
	fx.about().expect("the refusal reached the effect");
	let heard = heard.0.lock().unwrap();
	assert_eq!(heard.len(), 1, "{heard:?}");
	assert_eq!(heard[0].0, "PF_WorldTransformSuite1/blend");
	assert!(heard[0].1.contains("two depths"), "{heard:?}");
}
