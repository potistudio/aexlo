//! Unit-level host checks: one function, one suite, one variable at a time.
//!
//! Every check feeds a host service *fixed inputs* and records the exact
//! output as a `fact` event. Facts are deterministic by construction — no
//! dependence on comp size, current time, or how the host schedules commands
//! — so a fact that differs between real After Effects and aexlo is a real
//! behavioral divergence, never scenario noise. The harness diffs facts by
//! default and treats everything else in the trace as context.
//!
//! Worlds needed by pixel-level checks are allocated *through the host*
//! (`utils.new_world`), keeping every check self-contained.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

use after_effects::sys as ae;
use serde_json::{Value, json};

use crate::inspect;
use crate::trace::trace;

pub fn fact(name: &str, value: Value) {
	trace().emit("fact", json!({ "name": name, "value": value }));
}

/// Run one check group with a panic guard, so a single broken host service
/// surfaces as a fact instead of silencing the remaining checks.
fn guarded(name: &str, run: impl FnOnce()) {
	if std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).is_err() {
		fact(&format!("{name}.panicked"), json!(true));
	}
}

pub unsafe fn run_all(in_data: *mut ae::PF_InData) {
	guarded("var", || unsafe { variable_checks(in_data) });
	guarded("ansi", || unsafe { ansi_checks(in_data) });
	guarded("color", || unsafe { color_checks(in_data) });
	guarded("handle", || unsafe { handle_checks(in_data) });
	guarded("world", || unsafe { world_checks(in_data) });
	guarded("kernel", || unsafe { kernel_checks(in_data) });
	guarded("suite.ansi", || unsafe { suite_ansi_checks(in_data) });
	guarded("suite.handle", || unsafe { suite_handle_checks(in_data) });
	guarded("suite.world", || unsafe { suite_world_checks(in_data) });
	guarded("suite.iterate8", || unsafe { suite_iterate8_checks(in_data) });
}

// ---- Variables ------------------------------------------------------------

/// Host-identity fields of `PF_InData` that are static facts of the host
/// build, not of the session.
unsafe fn variable_checks(in_data: *mut ae::PF_InData) {
	let d = unsafe { &*in_data };
	fact("var.appl_id", json!(inspect::fourcc(d.appl_id)));
	fact("var.version", json!(format!("{}.{}", d.version.major, d.version.minor)));
	fact("var.quality", json!(d.quality));
	fact("var.in_flags", json!(d.in_flags));
}

// ---- ANSI callbacks --------------------------------------------------------

