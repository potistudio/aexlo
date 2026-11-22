//! Values that change over time: the one evaluator behind every API that
//! reads a value at a time (parameter checkouts, streams, keyframes, shapes).
//!
//! Times are ratios compared and converted without floating point, so a value
//! asked for at `10/30` and at `1000/3000` seconds is the same value.

use std::cmp::Ordering;

use crate::sys::{A_Time, A_long, A_u_long};
use crate::{MaskPath, MaskVertex, ParamValue, TextLayer, Time};

/// An instant: `value / scale` seconds, like the SDK's `A_Time`.
///
/// Unlike [`Time`], it has no frame duration, and two ways of writing the
/// same instant are equal.
///
/// ```
/// use aexlo::TimePoint;
///
/// assert_eq!(TimePoint::new(1, 2), TimePoint::new(15, 30));
/// assert!(TimePoint::new(-1, 30).unwrap() < TimePoint::ZERO);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TimePoint {
	value: i32,
	scale: u32,
}

impl TimePoint {
	/// Zero seconds.
	pub const ZERO: Self = Self { value: 0, scale: 1 };

	/// `value / scale` seconds, or `None` for a zero `scale`.
	#[must_use]
	pub const fn new(value: i32, scale: u32) -> Option<Self> {
		if scale > 0 { Some(Self { value, scale }) } else { None }
	}

	/// Frame `frame` at a whole `fps` frames per second.
	///
	/// # Panics
	/// If `fps` is zero.
	#[must_use]
	pub const fn frame(frame: i32, fps: u32) -> Self {
		assert!(fps > 0, "frame rate must be positive");
		Self {
			value: frame,
			scale: fps,
		}
	}

	/// A whole number of seconds.
	#[must_use]
	pub const fn whole_seconds(seconds: i32) -> Self {
		Self {
			value: seconds,
			scale: 1,
		}
	}

	/// The time, in units of `1 / scale` seconds.
	#[must_use]
	pub const fn value(self) -> i32 {
		self.value
	}

	/// Units per second. Always positive.
	#[must_use]
	pub const fn scale(self) -> u32 {
		self.scale
	}

	/// This time in seconds.
	#[must_use]
	pub fn seconds(self) -> f64 {
		f64::from(self.value) / f64::from(self.scale)
	}

	pub(crate) fn from_sys(time: A_Time) -> Option<Self> {
		Self::new(time.value, time.scale)
	}

	pub(crate) fn to_sys(self) -> A_Time {
		A_Time {
			value: self.value as A_long,
			scale: self.scale as A_u_long,
		}
	}

	/// `num / den` seconds. Written in units of `1 / scale` when that is exact,
	/// as After Effects hands times back in the comp's scale, else in lowest
	/// terms, and rounded to `scale` only when those do not fit an `A_Time`.
	pub(crate) fn from_ratio(num: i128, den: i128, scale: u32) -> Self {
		let (num, den) = if den < 0 { (-num, -den) } else { (num, den.max(1)) };
		let scale = scale.max(1);
		let scaled = num * i128::from(scale);
		if scaled % den == 0
			&& let Ok(value) = i32::try_from(scaled / den)
		{
			return Self { value, scale };
		}
		let common = gcd(num, den);
		if let (Ok(value), Ok(scale)) = (i32::try_from(num / common), u32::try_from(den / common)) {
			return Self { value, scale };
		}
		let rounded = (scaled + den / 2).div_euclid(den);
		Self {
			value: rounded.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32,
			scale,
		}
	}

	/// `self - earlier`, in `self`'s scale where that is exact.
	pub(crate) fn since(self, earlier: Self) -> Self {
		let (a, b) = (i128::from(self.scale), i128::from(earlier.scale));
		Self::from_ratio(
			i128::from(self.value) * b - i128::from(earlier.value) * a,
			a * b,
			self.scale,
		)
	}

