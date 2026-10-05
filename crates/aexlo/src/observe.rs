//! Observation hooks: what the plugin is asked to do and what it calls back.
//!
//! An [`Observer`] installed with [`PluginInstance::set_observer`] sees every
//! command sent to the plugin (cheap enough to leave on while benchmarking)
//! and, at [`ObserveLevel::Calls`], every host suite function the plugin
//! calls. Deciding what an event *means* (a failure, a slow phase) is left to
//! the observer; the host only reports.
//!
//! [`PluginInstance::set_observer`]: crate::PluginInstance::set_observer

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

/// How much an [`Observer`] is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ObserveLevel {
	/// One event as each command begins and ends.
	Commands,
	/// Commands, plus one event per host suite function the plugin calls on
	/// the thread that dispatched the command. Calls made from iterate worker
	/// threads are not reported.
	Calls,
}

/// Whether a [`CommandEvent`] marks the start or the end of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPhase {
	Begin,
	End,
}

/// A command sent to the plugin's entry point.
#[derive(Debug, Clone)]
pub struct CommandEvent {
	/// The `PF_Cmd_*` value.
	pub command: i32,
	/// The SDK name without its prefix, e.g. `"SMART_RENDER"`.
	pub name: &'static str,
	pub phase: CommandPhase,
	/// Time spent inside the entry point; set on [`CommandPhase::End`].
	pub duration: Option<Duration>,
	/// The `PF_Err` the plugin returned, when not `PF_Err_NONE`; set on
	/// [`CommandPhase::End`].
	pub error: Option<i32>,
}

/// A host suite function the plugin called.
#[derive(Debug, Clone)]
pub struct SuiteCallEvent {
	/// The suite and its version, e.g. `"PF_WorldSuite2"`.
	pub suite: &'static str,
	/// The function, e.g. `"PF_NewWorld"`.
	pub function: &'static str,
}

/// A host-tracked allocation changed (see [`Strict::track_allocations`]).
///
/// [`Strict::track_allocations`]: crate::Strict::track_allocations
#[derive(Debug, Clone)]
pub struct AllocationEvent {
	pub kind: AllocationKind,
	/// `true` for a new allocation, `false` for a disposal.
	pub allocated: bool,
	/// The allocation's address, as the plugin sees it.
	pub address: usize,
	pub bytes: usize,
}

/// What kind of host memory an [`AllocationEvent`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllocationKind {
	Handle,
	World,
}

/// A smart-render checkout (see [`Strict::track_checkouts`]).
///
/// [`Strict::track_checkouts`]: crate::Strict::track_checkouts
#[derive(Debug, Clone)]
pub struct CheckoutEvent {
	pub kind: CheckoutKind,
	/// The checkout id the plugin chose, or the parameter index for params.
	pub id: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckoutKind {
	/// `checkout_layer` during `SMART_PRE_RENDER`.
	DeclareLayer,
	/// `checkout_layer_pixels` during `SMART_RENDER`.
	CheckoutLayerPixels,
	/// `checkin_layer_pixels` during `SMART_RENDER`.
	CheckinLayerPixels,
	/// `PF_CHECKOUT_PARAM`.
	CheckoutParam,
	/// `PF_CHECKIN_PARAM`.
	CheckinParam,
}

/// Receives events from a [`PluginInstance`](crate::PluginInstance). Every
/// method has an empty default, so implement only what you need.
///
/// Methods run synchronously inside the host, some while the plugin waits on
/// them; keep them short and never call back into the instance.
pub trait Observer: Send + Sync {
	fn command(&self, _event: &CommandEvent) {}
	fn suite_call(&self, _event: &SuiteCallEvent) {}
	fn allocation(&self, _event: &AllocationEvent) {}
	fn checkout(&self, _event: &CheckoutEvent) {}
}

/// An [`Observer`] that logs every event at `debug` level through the `log`
/// crate: the command trace the `diagnostics` feature prints, without
/// rebuilding.
#[derive(Debug, Default, Clone, Copy)]
pub struct LogObserver;

impl Observer for LogObserver {
	fn command(&self, event: &CommandEvent) {
		match (event.phase, event.duration) {
			(CommandPhase::End, Some(duration)) => match event.error {
				Some(code) => log::debug!("PF_Cmd_{} failed with {code} after {duration:?}", event.name),
				None => log::debug!("PF_Cmd_{} took {duration:?}", event.name),
			},
			_ => log::debug!("PF_Cmd_{} begins", event.name),
		}
	}

	fn suite_call(&self, event: &SuiteCallEvent) {
		log::debug!("{}::{}", event.suite, event.function);
	}

	fn allocation(&self, event: &AllocationEvent) {
		let verb = if event.allocated { "allocated" } else { "disposed" };
		log::debug!("{verb} {:?} {:#x} ({} bytes)", event.kind, event.address, event.bytes);
	}

