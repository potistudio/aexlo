//! What a `PF_State` covers: the receipt `PF_GetCurrentState` gives, for a
//! plugin to tell whether what it cached is still good.
//!
//! Two states are identical only if everything they cover is:
//!
//! - a parameter's value over the time range asked about. Keyframes are placed
//!   relative to the range's start, so a value that does not move has one
//!   state in every range and a moving one a different state;
//! - a layer parameter's pixels, and for the input layer also the masks, text
//!   and shapes on the effect's layer;
//! - for a path parameter, the masks over the range, as for a value;
//! - for arbitrary data, the value as the plugin flattens it, taken when the
//!   host takes a value in (after setup and after the commands in which the
//!   plugin changes its values); for a value the plugin cannot flatten, the
//!   bytes in its handle.
//!
//! A null range is all time ("NULL for both start & duration means over all of
//! time", `AE_EffectSuites.h`). After Effects widens the range of an effect
//! with `AUTOMATIC_WIDE_TIME_INPUT` to the times that range needs. aexlo
//! takes those from the checkouts the effect has made so far: the range
//! moves out by the furthest any checkout reached before and after the time
//! it was made at, and takes in every time a checkout reached, so it covers
//! a simulation that runs from the start. Before any checkout it is all of
//! time.
//!
//! The host has one picture of each layer, the current one, so a state cannot
//! tell that a layer is unchanged in the past: a picture set for a new frame
//! reads as a change.

use std::hash::{DefaultHasher, Hash, Hasher};

use crate::PluginInstance;
use crate::animation::{Interpolation, TimePoint, Track};
use crate::sys::*;

/// Two hashes with different seeds, for the 128 bits of a `PF_State`.
struct Receipt(DefaultHasher, DefaultHasher);

impl Receipt {
	fn new() -> Self {
		let mut second = DefaultHasher::new();
		second.write_u64(0x9E37_79B9_7F4A_7C15);
		Self(DefaultHasher::new(), second)
	}

	fn state(&self) -> PF_State {
		let (a, b) = (self.0.finish(), self.1.finish());
		PF_State {
			reservedAL: [a, a >> 32, b, b >> 32].map(|word| word as A_long),
		}
	}
}

impl Hasher for Receipt {
	fn write(&mut self, bytes: &[u8]) {
		self.0.write(bytes);
		self.1.write(bytes);
	}

	fn finish(&self) -> u64 {
		self.0.finish()
	}
}

fn is_layer(def: &PF_ParamDef) -> bool {
	def.param_type == PF_Param_LAYER as PF_ParamType
}

/// The parameters `param_index` stands for, `None` for an index that means
/// nothing.
#[allow(non_upper_case_globals)]
pub(super) fn indices(instance: &PluginInstance, param_index: PF_ParamIndex) -> Option<Vec<usize>> {
	if let Ok(index) = usize::try_from(param_index) {
		return Some(vec![index]);
	}
	let all = 0..instance.param_count();
	let param = |index: &usize| instance.param_by_index(*index);
	Some(match param_index {
		PF_ParamIndex_NONE => Vec::new(),
		PF_ParamIndex_CHECK_ALL => all.collect(),
		// "Omit all layers": the input layer is one.
		PF_ParamIndex_CHECK_ALL_EXCEPT_LAYER_PARAMS => all
			.filter(|index| param(index).is_some_and(|def| !is_layer(def)))
			.collect(),
		PF_ParamIndex_CHECK_ALL_HONOR_EXCLUDE => {
			let excluded = PF_ParamFlag_EXCLUDE_FROM_HAVE_INPUTS_CHANGED as PF_ParamFlags;
			all.filter(|index| param(index).is_some_and(|def| def.flags & excluded == 0))
				.collect()
		}
		_ => return None,
	})
}