	/// How far `self` is from `from` towards `to`, in `0..=1`.
	fn fraction(self, from: Self, to: Self) -> f64 {
		let (s, a, b) = (i128::from(self.scale), i128::from(from.scale), i128::from(to.scale));
		let done = (i128::from(self.value) * a - i128::from(from.value) * s) * b;
		let whole = (i128::from(to.value) * a - i128::from(from.value) * b) * s;
		if whole == 0 {
			0.0
		} else {
			(done as f64 / whole as f64).clamp(0.0, 1.0)
		}
	}
}

fn gcd(a: i128, b: i128) -> i128 {
	let (mut a, mut b) = (a.abs(), b.abs());
	while b != 0 {
		(a, b) = (b, a % b);
	}
	a.max(1)
}

impl PartialEq for TimePoint {
	fn eq(&self, other: &Self) -> bool {
		self.cmp(other) == Ordering::Equal
	}
}

impl Eq for TimePoint {}

impl PartialOrd for TimePoint {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for TimePoint {
	fn cmp(&self, other: &Self) -> Ordering {
		// `i32 * u32` always fits in an `i64`.
		(i64::from(self.value) * i64::from(other.scale)).cmp(&(i64::from(other.value) * i64::from(self.scale)))
	}
}

impl From<Time> for TimePoint {
	fn from(time: Time) -> Self {
		Self {
			value: time.value(),
			scale: time.scale(),
		}
	}
}

/// How a keyframe leads to the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Interpolation {
	/// In a straight line.
	#[default]
	Linear,
	/// This keyframe's value until the next one.
	Hold,
}

/// A value at a time.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyframe<T> {
	/// When, in the time of the layer the value is on.
	pub time: TimePoint,
	/// The value then.
	pub value: T,
	/// How it leads to the next keyframe.
	pub interpolation: Interpolation,
}

impl<T> Keyframe<T> {
	/// A linear keyframe.
	pub fn linear(time: impl Into<TimePoint>, value: T) -> Self {
		Self {
			time: time.into(),
			value,
			interpolation: Interpolation::Linear,
		}
	}

	/// A keyframe that holds until the next one.
	pub fn hold(time: impl Into<TimePoint>, value: T) -> Self {
		Self {
			time: time.into(),
			value,
			interpolation: Interpolation::Hold,
		}
	}
}

/// A value [`Track`] can evaluate between two keyframes.
pub trait Animate: Clone {
	/// The value `amount` (`0..1`) of the way from `self` to `next`. A value
	/// with nothing in between keeps `self`, which is what this does unless
	/// implemented.
	fn blend(&self, next: &Self, amount: f64) -> Self {
		let _ = (next, amount);
		self.clone()
	}
}

/// A value over time: either constant, or keyframed.
///
/// Before the first keyframe it is the first one's value, after the last the
/// last one's.
///
/// ```
/// use aexlo::{Keyframe, TimePoint, Track};
///
/// let fade = Track::keyed(vec![
///     Keyframe::linear(TimePoint::frame(0, 30), 0.0),
///     Keyframe::linear(TimePoint::frame(30, 30), 100.0),
/// ])
/// .unwrap();
/// assert_eq!(fade.at(TimePoint::frame(15, 30)), 50.0);
/// assert_eq!(Track::new(7.0).at(TimePoint::ZERO), 7.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Track<T>(Values<T>);

#[derive(Debug, Clone, PartialEq)]
enum Values<T> {
	Constant(T),
	/// Never empty, strictly ascending in time.
	Keyed(Vec<Keyframe<T>>),
}

impl<T> Track<T> {
	/// The same value at every time.
	pub fn new(value: T) -> Self {
		Self(Values::Constant(value))
	}

	/// A keyframed value, whatever order `keys` are in. `None` without keys,
	/// or with two at the same instant.
	#[must_use]
	pub fn keyed(mut keys: Vec<Keyframe<T>>) -> Option<Self> {
		keys.sort_by_key(|key| key.time);
		let usable = !keys.is_empty() && keys.windows(2).all(|pair| pair[0].time != pair[1].time);
		usable.then_some(Self(Values::Keyed(keys)))
	}

	/// The keyframes, in time order. None for a constant.
	pub fn keys(&self) -> &[Keyframe<T>] {
		match &self.0 {
			Values::Constant(_) => &[],
			Values::Keyed(keys) => keys,
		}
	}