// The literal `3.14159265` is deliberate sprintf `%f` test input, not an attempt
// to use PI — don't rewrite it to `std::f64::consts::PI`.
#[allow(clippy::approx_constant)]
unsafe fn ansi_checks(in_data: *mut ae::PF_InData) {
	let Some(u) = (unsafe { (*in_data).utils.as_ref() }) else {
		fact("ansi.available", json!(false));
		return;
	};
	let a = &u.ansi;

	// Fixed-input math: full f64 precision on purpose — a last-bit difference
	// between the host's libm and ours is a genuine finding.
	macro_rules! math1 {
		($fn:ident, $x:expr) => {
			match a.$fn {
				Some(f) => fact(
					concat!("ansi.", stringify!($fn), "(", stringify!($x), ")"),
					json!(unsafe { f($x) }),
				),
				None => fact(concat!("ansi.", stringify!($fn)), json!("unavailable")),
			}
		};
	}
	macro_rules! math2 {
		($fn:ident, $x:expr, $y:expr) => {
			match a.$fn {
				Some(f) => fact(
					concat!(
						"ansi.",
						stringify!($fn),
						"(",
						stringify!($x),
						",",
						stringify!($y),
						")"
					),
					json!(unsafe { f($x, $y) }),
				),
				None => fact(concat!("ansi.", stringify!($fn)), json!("unavailable")),
			}
		};
	}

	math1!(atan, 1.0);
	math2!(atan2, 1.0, 2.0);
	math1!(ceil, 2.3);
	math1!(cos, 1.0);
	math1!(exp, 1.0);
	math1!(fabs, -3.5);
	math1!(floor, 2.7);
	math2!(fmod, 7.5, 2.0);
	math2!(hypot, 3.0, 4.0);
	math1!(log, 2.0);
	math1!(log10, 1000.0);
	math2!(pow, 2.0, 0.5);
	math1!(sin, 0.5);
	math1!(sqrt, 2.0);
	math1!(tan, 0.5);
	math1!(asin, 0.5);
	math1!(acos, 0.5);

	// sprintf formatting matrix: width, precision, alignment, zero-pad —
	// exactly the territory where an sprintf emulation drifts from the CRT.
	if let Some(sprintf) = a.sprintf {
		let case = |name: &str, call: &dyn Fn(*mut i8) -> i32| {
			let mut buffer = [0i8; 128];
			let returned = call(buffer.as_mut_ptr());
			fact(
				&format!("ansi.sprintf.{name}"),
				json!({ "ret": returned, "out": inspect::cstr_field(&buffer) }),
			);
		};

		case("int", &|b| unsafe {
			sprintf(b, c"%d|%5d|%-5d|%05d|%+d".as_ptr(), 42i32, 42i32, 42i32, 42i32, 42i32)
		});
		case("uint", &|b| unsafe {
			sprintf(b, c"%u|%x|%X|%o".as_ptr(), 255u32, 255u32, 255u32, 255u32)
		});
		case("float", &|b| unsafe {
			sprintf(
				b,
				c"%f|%.2f|%10.3f|%-10.1f|".as_ptr(),
				3.14159265f64,
				3.14159265f64,
				3.14159265f64,
				3.14159265f64,
			)
		});
		case("exp", &|b| unsafe {
			sprintf(b, c"%e|%E|%g".as_ptr(), 12345.6789f64, 12345.6789f64, 12345.6789f64)
		});
		case("str", &|b| unsafe {
			sprintf(
				b,
				c"[%s][%8s][%-8s]".as_ptr(),
				c"ae".as_ptr(),
				c"ae".as_ptr(),
				c"ae".as_ptr(),
			)
		});
		case("char", &|b| unsafe { sprintf(b, c"%c%%".as_ptr(), b'A' as i32) });
	} else {
		fact("ansi.sprintf", json!("unavailable"));
	}

	if let Some(strcpy) = a.strcpy {
		let mut buffer = [0i8; 32];
		let returned = unsafe { strcpy(buffer.as_mut_ptr(), c"probe".as_ptr()) };
		fact(
			"ansi.strcpy(\"probe\")",
			json!({ "returns_dst": returned == buffer.as_mut_ptr(), "out": inspect::cstr_field(&buffer) }),
		);
	} else {
		fact("ansi.strcpy", json!("unavailable"));
	}
}

// ---- Color conversion callbacks --------------------------------------------

unsafe fn color_checks(in_data: *mut ae::PF_InData) {
	let Some(u) = (unsafe { (*in_data).utils.as_ref() }) else {
		return;
	};
	let effect_ref = unsafe { (*in_data).effect_ref };
	let fixed = |v: ae::PF_Fixed| v as f64 / 65536.0;

	if let Some(rgb_to_hls) = u.colorCB.RGBtoHLS {
		let mut rgb = ae::PF_Pixel {
			alpha: 255,
			red: 200,
			green: 100,
			blue: 50,
		};
		let mut hls = [0 as ae::PF_Fixed; 3];
		let err = unsafe { rgb_to_hls(effect_ref, &mut rgb, hls.as_mut_ptr()) };
		fact(
			"color.rgb_to_hls(200,100,50)",
			json!({ "err": err, "h": fixed(hls[0]), "l": fixed(hls[1]), "s": fixed(hls[2]) }),
		);
	} else {
		fact("color.rgb_to_hls", json!("unavailable"));
	}

	if let Some(hls_to_rgb) = u.colorCB.HLStoRGB {
		// H=0.25 (of the fixed-point hue range), L=0.5, S=1.0.
		let mut hls = [16384 as ae::PF_Fixed, 32768, 65536];
		let mut rgb = ae::PF_Pixel {
			alpha: 0,
			red: 0,
			green: 0,
			blue: 0,
		};
		let err = unsafe { hls_to_rgb(effect_ref, hls.as_mut_ptr(), &mut rgb) };
		fact(
			"color.hls_to_rgb(0.25,0.5,1.0)",
			json!({ "err": err, "r": rgb.red, "g": rgb.green, "b": rgb.blue, "a": rgb.alpha }),
		);
	} else {
		fact("color.hls_to_rgb", json!("unavailable"));
	}
}

