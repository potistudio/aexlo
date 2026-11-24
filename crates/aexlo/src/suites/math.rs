//! `AEGP Math Suite` v1: 4x4 matrix helpers.
//!
//! Matrices act on row vectors, as everywhere in the AEGP API: the translation
//! is the last row, and `A * B` applies `A` first. The SDK says neither the
//! units nor the order of `AEGP_MatrixDecompose4`'s angles; aexlo gives
//! radians about x, then y, then z, and the shears xy, xz and yz (the
//! decomposition of Graphics Gems II). That part is untested against After
//! Effects.

use crate::core::diagnostics::diag;
use crate::sys::*;

type M4 = [[f64; 4]; 4];
type V3 = [f64; 3];

const IDENTITY: M4 = [
	[1.0, 0.0, 0.0, 0.0],
	[0.0, 1.0, 0.0, 0.0],
	[0.0, 0.0, 1.0, 0.0],
	[0.0, 0.0, 0.0, 1.0],
];

fn multiply(a: &M4, b: &M4) -> M4 {
	std::array::from_fn(|r| std::array::from_fn(|c| (0..4).map(|k| a[r][k] * b[k][c]).sum()))
}

/// A 2D transform as a 3D one that leaves z alone.
fn widen(m: &[[f64; 3]; 3]) -> M4 {
	[
		[m[0][0], m[0][1], 0.0, m[0][2]],
		[m[1][0], m[1][1], 0.0, m[1][2]],
		[0.0, 0.0, 1.0, 0.0],
		[m[2][0], m[2][1], 0.0, m[2][2]],
	]
}

fn dot(a: V3, b: V3) -> f64 {
	a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
	[
		a[1] * b[2] - a[2] * b[1],
		a[2] * b[0] - a[0] * b[2],
		a[0] * b[1] - a[1] * b[0],
	]
}

fn unit(v: V3) -> Option<(V3, f64)> {
	let length = dot(v, v).sqrt();
	(length.is_finite() && length > 1e-12).then(|| (v.map(|x| x / length), length))
}

fn without(v: V3, along: V3, amount: f64) -> V3 {
	std::array::from_fn(|i| v[i] - amount * along[i])
}

struct Parts {
	position: V3,
	scale: V3,
	shear: V3,
	rotation: V3,
}

/// `None` for a matrix that flattens space or has no finite position.
fn decompose(m: &M4) -> Option<Parts> {
	let w = m[3][3];
	if !w.is_finite() || w == 0.0 {
		return None;
	}
	let row = |r: usize| -> V3 { std::array::from_fn(|c| m[r][c] / w) };

	let (mut x, mut sx) = unit(row(0))?;
	let mut xy = dot(x, row(1));
	let (mut y, mut sy) = unit(without(row(1), x, xy))?;
	xy /= sy;
	let mut xz = dot(x, row(2));
	let partial = without(row(2), x, xz);
	let mut yz = dot(y, partial);
	let (mut z, mut sz) = unit(without(partial, y, yz))?;
	xz /= sz;
	yz /= sz;
	// A mirror is a negative scale, not a rotation.
	if dot(x, cross(y, z)) < 0.0 {
		(sx, sy, sz) = (-sx, -sy, -sz);
		(x, y, z) = (x.map(|v| -v), y.map(|v| -v), z.map(|v| -v));
	}

	let about_y = (-x[2]).clamp(-1.0, 1.0).asin();
	let (about_x, about_z) = if about_y.cos().abs() > 1e-9 {
		(y[2].atan2(z[2]), x[1].atan2(x[0]))
	} else {
		((-z[1]).atan2(y[1]), 0.0)
	};
	Some(Parts {
		position: row(3),
		scale: [sx, sy, sz],
		shear: [xy, xz, yz],
		rotation: [about_x, about_y, about_z],
	})
}