	/// Every value the track is made of.
	pub(crate) fn values(&self) -> impl Iterator<Item = &T> {
		let constant = match &self.0 {
			Values::Constant(value) => Some(value),
			Values::Keyed(_) => None,
		};
		constant.into_iter().chain(self.keys().iter().map(|key| &key.value))
	}
}

impl<T: Animate> Track<T> {
	/// The value at `time`.
	pub fn at(&self, time: impl Into<TimePoint>) -> T {
		let time = time.into();
		let keys = match &self.0 {
			Values::Constant(value) => return value.clone(),
			Values::Keyed(keys) => keys,
		};
		let after = keys.partition_point(|key| key.time <= time);
		if after == 0 {
			return keys[0].value.clone();
		}
		let from = &keys[after - 1];
		match keys.get(after) {
			Some(to) if from.interpolation == Interpolation::Linear && time != from.time => {
				from.value.blend(&to.value, time.fraction(from.time, to.time))
			}
			_ => from.value.clone(),
		}
	}
}

impl<T> From<T> for Track<T> {
	fn from(value: T) -> Self {
		Self::new(value)
	}
}

fn lerp(a: f64, b: f64, amount: f64) -> f64 {
	a + (b - a) * amount
}

impl Animate for f64 {
	fn blend(&self, next: &Self, amount: f64) -> Self {
		lerp(*self, *next, amount)
	}
}

impl<const N: usize> Animate for [f64; N] {
	fn blend(&self, next: &Self, amount: f64) -> Self {
		std::array::from_fn(|i| lerp(self[i], next[i], amount))
	}
}

/// Paths with as many vertices morph vertex by vertex; others hold.
impl Animate for MaskPath {
	fn blend(&self, next: &Self, amount: f64) -> Self {
		if self.vertices.len() != next.vertices.len() || self.closed != next.closed {
			return self.clone();
		}
		let mix = |a, b| lerp(a, b, amount);
		Self {
			vertices: self
				.vertices
				.iter()
				.zip(&next.vertices)
				.map(|(a, b)| MaskVertex {
					x: mix(a.x, b.x),
					y: mix(a.y, b.y),
					tan_in_x: mix(a.tan_in_x, b.tan_in_x),
					tan_in_y: mix(a.tan_in_y, b.tan_in_y),
					tan_out_x: mix(a.tan_out_x, b.tan_out_x),
					tan_out_y: mix(a.tan_out_y, b.tan_out_y),
				})
				.collect(),
			..self.clone()
		}
	}
}

/// Source text only ever holds, as in After Effects.
impl Animate for TextLayer {}