// ---- Host handle allocator --------------------------------------------------

unsafe fn handle_checks(in_data: *mut ae::PF_InData) {
	let Some(u) = (unsafe { (*in_data).utils.as_ref() }) else {
		return;
	};

	let (Some(new_handle), Some(lock), Some(unlock), Some(dispose)) = (
		u.host_new_handle,
		u.host_lock_handle,
		u.host_unlock_handle,
		u.host_dispose_handle,
	) else {
		fact("handle.available", json!(false));
		return;
	};

	let mut handle = unsafe { new_handle(24) };
	fact("handle.new(24).nonnull", json!(!handle.is_null()));
	if handle.is_null() {
		return;
	}

	if let Some(get_size) = u.host_get_handle_size {
		fact("handle.new(24).size", json!(unsafe { get_size(handle) }));
	}

	let ptr = unsafe { lock(handle) };
	let mut roundtrip = false;
	if !ptr.is_null() {
		unsafe {
			std::ptr::write_bytes(ptr as *mut u8, 0xA5, 24);
			roundtrip = (ptr as *const u8).read() == 0xA5 && (ptr as *const u8).add(23).read() == 0xA5;
		}
	}
	unsafe { unlock(handle) };
	fact("handle.lock.write_read_roundtrip", json!(roundtrip));

	if let Some(resize) = u.host_resize_handle {
		let err = unsafe { resize(64, &mut handle) };
		let size = u.host_get_handle_size.map(|get_size| unsafe { get_size(handle) });
		let preserved = {
			let ptr = unsafe { lock(handle) };
			let ok = !ptr.is_null()
				&& unsafe { (ptr as *const u8).read() == 0xA5 && (ptr as *const u8).add(23).read() == 0xA5 };
			unsafe { unlock(handle) };
			ok
		};
		fact(
			"handle.resize(24->64)",
			json!({ "err": err, "size": size, "preserves_contents": preserved }),
		);
	} else {
		fact("handle.resize", json!("unavailable"));
	}

	unsafe { dispose(handle) };
}

// ---- Worlds & pixel operations ----------------------------------------------

const CHECK_W: i32 = 16;
const CHECK_H: i32 = 8;

/// A host-allocated scratch world plus the allocator that must free it.
struct CheckWorld {
	world: ae::PF_EffectWorld,
	via_suite: bool,
}

/// Allocate a cleared 16x8 ARGB32 world through the host: the `utils`
/// callback when the host fills it in, falling back to World Suite 2 so
/// pixel-level checks still run on hosts that only vend the suite.
unsafe fn make_world(in_data: *mut ae::PF_InData) -> Option<CheckWorld> {
	let mut world: ae::PF_EffectWorld = unsafe { std::mem::zeroed() };

	if let Some(u) = unsafe { (*in_data).utils.as_ref() }
		&& let Some(new_world) = u.new_world
	{
		let err = unsafe {
			new_world(
				(*in_data).effect_ref,
				CHECK_W,
				CHECK_H,
				ae::PF_NewWorldFlag_CLEAR_PIXELS as ae::PF_NewWorldFlags,
				&mut world,
			)
		};
		if err == 0 && !world.data.is_null() {
			return Some(CheckWorld {
				world,
				via_suite: false,
			});
		}
		// A vended-but-failing callback falls through to the suite path.
		world = unsafe { std::mem::zeroed() };
	}

	let guard = unsafe { acquire(in_data, ae::kPFWorldSuite, ae::kPFWorldSuiteVersion2) }?;
	let suite = unsafe { &*(guard.ptr as *const ae::PF_WorldSuite2) };
	let new_world = suite.PF_NewWorld?;
	let err = unsafe {
		new_world(
			(*in_data).effect_ref,
			CHECK_W,
			CHECK_H,
			1,
			ae::PF_PixelFormat_ARGB32 as ae::PF_PixelFormat,
			&mut world,
		)
	};
	(err == 0 && !world.data.is_null()).then_some(CheckWorld { world, via_suite: true })
}