unsafe extern "C" fn identity_matrix4(matrixP: *mut A_Matrix4) -> A_Err {
	crate::core::ffi::guard("identity_matrix4", || {
		diag!("AEGP_MathSuite1/AEGP_IdentityMatrix4");
		let Some(out) = (unsafe { matrixP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		out.mat = IDENTITY;
		PF_Err_NONE
	})
}

unsafe extern "C" fn multiply_matrix4(a: *const A_Matrix4, b: *const A_Matrix4, resultP: *mut A_Matrix4) -> A_Err {
	crate::core::ffi::guard("multiply_matrix4", || {
		diag!("AEGP_MathSuite1/AEGP_MultiplyMatrix4");
		let (Some(a), Some(b)) = (unsafe { a.as_ref() }, unsafe { b.as_ref() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		// The result may be one of the operands: finish reading them first.
		let product = multiply(&a.mat, &b.mat);
		let Some(out) = (unsafe { resultP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		out.mat = product;
		PF_Err_NONE
	})
}

unsafe extern "C" fn matrix3_to_matrix4(a: *const A_Matrix3, b: *mut A_Matrix4) -> A_Err {
	crate::core::ffi::guard("matrix3_to_matrix4", || {
		diag!("AEGP_MathSuite1/AEGP_Matrix3ToMatrix4");
		let (Some(a), Some(out)) = (unsafe { a.as_ref() }, unsafe { b.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		out.mat = widen(&a.mat);
		PF_Err_NONE
	})
}

unsafe extern "C" fn multiply_matrix4_by_3(a: *const A_Matrix4, b: *const A_Matrix3, resultP: *mut A_Matrix4) -> A_Err {
	crate::core::ffi::guard("multiply_matrix4_by_3", || {
		diag!("AEGP_MathSuite1/AEGP_MultiplyMatrix4by3");
		let (Some(a), Some(b)) = (unsafe { a.as_ref() }, unsafe { b.as_ref() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let product = multiply(&a.mat, &widen(&b.mat));
		let Some(out) = (unsafe { resultP.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		out.mat = product;
		PF_Err_NONE
	})
}

unsafe extern "C" fn matrix_decompose4(
	a: *const A_Matrix4,
	posVP: *mut A_FloatPoint3,
	scaleVP: *mut A_FloatPoint3,
	shearVP: *mut A_FloatPoint3,
	rotVP: *mut A_FloatPoint3,
) -> A_Err {
	crate::core::ffi::guard("matrix_decompose4", || {
		diag!("AEGP_MathSuite1/AEGP_MatrixDecompose4");
		let Some(parts) = (unsafe { a.as_ref() }).and_then(|a| decompose(&a.mat)) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		for (out, [x, y, z]) in [
			(posVP, parts.position),
			(scaleVP, parts.scale),
			(shearVP, parts.shear),
			(rotVP, parts.rotation),
		] {
			if let Some(out) = unsafe { out.as_mut() } {
				*out = A_FloatPoint3 { x, y, z };
			}
		}
		PF_Err_NONE
	})
}

pub(super) const fn suite_1() -> AEGP_MathSuite1 {
	AEGP_MathSuite1 {
		AEGP_IdentityMatrix4: Some(identity_matrix4),
		AEGP_MultiplyMatrix4: Some(multiply_matrix4),
		AEGP_Matrix3ToMatrix4: Some(matrix3_to_matrix4),
		AEGP_MultiplyMatrix4by3: Some(multiply_matrix4_by_3),
		AEGP_MatrixDecompose4: Some(matrix_decompose4),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn close(a: V3, b: V3) -> bool {
		a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-9)
	}

	#[test]
	fn the_left_matrix_is_applied_first() {
		let scale = widen(&[[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 1.0]]);
		let shift = widen(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [10.0, 20.0, 1.0]]);
		assert_eq!(multiply(&scale, &shift)[3][..2], [10.0, 20.0]);
		assert_eq!(multiply(&shift, &scale)[3][..2], [20.0, 60.0]);
		assert_eq!(multiply(&scale, &IDENTITY), scale);
	}

	#[test]
	fn the_result_may_be_an_operand() {
		let mut a = A_Matrix4 {
			mat: widen(&[[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [5.0, 7.0, 1.0]]),
		};
		let p = &raw mut a;
		assert_eq!(unsafe { multiply_matrix4(p, p, p) }, PF_Err_NONE);
		assert_eq!(a.mat[3][..2], [15.0, 28.0]);
	}

	#[test]
	fn a_transform_comes_apart_into_what_made_it() {
		let (sin, cos) = 0.5f64.sin_cos();
		let m = widen(&[
			[2.0 * cos, 2.0 * sin, 0.0],
			[-3.0 * sin, 3.0 * cos, 0.0],
			[40.0, 50.0, 1.0],
		]);
		let parts = decompose(&m).unwrap();
		assert!(close(parts.position, [40.0, 50.0, 0.0]));
		assert!(close(parts.scale, [2.0, 3.0, 1.0]));
		assert!(close(parts.shear, [0.0; 3]));
		assert!(close(parts.rotation, [0.0, 0.0, 0.5]));
		assert!(decompose(&[[0.0; 4]; 4]).is_none());
	}
}