	fn checkout(&self, event: &CheckoutEvent) {
		log::debug!("{:?} #{}", event.kind, event.id);
	}
}

/// The SDK name of a `PF_Cmd_*` value, without its `PF_Cmd_` prefix.
#[allow(clippy::unnecessary_cast)] // `PF_Cmd_*` is `u32` on macOS only.
pub fn command_name(command: i32) -> &'static str {
	use after_effects_sys::*;
	const NAMES: &[(u32, &str)] = &[
		(PF_Cmd_ABOUT as u32, "ABOUT"),
		(PF_Cmd_GLOBAL_SETUP as u32, "GLOBAL_SETUP"),
		(PF_Cmd_GLOBAL_SETDOWN as u32, "GLOBAL_SETDOWN"),
		(PF_Cmd_PARAMS_SETUP as u32, "PARAMS_SETUP"),
		(PF_Cmd_SEQUENCE_SETUP as u32, "SEQUENCE_SETUP"),
		(PF_Cmd_SEQUENCE_RESETUP as u32, "SEQUENCE_RESETUP"),
		(PF_Cmd_SEQUENCE_FLATTEN as u32, "SEQUENCE_FLATTEN"),
		(PF_Cmd_SEQUENCE_SETDOWN as u32, "SEQUENCE_SETDOWN"),
		(PF_Cmd_DO_DIALOG as u32, "DO_DIALOG"),
		(PF_Cmd_FRAME_SETUP as u32, "FRAME_SETUP"),
		(PF_Cmd_RENDER as u32, "RENDER"),
		(PF_Cmd_FRAME_SETDOWN as u32, "FRAME_SETDOWN"),
		(PF_Cmd_USER_CHANGED_PARAM as u32, "USER_CHANGED_PARAM"),
		(PF_Cmd_UPDATE_PARAMS_UI as u32, "UPDATE_PARAMS_UI"),
		(PF_Cmd_EVENT as u32, "EVENT"),
		(PF_Cmd_GET_EXTERNAL_DEPENDENCIES as u32, "GET_EXTERNAL_DEPENDENCIES"),
		(PF_Cmd_COMPLETELY_GENERAL as u32, "COMPLETELY_GENERAL"),
		(PF_Cmd_QUERY_DYNAMIC_FLAGS as u32, "QUERY_DYNAMIC_FLAGS"),
		(PF_Cmd_AUDIO_RENDER as u32, "AUDIO_RENDER"),
		(PF_Cmd_AUDIO_SETUP as u32, "AUDIO_SETUP"),
		(PF_Cmd_AUDIO_SETDOWN as u32, "AUDIO_SETDOWN"),
		(PF_Cmd_ARBITRARY_CALLBACK as u32, "ARBITRARY_CALLBACK"),
		(PF_Cmd_SMART_PRE_RENDER as u32, "SMART_PRE_RENDER"),
		(PF_Cmd_SMART_RENDER as u32, "SMART_RENDER"),
		(PF_Cmd_GET_FLATTENED_SEQUENCE_DATA as u32, "GET_FLATTENED_SEQUENCE_DATA"),
		(PF_Cmd_TRANSLATE_PARAMS_TO_PREFS as u32, "TRANSLATE_PARAMS_TO_PREFS"),
		(PF_Cmd_SMART_RENDER_GPU as u32, "SMART_RENDER_GPU"),
		(PF_Cmd_GPU_DEVICE_SETUP as u32, "GPU_DEVICE_SETUP"),
		(PF_Cmd_GPU_DEVICE_SETDOWN as u32, "GPU_DEVICE_SETDOWN"),
	];
	NAMES
		.iter()
		.find(|(value, _)| *value == command as u32)
		.map_or("UNKNOWN", |(_, name)| name)
}

/// An installed observer and how much it asked to see.
#[derive(Clone)]
pub(crate) struct Installed {
	pub(crate) observer: Arc<dyn Observer>,
	pub(crate) level: ObserveLevel,
}

thread_local! {
	/// The observer of the command being dispatched on this thread, when it
	/// asked for [`ObserveLevel::Calls`]: suite callbacks without an
	/// `effect_ref` (and the ones that have one) are attributed through it.
	static CALLS: RefCell<Option<Arc<dyn Observer>>> = const { RefCell::new(None) };
}

/// Make `observer` the target of suite-call events on this thread until the
/// returned guard drops, restoring whatever was there (commands nest: a
/// plugin's setup may trigger an arbitrary-data callback).
pub(crate) fn enter_calls(observer: Option<Arc<dyn Observer>>) -> CallsGuard {
	let previous = CALLS.with(|c| std::mem::replace(&mut *c.borrow_mut(), observer));
	CallsGuard { previous }
}

pub(crate) struct CallsGuard {
	previous: Option<Arc<dyn Observer>>,
}

impl Drop for CallsGuard {
	fn drop(&mut self) {
		let previous = self.previous.take();
		CALLS.with(|c| *c.borrow_mut() = previous);
	}
}

/// Report a suite call named `"Suite/Function"` to this thread's observer.
/// Called from every host callback through `diag!`; a thread-local check when
/// nobody listens.
pub(crate) fn suite_call(name: &'static str) {
	CALLS.with(|c| {
		if let Some(observer) = c.borrow().as_ref() {
			let (suite, function) = name.split_once('/').unwrap_or(("", name));
			observer.suite_call(&SuiteCallEvent { suite, function });
		}
	});
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::Mutex;

	#[derive(Default)]
	struct Recorder(Mutex<Vec<String>>);

	impl Observer for Recorder {
		fn suite_call(&self, event: &SuiteCallEvent) {
			self.0
				.lock()
				.unwrap()
				.push(format!("{}::{}", event.suite, event.function));
		}
	}

	#[test]
	fn names_commands_by_sdk_name() {
		assert_eq!(
			command_name(after_effects_sys::PF_Cmd_SMART_RENDER as i32),
			"SMART_RENDER"
		);
		assert_eq!(command_name(-5), "UNKNOWN");
	}

	#[test]
	fn suite_calls_reach_only_the_entered_observer() {
		let recorder = Arc::new(Recorder::default());
		suite_call("Ignored/Before");
		{
			let _guard = enter_calls(Some(recorder.clone()));
			suite_call("PF_WorldSuite2/PF_NewWorld");
			{
				let _inner = enter_calls(None);
				suite_call("Ignored/Nested");
			}
			suite_call("PF_HandleSuite1/host_new_handle");
		}
		suite_call("Ignored/After");
		assert_eq!(
			*recorder.0.lock().unwrap(),
			["PF_WorldSuite2::PF_NewWorld", "PF_HandleSuite1::host_new_handle"]
		);
	}
}