unsafe fn dispose_world(in_data: *mut ae::PF_InData, cw: &mut CheckWorld) {
	if !cw.via_suite {
		if let Some(u) = unsafe { (*in_data).utils.as_ref() }
			&& let Some(dispose) = u.dispose_world
		{
			unsafe { dispose((*in_data).effect_ref, &mut cw.world) };
		}
		return;
	}

	if let Some(guard) = unsafe { acquire(in_data, ae::kPFWorldSuite, ae::kPFWorldSuiteVersion2) }
		&& let Some(dispose) = unsafe { &*(guard.ptr as *const ae::PF_WorldSuite2) }.PF_DisposeWorld
	{
		unsafe { dispose((*in_data).effect_ref, &mut cw.world) };
	}
}

unsafe fn sample(world: &ae::PF_EffectWorld, x: i32, y: i32) -> ae::PF_Pixel {
	unsafe {
		let row = (world.data as *const u8).add(y as usize * world.rowbytes as usize);
		*(row as *const ae::PF_Pixel).add(x as usize)
	}
}

fn pixel_json(p: ae::PF_Pixel) -> Value {
	json!({ "a": p.alpha, "r": p.red, "g": p.green, "b": p.blue })
}

fn world_hash(world: &ae::PF_EffectWorld) -> Value {
	match unsafe { inspect::world_pixels_fnv1a(world) } {
		Some(hash) => json!(format!("{hash:016x}")),
		None => json!(null),
	}
}