/// Numbers, points and colours interpolate; choices hold.
impl Animate for ParamValue {
	fn blend(&self, next: &Self, amount: f64) -> Self {
		use ParamValue as P;
		let wide = |a: f32, b: f32| lerp(f64::from(a), f64::from(b), amount) as f32;
		let channel = |a: u8, b: u8| lerp(f64::from(a), f64::from(b), amount).round() as u8;
		match (self, next) {
			(&P::Float(a), &P::Float(b)) => P::Float(lerp(a, b, amount)),
			(&P::Fixed(a), &P::Fixed(b)) => P::Fixed(wide(a, b)),
			(&P::Angle(a), &P::Angle(b)) => P::Angle(wide(a, b)),
			(&P::Slider(a), &P::Slider(b)) => P::Slider(lerp(f64::from(a), f64::from(b), amount).round() as i32),
			(&P::Point { x, y }, &P::Point { x: x1, y: y1 }) => P::Point {
				x: wide(x, x1),
				y: wide(y, y1),
			},
			(&P::Point3D { x, y, z }, &P::Point3D { x: x1, y: y1, z: z1 }) => P::Point3D {
				x: lerp(x, x1, amount),
				y: lerp(y, y1, amount),
				z: lerp(z, z1, amount),
			},
			(
				&P::Color {
					red,
					green,
					blue,
					alpha,
				},
				&P::Color {
					red: r1,
					green: g1,
					blue: b1,
					alpha: a1,
				},
			) => P::Color {
				red: channel(red, r1),
				green: channel(green, g1),
				blue: channel(blue, b1),
				alpha: channel(alpha, a1),
			},
			_ => self.clone(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn at(value: i32, scale: u32) -> TimePoint {
		TimePoint::new(value, scale).unwrap()
	}

	fn ramp() -> Track<f64> {
		Track::keyed(vec![
			Keyframe::linear(at(30, 30), 100.0),
			Keyframe::linear(at(-15, 30), -50.0),
			Keyframe::hold(at(60, 30), 10.0),
			Keyframe::linear(at(90, 30), 20.0),
		])
		.unwrap()
	}

	#[test]
	fn instants_compare_across_scales() {
		assert_eq!(at(1001, 30000), at(2002, 60000));
		assert!(at(1001, 30000) > at(1, 30));
		assert!(at(i32::MIN, 1) < at(i32::MAX, u32::MAX));
		assert!(TimePoint::new(1, 0).is_none());
	}

	#[test]
	fn a_ratio_keeps_the_scale_when_exact_and_is_exact_otherwise() {
		let same = TimePoint::from_ratio(1, 2, 30);
		assert_eq!((same.value(), same.scale()), (15, 30));
		let finer = TimePoint::from_ratio(1, 7, 30);
		assert_eq!((finer.value(), finer.scale()), (1, 7));
		let flipped = TimePoint::from_ratio(3, -6, 30);
		assert_eq!((flipped.value(), flipped.scale()), (-15, 30));
		let huge = TimePoint::from_ratio(i128::from(i64::MAX), 1, 30);
		assert_eq!(huge.value(), i32::MAX);
		assert_eq!(at(1001, 30000).since(at(1, 30)), at(1, 30000));
	}

	#[test]
	fn the_ends_hold_and_the_middle_interpolates() {
		let track = ramp();
		assert_eq!(track.at(at(-900, 30)), -50.0);
		assert_eq!(track.at(at(-15, 30)), -50.0);
		assert_eq!(track.at(TimePoint::ZERO), 0.0);
		assert_eq!(track.at(at(30, 30)), 100.0);
		assert_eq!(track.at(at(45, 30)), 55.0);
		assert_eq!(track.at(at(900, 30)), 20.0);
	}

	#[test]
	fn hold_lasts_until_the_next_key_exactly() {
		let track = ramp();
		assert_eq!(track.at(at(2, 1)), 10.0);
		assert_eq!(track.at(at(89_999, 30_000)), 10.0);
		assert_eq!(track.at(at(3, 1)), 20.0);
	}

	#[test]
	fn the_same_instant_is_the_same_value_in_any_scale_and_order() {
		let track = ramp();
		for frame in [80, -40, 3, 44, 3, 0, 61, -15] {
			let value = track.at(at(frame, 30));
			assert_eq!(value, track.at(at(frame * 1000, 30_000)), "frame {frame}");
			assert_eq!(value, track.at(at(frame * 7, 210)), "frame {frame}");
		}
	}

	#[test]
	fn keys_need_times_of_their_own() {
		assert!(Track::<f64>::keyed(vec![]).is_none());
		let twice = vec![Keyframe::linear(at(1, 2), 0.0), Keyframe::hold(at(15, 30), 1.0)];
		assert!(Track::keyed(twice).is_none());
	}

	#[test]
	fn values_without_a_between_hold() {
		let popup = ParamValue::Popup(1).blend(&ParamValue::Popup(4), 0.9);
		assert_eq!(popup, ParamValue::Popup(1));
		assert_eq!(
			ParamValue::Slider(0).blend(&ParamValue::Slider(3), 0.5),
			ParamValue::Slider(2)
		);

		let line = |n: usize, y: f64| MaskPath {
			vertices: (0..n).map(|x| MaskVertex::corner(x as f64, y)).collect(),
			..Default::default()
		};
		assert_eq!(line(3, 0.0).blend(&line(3, 10.0), 0.5), line(3, 5.0));
		assert_eq!(line(3, 0.0).blend(&line(4, 10.0), 0.5), line(3, 0.0));
	}
}
