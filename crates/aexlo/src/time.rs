//! Comp time, in the rational form After Effects hands to plugins.

/// A point in comp time: `value / scale` seconds, one frame lasting
/// `step / scale` seconds. These are `in_data.current_time`, `time_step` and
/// `time_scale` as the plugin sees them.
///
/// Plugins divide by `step` and `scale`, so a `Time` can only be built with
/// both positive ([`Time::new`], [`Time::frame`]); the fields are read through
/// accessors.
///
/// Equality and hashing compare the three fields as they are, not the instant
/// they describe: `1/2` and `2/4` seconds are different values. Use
/// [`Time::is_same_instant`] to compare instants.
///
/// ```
/// use aexlo::Time;
///
/// // Frame 48 of a 24 fps comp: two seconds in.
/// let t = Time::frame(48, 24);
/// assert_eq!(t.seconds(), 2.0);
///
/// // 29.97 fps: 1001 / 30000 seconds per frame.
/// let ntsc = Time::new(1001 * 10, 1001, 30000).unwrap();
/// assert_eq!(ntsc.seconds(), 10.0 * 1001.0 / 30000.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Time {
	value: i32,
	step: i32,
	scale: u32,
}

impl Time {
	/// A time at `value / scale` seconds in a comp whose frames last
	/// `step / scale` seconds, or `None` unless both `step` and `scale` are
	/// positive.
	///
	/// ```
	/// use aexlo::Time;
	///
	/// assert!(Time::new(0, 1, 24).is_some());
	/// assert!(Time::new(0, 1, 0).is_none());
	/// assert!(Time::new(0, 0, 24).is_none());
	/// ```
	#[must_use]
	pub const fn new(value: i32, step: i32, scale: u32) -> Option<Self> {
		if step > 0 && scale > 0 {
			Some(Self { value, step, scale })
		} else {
			None
		}
	}

	/// Frame `frame` of a comp running at a whole `fps` frames per second.
	///
	/// # Panics
	/// If `fps` is zero.
	///
	/// ```
	/// use aexlo::Time;
	///
	/// assert_eq!(Time::frame(24, 24).seconds(), 1.0);
	/// ```
	#[must_use]
	pub const fn frame(frame: i32, fps: u32) -> Self {
		assert!(fps > 0, "frame rate must be positive");
		Self {
			value: frame,
			step: 1,
			scale: fps,
		}
	}

	/// The time, in units of `1 / scale` seconds (`in_data.current_time`).
	#[must_use]
	pub const fn value(self) -> i32 {
		self.value
	}

	/// The duration of one frame, in units of `1 / scale` seconds
	/// (`in_data.time_step`). Always positive.
	#[must_use]
	pub const fn step(self) -> i32 {
		self.step
	}

	/// Units per second (`in_data.time_scale`). Always positive.
	#[must_use]
	pub const fn scale(self) -> u32 {
		self.scale
	}

	/// `(value, step, scale)`, in the order of the `in_data` fields.
	#[must_use]
	pub const fn into_parts(self) -> (i32, i32, u32) {
		(self.value, self.step, self.scale)
	}

	/// This time in seconds (`value / scale`).
	///
	/// ```
	/// use aexlo::Time;
	///
	/// assert_eq!(Time::frame(15, 30).seconds(), 0.5);
	/// ```
	#[must_use]
	pub fn seconds(self) -> f64 {
		f64::from(self.value) / f64::from(self.scale)
	}

	/// How long one frame lasts, in seconds (`step / scale`).
	///
	/// ```
	/// use aexlo::Time;
	///
	/// assert_eq!(Time::frame(0, 25).frame_duration(), 0.04);
	/// ```
	#[must_use]
	pub fn frame_duration(self) -> f64 {
		f64::from(self.step) / f64::from(self.scale)
	}

	/// Whether both describe the same instant, whatever their `step` and
	/// `scale` (`1/2` and `2/4` seconds are the same instant). Exact: it
	/// compares by cross-multiplication, not through floating point.
	///
	/// ```
	/// use aexlo::Time;
	///
	/// // Frame 12 at 24 fps and frame 24 at 48 fps are both half a second in.
	/// assert!(Time::frame(12, 24).is_same_instant(Time::frame(24, 48)));
	/// assert_ne!(Time::frame(12, 24), Time::frame(24, 48));
	/// ```
	#[must_use]
	pub const fn is_same_instant(self, other: Self) -> bool {
		// `i32 * u32` always fits in an `i64`.
		self.value as i64 * other.scale as i64 == other.value as i64 * self.scale as i64
	}
}

impl Default for Time {
	/// Frame 0 of a 30 fps comp, where a freshly loaded instance starts.
	fn default() -> Self {
		Self::frame(0, 30)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn new_rejects_non_positive_step_and_scale() {
		assert!(Time::new(5, 1, 24).is_some());
		assert!(Time::new(5, 0, 24).is_none());
		assert!(Time::new(5, -1, 24).is_none());
		assert!(Time::new(5, 1, 0).is_none());
	}

	#[test]
	fn new_accepts_any_value() {
		let t = Time::new(-48, 1, 24).unwrap();
		assert_eq!(t.seconds(), -2.0);
		assert!(Time::new(i32::MIN, 1, u32::MAX).is_some());
	}

	#[test]
	#[should_panic(expected = "frame rate must be positive")]
	fn frame_panics_on_zero_fps() {
		let _ = Time::frame(0, 0);
	}

	#[test]
	fn accessors_and_parts_follow_in_data_order() {
		let t = Time::new(10010, 1001, 30000).unwrap();
		assert_eq!((t.value(), t.step(), t.scale()), (10010, 1001, 30000));
		assert_eq!(t.into_parts(), (10010, 1001, 30000));
		assert_eq!(Time::frame(7, 24).into_parts(), (7, 1, 24));
	}

	#[test]
	fn ntsc_seconds_and_frame_duration() {
		let t = Time::new(1001 * 10, 1001, 30000).unwrap();
		assert_eq!(t.seconds(), 10.0 * 1001.0 / 30000.0);
		assert_eq!(t.frame_duration(), 1001.0 / 30000.0);
	}

	#[test]
	fn equality_is_structural_but_instants_compare_exactly() {
		let a = Time::frame(12, 24);
		let b = Time::frame(24, 48);
		assert_ne!(a, b);
		assert!(a.is_same_instant(b));
		assert!(!a.is_same_instant(Time::frame(13, 24)));
		// The extremes still compare exactly.
		let max = Time::new(i32::MAX, 1, u32::MAX).unwrap();
		assert!(max.is_same_instant(max));
		assert!(!max.is_same_instant(Time::new(i32::MIN, 1, u32::MAX).unwrap()));
	}

	#[test]
	fn default_is_frame_zero_at_30_fps() {
		assert_eq!(Time::default(), Time::frame(0, 30));
		assert_eq!(Time::default().seconds(), 0.0);
	}
}