unsafe fn world_checks(in_data: *mut ae::PF_InData) {
	let Some(u) = (unsafe { (*in_data).utils.as_ref() }) else {
		return;
	};
	let effect_ref = unsafe { (*in_data).effect_ref };

	let Some(mut world) = (unsafe { make_world(in_data) }) else {
		fact("world.new_world(16x8)", json!("unavailable"));
		return;
	};

	// Layout policy: does the host pad rowbytes, and how?
	fact(
		"world.new_world(16x8)",
		json!({
			"via": if world.via_suite { "world_suite_v2" } else { "utils.new_world" },
			"width": world.world.width,
			"height": world.world.height,
			"rowbytes": world.world.rowbytes,
			"world_flags": world.world.world_flags,
			"cleared_to_zero": world_hash(&world.world) == json!(format!("{:016x}", zero_hash())),
		}),
	);

	if let Some(fill) = u.fill {
		// Full fill (null rect).
		let color = ae::PF_Pixel {
			alpha: 255,
			red: 10,
			green: 20,
			blue: 30,
		};
		let err = unsafe { fill(effect_ref, &color, std::ptr::null(), &mut world.world) };
		fact(
			"world.fill.full(10,20,30)",
			json!({ "err": err, "hash": world_hash(&world.world), "px(0,0)": pixel_json(unsafe { sample(&world.world, 0, 0) }) }),
		);

		// Rect fill: PF_Rect right/bottom are exclusive — is the host faithful?
		let color = ae::PF_Pixel {
			alpha: 200,
			red: 99,
			green: 88,
			blue: 77,
		};
		let rect = ae::PF_LRect {
			left: 2,
			top: 1,
			right: 6,
			bottom: 5,
		};
		let err = unsafe { fill(effect_ref, &color, &rect, &mut world.world) };
		fact(
			"world.fill.rect(2,1,6,5)",
			json!({
				"err": err,
				"inside(2,1)": pixel_json(unsafe { sample(&world.world, 2, 1) }),
				"inside(5,4)": pixel_json(unsafe { sample(&world.world, 5, 4) }),
				"outside(6,5)": pixel_json(unsafe { sample(&world.world, 6, 5) }),
				"outside(0,0)": pixel_json(unsafe { sample(&world.world, 0, 0) }),
			}),
		);
	} else {
		fact("world.fill", json!("unavailable"));
	}

	if let Some(copy) = u.copy {
		if let Some(mut dst) = unsafe { make_world(in_data) } {
			let err = unsafe {
				copy(
					effect_ref,
					&mut world.world,
					&mut dst.world,
					std::ptr::null_mut(),
					std::ptr::null_mut(),
				)
			};
			fact(
				"world.copy.full",
				json!({ "err": err, "dst_equals_src": world_hash(&dst.world) == world_hash(&world.world) }),
			);
			unsafe { dispose_world(in_data, &mut dst) };
		}
	} else {
		fact("world.copy", json!("unavailable"));
	}

	if let (Some(fill), Some(blend)) = (u.fill, u.blend) {
		let (a, b, d) = unsafe { (make_world(in_data), make_world(in_data), make_world(in_data)) };
		if let (Some(mut a), Some(mut b), Some(mut d)) = (a, b, d) {
			let gray100 = ae::PF_Pixel {
				alpha: 255,
				red: 100,
				green: 100,
				blue: 100,
			};
			let gray201 = ae::PF_Pixel {
				alpha: 255,
				red: 201,
				green: 201,
				blue: 201,
			};
			unsafe {
				fill(effect_ref, &gray100, std::ptr::null(), &mut a.world);
				fill(effect_ref, &gray201, std::ptr::null(), &mut b.world);
			}
			// ratio 0.5 in fixed point; 100..201 makes the rounding direction visible.
			let err = unsafe { blend(effect_ref, &a.world, &b.world, 32768, &mut d.world) };
			fact(
				"world.blend(100,201,ratio=0.5)",
				json!({ "err": err, "px(0,0)": pixel_json(unsafe { sample(&d.world, 0, 0) }) }),
			);
			unsafe {
				dispose_world(in_data, &mut a);
				dispose_world(in_data, &mut b);
				dispose_world(in_data, &mut d);
			}
		}
	} else {
		fact("world.blend", json!("unavailable"));
	}

	if let (Some(fill), Some(premultiply)) = (u.fill, u.premultiply) {
		if let Some(mut w) = unsafe { make_world(in_data) } {
			let color = ae::PF_Pixel {
				alpha: 128,
				red: 255,
				green: 64,
				blue: 1,
			};
			unsafe { fill(effect_ref, &color, std::ptr::null(), &mut w.world) };
			// Rounding policy of alpha-multiply: 255*128/255, 64*128/255, 1*128/255.
			let err = unsafe { premultiply(effect_ref, 1, &mut w.world) };
			fact(
				"world.premultiply(a=128,rgb=255,64,1)",
				json!({ "err": err, "px(0,0)": pixel_json(unsafe { sample(&w.world, 0, 0) }) }),
			);
			unsafe { dispose_world(in_data, &mut w) };
		}
	} else {
		fact("world.premultiply", json!("unavailable"));
	}

	if let Some(iterate) = u.iterate {
		if let Some(mut dst) = unsafe { make_world(in_data) } {
			let count = AtomicU64::new(0);
			let err = unsafe {
				iterate(
					in_data,
					0,
					1,
					&mut world.world,
					std::ptr::null(),
					&count as *const AtomicU64 as *mut c_void,
					Some(count_and_increment),
					&mut dst.world,
				)
			};
			fact(
				"world.iterate(16x8)",
				json!({
					"err": err,
					"invocations": count.load(Ordering::Relaxed),
					"dst_hash": world_hash(&dst.world),
				}),
			);
			unsafe { dispose_world(in_data, &mut dst) };
		}
	} else {
		fact("world.iterate", json!("unavailable"));
	}

	unsafe { dispose_world(in_data, &mut world) };
}

/// FNV-1a of an all-zero 16x8 ARGB world (the expected hash after
/// `PF_NewWorldFlag_CLEAR_PIXELS`), independent of rowbytes padding.
fn zero_hash() -> u64 {
	let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
	for _ in 0..(CHECK_W * CHECK_H * 4) {
		hash ^= 0;
		hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
	}
	hash
}

