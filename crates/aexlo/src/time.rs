//! Comp time, in the rational form After Effects hands to plugins.

/// A point in comp time: `value / scale` seconds, one frame lasting
/// `step / scale` seconds. These are `in_data.current_time`, `time_step` and
/// `time_scale` as the plugin sees them.
///
/// ```
/// use aexlo::Time;
///
/// // Frame 48 of a 24 fps comp: two seconds in.
/// let t = Time::frame(48, 24);
/// assert_eq!(t.seconds(), 2.0);
///
/// // 29.97 fps: 1001 / 30000 seconds per frame.
/// let ntsc = Time { value: 1001 * 10, step: 1001, scale: 30000 };
/// assert_eq!(ntsc.seconds(), 10.0 * 1001.0 / 30000.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Time {
	/// The time, in units of `1 / scale` seconds.
	pub value: i32,
	/// The duration of one frame, in units of `1 / scale` seconds. Must be
	/// positive.
	pub step: i32,
	/// Units per second. Must be positive.
	pub scale: u32,
}

impl Time {
	/// Frame `frame` of a comp running at a whole `fps` frames per second.
	pub const fn frame(frame: i32, fps: u32) -> Self {
		Self {
			value: frame,
			step: 1,
			scale: fps,
		}
	}

	/// This time in seconds.
	pub fn seconds(&self) -> f64 {
		f64::from(self.value) / f64::from(self.scale)
	}
}

impl Default for Time {
	/// Frame 0 of a 30 fps comp, where a freshly loaded instance starts.
	fn default() -> Self {
		Self::frame(0, 30)
	}
}