/// The state of `param_index` from `start` for `duration`.
pub(super) fn of(
	instance: &PluginInstance,
	param_index: PF_ParamIndex,
	start: Option<A_Time>,
	duration: Option<A_Time>,
) -> Option<PF_State> {
	let range = match (start, duration) {
		(None, _) => None,
		(Some(start), duration) => {
			let start = TimePoint::from_sys(start).unwrap_or_else(|| instance.time().into());
			let end = duration
				.and_then(TimePoint::from_sys)
				.map_or(start, |length| start.plus(length));
			Some((start.min(end), start.max(end)))
		}
	};
	// Before any checkout, the range asked about and all of time.
	let (range, all_time) = match range {
		Some(asked)
			if instance
				.out_flags2()
				.contains(crate::OutFlags2::AUTOMATIC_WIDE_TIME_INPUT) =>
		{
			match instance.widened_range(asked) {
				Some(widened) => (Some(widened), false),
				None => (Some(asked), true),
			}
		}
		range => (range, false),
	};

	let mut receipt = Receipt::new();
	for index in indices(instance, param_index)? {
		index.hash(&mut receipt);
		cover(instance, index, range, &mut receipt);
		if all_time {
			cover(instance, index, None, &mut receipt);
		}
	}
	Some(receipt.state())
}

fn cover(instance: &PluginInstance, index: usize, range: Option<(TimePoint, TimePoint)>, receipt: &mut Receipt) {
	let Some(def) = instance.param_by_index(index) else {
		return;
	};
	if is_layer(def) {
		instance.layer_digest(index).hash(receipt);
		let project = instance.project();
		project.and_then(|project| project.param_layer(index)).hash(receipt);
		if index == 0 {
			cover_masks(instance, range, receipt);
			format!("{:?}", instance.text_layer()).hash(receipt);
			if let Some(layer) = project.map(|project| project.effect_layer()) {
				format!("{:?}", layer.content).hash(receipt);
				// Contents move without changing.
				let (from, to) = range.unwrap_or((TimePoint::ZERO, TimePoint::ZERO));
				format!("{:?}{:?}", layer.outlines(from), layer.outlines(to)).hash(receipt);
			}
		}
		return;
	}
	if def.param_type == PF_Param_ARBITRARY_DATA as PF_ParamType {
		if let Some(digest) = instance.arb_digest(index) {
			("flat", digest).hash(receipt);
			return;
		}
		// SAFETY: the union member of an arbitrary-data parameter.
		let handle = unsafe { def.u.arb_d.value };
		match super::handle::owned_bytes(handle) {
			Some(bytes) => bytes.hash(receipt),
			// Only the plugin knows what its own memory holds.
			None => (handle as usize).hash(receipt),
		}
		return;
	}
	if def.param_type == PF_Param_PATH as PF_ParamType {
		cover_masks(instance, range, receipt);
	}
	match instance.param_track(index) {
		Some(track) => cover_track(track, range, receipt),
		None => {
			if let Some(value) = PluginInstance::value_of(def) {
				still(&value, receipt);
			}
		}
	}
}

/// The masks on the effect's layer over the range.
fn cover_masks(instance: &PluginInstance, range: Option<(TimePoint, TimePoint)>, receipt: &mut Receipt) {
	("masks", instance.mask_tracks().len()).hash(receipt);
	for track in instance.mask_tracks() {
		cover_track(track, range, receipt);
	}
}

/// A value that does not move over the range.
fn still<T: std::fmt::Debug>(value: &T, receipt: &mut Receipt) {
	("still", format!("{value:?}")).hash(receipt);
}

fn cover_track<T: crate::animation::Animate + PartialEq + std::fmt::Debug>(
	track: &Track<T>,
	range: Option<(TimePoint, TimePoint)>,
	receipt: &mut Receipt,
) {
	let keys = track.keys();
	let Some((from, to)) = range else {
		match keys {
			[] => {}
			[first, rest @ ..] if rest.iter().all(|key| key.value == first.value) => still(&first.value, receipt),
			_ => {
				for key in keys {
					(key.time.seconds().to_bits(), format!("{:?}", key.value)).hash(receipt);
					(key.interpolation == Interpolation::Hold).hash(receipt);
				}
			}
		}
		return;
	};

	let (first, last) = (track.at(from), track.at(to));
	let inside: Vec<_> = keys.iter().filter(|key| from < key.time && key.time < to).collect();
	if first == last && inside.iter().all(|key| key.value == first) {
		return still(&first, receipt);
	}
	// Between two keys the ends and how the segment runs say it all.
	let leading = keys.iter().rev().find(|key| key.time <= from);
	leading
		.map(|key| key.interpolation == Interpolation::Hold)
		.hash(receipt);
	format!("{first:?}{last:?}").hash(receipt);
	to.since(from).seconds().to_bits().hash(receipt);
	for key in inside {
		(key.time.since(from).seconds().to_bits(), format!("{:?}", key.value)).hash(receipt);
		(key.interpolation == Interpolation::Hold).hash(receipt);
	}
}