/// Iterate callback: count invocations, bump every channel by one. Kept
/// trivially panic-free (panics may not unwind across `extern "C"`).
unsafe extern "C" fn count_and_increment(
	refcon: *mut c_void,
	_x: ae::A_long,
	_y: ae::A_long,
	in_pixel: *mut ae::PF_Pixel,
	out_pixel: *mut ae::PF_Pixel,
) -> ae::PF_Err {
	if !refcon.is_null() {
		unsafe { &*(refcon as *const AtomicU64) }.fetch_add(1, Ordering::Relaxed);
	}
	if !in_pixel.is_null() && !out_pixel.is_null() {
		let p = unsafe { *in_pixel };
		unsafe {
			*out_pixel = ae::PF_Pixel {
				alpha: p.alpha.wrapping_add(1),
				red: p.red.wrapping_add(1),
				green: p.green.wrapping_add(1),
				blue: p.blue.wrapping_add(1),
			};
		}
	}
	0
}

// ---- Convolution kernel -------------------------------------------------------

unsafe fn kernel_checks(in_data: *mut ae::PF_InData) {
	let Some(u) = (unsafe { (*in_data).utils.as_ref() }) else {
		return;
	};
	let Some(gaussian) = u.gaussian_kernel else {
		fact("kernel.gaussian", json!("unavailable"));
		return;
	};

	let flags =
		(ae::PF_KernelFlag_2D | ae::PF_KernelFlag_NORMALIZED | ae::PF_KernelFlag_USE_LONG) as ae::PF_KernelFlags;
	let mut diameter: ae::A_long = 0;
	let mut kernel = [0 as ae::A_long; 1024];
	let err = unsafe {
		gaussian(
			(*in_data).effect_ref,
			2.0,
			flags,
			1.0,
			&mut diameter,
			kernel.as_mut_ptr() as *mut c_void,
		)
	};

	let cells = (diameter * diameter).clamp(0, 1024) as usize;
	let sum: i64 = kernel[..cells].iter().map(|&v| v as i64).sum();
	fact(
		"kernel.gaussian(r=2,normalized,long)",
		json!({
			"err": err,
			"diameter": diameter,
			"first_row": &kernel[..(diameter.clamp(0, 8) as usize)],
			"sum": sum,
		}),
	);
}

// ---- Suite-level checks ----------------------------------------------------
// The suite map (which suites acquire at all) lives in probes.rs; these go one
// level deeper and pin down the behavior of individual suite functions.

struct SuiteGuard<'a> {
	pica: &'a ae::SPBasicSuite,
	name: &'static [u8],
	version: u32,
	ptr: *const c_void,
}

impl Drop for SuiteGuard<'_> {
	fn drop(&mut self) {
		if let Some(release) = self.pica.ReleaseSuite {
			unsafe { release(self.name.as_ptr() as *const _, self.version as i32) };
		}
	}
}

unsafe fn acquire(in_data: *mut ae::PF_InData, name: &'static [u8], version: u32) -> Option<SuiteGuard<'static>> {
	let pica = unsafe { (*in_data).pica_basicP.as_ref() }?;
	let acquire = pica.AcquireSuite?;
	let mut ptr: *const c_void = std::ptr::null();
	let err = unsafe { acquire(name.as_ptr() as *const _, version as i32, &mut ptr) };
	(err == 0 && !ptr.is_null()).then_some(SuiteGuard {
		pica,
		name,
		version,
		ptr,
	})
}

unsafe fn suite_ansi_checks(in_data: *mut ae::PF_InData) {
	let Some(guard) = (unsafe { acquire(in_data, ae::kPFANSISuite, ae::kPFANSISuiteVersion1) }) else {
		fact("suite.ansi.acquire", json!("unavailable"));
		return;
	};
	let suite = unsafe { &*(guard.ptr as *const ae::PF_ANSICallbacksSuite1) };

	if let Some(sin) = suite.sin {
		fact("suite.ansi.sin(0.5)", json!(unsafe { sin(0.5) }));
	}
	if let Some(pow) = suite.pow {
		fact("suite.ansi.pow(2,10)", json!(unsafe { pow(2.0, 10.0) }));
	}
	if let Some(sprintf) = suite.sprintf {
		let mut buffer = [0i8; 64];
		let returned = unsafe { sprintf(buffer.as_mut_ptr(), c"%d/%s/%.2f".as_ptr(), 7i32, c"x".as_ptr(), 1.5f64) };
		fact(
			"suite.ansi.sprintf",
			json!({ "ret": returned, "out": inspect::cstr_field(&buffer) }),
		);
	}
}

unsafe fn suite_handle_checks(in_data: *mut ae::PF_InData) {
	let Some(guard) = (unsafe { acquire(in_data, ae::kPFHandleSuite, ae::kPFHandleSuiteVersion1) }) else {
		fact("suite.handle.acquire", json!("unavailable"));
		return;
	};
	let suite = unsafe { &*(guard.ptr as *const ae::PF_HandleSuite1) };

	let (Some(new_handle), Some(dispose)) = (suite.host_new_handle, suite.host_dispose_handle) else {
		return;
	};
	let mut handle = unsafe { new_handle(32) };
	let size = suite.host_get_handle_size.map(|get_size| unsafe { get_size(handle) });
	let resize_err = suite
		.host_resize_handle
		.map(|resize| unsafe { resize(48, &mut handle) });
	let resized = suite.host_get_handle_size.map(|get_size| unsafe { get_size(handle) });
	unsafe { dispose(handle) };
	fact(
		"suite.handle.new(32).resize(48)",
		json!({ "size": size, "resize_err": resize_err, "resized_size": resized }),
	);
}

unsafe fn suite_world_checks(in_data: *mut ae::PF_InData) {
	let Some(guard) = (unsafe { acquire(in_data, ae::kPFWorldSuite, ae::kPFWorldSuiteVersion2) }) else {
		fact("suite.world.acquire", json!("unavailable"));
		return;
	};
	let suite = unsafe { &*(guard.ptr as *const ae::PF_WorldSuite2) };

	let (Some(new_world), Some(dispose), get_format) =
		(suite.PF_NewWorld, suite.PF_DisposeWorld, suite.PF_GetPixelFormat)
	else {
		return;
	};

	// Which pixel formats can the host actually allocate, and what layout
	// (rowbytes) does each get?
	for (name, format) in [
		("ARGB32", ae::PF_PixelFormat_ARGB32),
		("ARGB64", ae::PF_PixelFormat_ARGB64),
		("ARGB128", ae::PF_PixelFormat_ARGB128),
	] {
		let mut world: ae::PF_EffectWorld = unsafe { std::mem::zeroed() };
		let err = unsafe { new_world((*in_data).effect_ref, 8, 4, 1, format as ae::PF_PixelFormat, &mut world) };

		if err == 0 {
			let mut reported: ae::PF_PixelFormat = 0;
			let format_err = get_format.map(|f| unsafe { f(&world, &mut reported) });
			fact(
				&format!("suite.world.new_world[{name}]"),
				json!({
					"err": err,
					"rowbytes": world.rowbytes,
					"world_flags": world.world_flags,
					"get_pixel_format": { "err": format_err, "format": inspect::fourcc(reported) },
				}),
			);
			unsafe { dispose((*in_data).effect_ref, &mut world) };
		} else {
			fact(&format!("suite.world.new_world[{name}]"), json!({ "err": err }));
		}
	}
}

unsafe fn suite_iterate8_checks(in_data: *mut ae::PF_InData) {
	let Some(guard) = (unsafe { acquire(in_data, ae::kPFIterate8Suite, ae::kPFIterate8SuiteVersion2) }) else {
		fact("suite.iterate8.acquire", json!("unavailable"));
		return;
	};
	let suite = unsafe { &*(guard.ptr as *const ae::PF_Iterate8Suite2) };
	let Some(iterate) = suite.iterate else {
		return;
	};

	let (src, dst) = unsafe { (make_world(in_data), make_world(in_data)) };
	let (Some(mut src), Some(mut dst)) = (src, dst) else {
		fact("suite.iterate8.iterate", json!("worlds unavailable"));
		return;
	};

	let count = AtomicU64::new(0);
	let err = unsafe {
		iterate(
			in_data,
			0,
			1,
			&mut src.world,
			std::ptr::null(),
			&count as *const AtomicU64 as *mut c_void,
			Some(count_and_increment),
			&mut dst.world,
		)
	};
	fact(
		"suite.iterate8.iterate(16x8)",
		json!({
			"err": err,
			"invocations": count.load(Ordering::Relaxed),
			"dst_hash": world_hash(&dst.world),
		}),
	);

	unsafe {
		dispose_world(in_data, &mut src);
		dispose_world(in_data, &mut dst);
	}
}
