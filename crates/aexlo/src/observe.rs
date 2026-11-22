//! Observation hooks: what the plugin is asked to do and what it calls back.
//!
//! An [`Observer`] installed with [`PluginInstance::set_observer`] sees every
//! command sent to the plugin (cheap enough to leave on while benchmarking)
//! and, at [`ObserveLevel::Calls`], every host suite function the plugin
//! calls. Deciding what an event *means* (a failure, a slow phase) is left to
//! the observer; the host only reports.
//!
//! [`PluginInstance::set_observer`]: crate::PluginInstance::set_observer

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::Duration;

/// How much an [`Observer`] is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ObserveLevel {
	/// One event as each command begins and ends.
	Commands,
	/// Commands, plus one event per host suite function the plugin calls on
	/// the thread that dispatched the command. Calls made from iterate worker
	/// threads are not reported.
	Calls,
	/// Calls, with their arguments (and result, where the host reports one)
	/// as text in [`SuiteCallEvent::args`]: what the `diagnostics` feature
	/// prints, without rebuilding. Formatting them costs time on every call.
	Arguments,
}

/// Whether a [`CommandEvent`] marks the start or the end of a command.
///
/// A closed set: a command only ever begins and ends, so this enum is
/// deliberately exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPhase {
	/// The entry point is about to be called.
	Begin,
	/// The entry point returned.
	End,
}

/// A command sent to the plugin's entry point.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CommandEvent {
	/// The `PF_Cmd_*` value.
	pub command: i32,
	/// The SDK name without its prefix, e.g. `"SMART_RENDER"`.
	pub name: &'static str,
	/// Whether the command is starting or has finished.
	pub phase: CommandPhase,
	/// Time spent inside the entry point; set on [`CommandPhase::End`].
	pub duration: Option<Duration>,
	/// The `PF_Err` the plugin returned, when not `PF_Err_NONE`; set on
	/// [`CommandPhase::End`].
	pub error: Option<i32>,
}

/// A host suite function the plugin called.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SuiteCallEvent {
	/// The suite and its version, e.g. `"PF_WorldSuite2"`.
	pub suite: &'static str,
	/// The function, e.g. `"PF_NewWorld"`.
	pub function: &'static str,
	/// The arguments as `(name, value)` text, at [`ObserveLevel::Arguments`];
	/// empty otherwise. Pointers are shown as addresses, and names ending in
	/// `(out)` are what the host wrote back.
	pub args: Vec<(&'static str, String)>,
	/// What the host returned, for the calls that report it, at
	/// [`ObserveLevel::Arguments`].
	pub result: Option<String>,
}

/// The plugin got memory from the host or gave it back. Reported whether or
/// not [`Strict::track_allocations`] is on.
///
/// [`Strict::track_allocations`]: crate::Strict::track_allocations
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AllocationEvent {
	/// What was allocated or disposed.
	pub kind: AllocationKind,
	/// `true` for a new allocation, `false` for a disposal.
	pub allocated: bool,
	/// The allocation's address, as the plugin sees it.
	pub address: usize,
	/// The allocation's size in bytes.
	pub bytes: usize,
}

/// What kind of host memory an [`AllocationEvent`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AllocationKind {
	/// A memory handle (`PF_NewHandle` and the Handle Suite).
	Handle,
	/// A pixel world (`PF_NewWorld` and the World Suite).
	World,
}

/// A smart-render checkout or checkin. Reported whether or not
/// [`Strict::track_checkouts`] is on.
///
/// [`Strict::track_checkouts`]: crate::Strict::track_checkouts
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CheckoutEvent {
	/// Which checkout call this is.
	pub kind: CheckoutKind,
	/// The checkout id the plugin chose, or the parameter index for params.
	pub id: i32,
}

/// Which checkout call a [`CheckoutEvent`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
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

/// What kind of gap in aexlo an [`UnsupportedEvent`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnsupportedKind {
	/// `AcquireSuite` for a suite, or a version of one, that aexlo does not
	/// serve. The plugin got an error and no suite.
	MissingSuite,
	/// A host function aexlo only stubs: it reported success without doing
	/// anything.
	Stub,
	/// A host function aexlo serves, but not for these arguments (an unknown
	/// pixel format, a registration it cannot honor, ...).
	Declined,
}

/// The plugin asked for something aexlo does not do: what a plugin that runs
/// but renders wrong (or not at all) most often hit. Reported at every
/// [`ObserveLevel`], for calls made on the thread that dispatched the
/// command.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct UnsupportedEvent {
	/// What kind of gap this is.
	pub kind: UnsupportedKind,
	/// The suite name for [`UnsupportedKind::MissingSuite`] (e.g.
	/// `"AEGP Mask Suite"`), else the function as `"Suite/Function"` (or the
	/// callback's name).
	pub name: String,
	/// Which version or argument was not served, e.g. `"v6"` or
	/// `"which=7"`; empty when the name says it all.
	pub detail: String,
}

impl UnsupportedEvent {
	/// An event as the host reports it (for feeding observers in tests).
	pub fn new(kind: UnsupportedKind, name: impl Into<String>, detail: impl Into<String>) -> Self {
		Self {
			kind,
			name: name.into(),
			detail: detail.into(),
		}
	}
}

/// A host allocation the plugin still holds, as listed by
/// [`PluginInstance::allocations`](crate::PluginInstance::allocations).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LiveAllocation {
	/// What was allocated.
	pub kind: AllocationKind,
	/// The allocation's address, as the plugin sees it: the handle for a
	/// [`AllocationKind::Handle`], the first pixel for a
	/// [`AllocationKind::World`].
	pub address: usize,
	/// The size in bytes it was allocated with.
	pub bytes: usize,
	/// The command during which it was allocated, e.g. `"GLOBAL_SETUP"`.
	pub allocated_in: &'static str,
}

/// Receives events from a [`PluginInstance`](crate::PluginInstance). Every
/// method has an empty default, so implement only what you need.
///
/// Methods run synchronously inside the host, some while the plugin waits on
/// them; keep them short and never call back into the instance.
pub trait Observer: Send + Sync {
	/// A command began or ended.
	fn command(&self, _event: &CommandEvent) {}
	/// The plugin called a host suite function (only at [`ObserveLevel::Calls`]).
	fn suite_call(&self, _event: &SuiteCallEvent) {}
	/// The plugin got memory from the host or gave it back.
	fn allocation(&self, _event: &AllocationEvent) {}
	/// The plugin checked a layer or parameter out or in.
	fn checkout(&self, _event: &CheckoutEvent) {}
	/// The plugin asked for something aexlo does not do.
	fn unsupported(&self, _event: &UnsupportedEvent) {}
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
		let args: Vec<String> = event
			.args
			.iter()
			.map(|(name, value)| format!("{name}: {value}"))
			.collect();
		match &event.result {
			Some(result) => log::debug!("{}::{}({}) -> {result}", event.suite, event.function, args.join(", ")),
			None => log::debug!("{}::{}({})", event.suite, event.function, args.join(", ")),
		}
	}

	fn allocation(&self, event: &AllocationEvent) {
		let verb = if event.allocated { "allocated" } else { "disposed" };
		log::debug!("{verb} {:?} {:#x} ({} bytes)", event.kind, event.address, event.bytes);
	}

	fn checkout(&self, event: &CheckoutEvent) {
		log::debug!("{:?} #{}", event.kind, event.id);
	}

	fn unsupported(&self, event: &UnsupportedEvent) {
		log::debug!("unsupported ({:?}): {} {}", event.kind, event.name, event.detail);
	}
}

/// The SDK name of a `PF_Cmd_*` value, without its `PF_Cmd_` prefix.
pub fn command_name(command: i32) -> &'static str {
	use crate::sys::*;
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
	/// The observer of the command being dispatched on this thread, at any
	/// level: it hears about [`UnsupportedEvent`]s.
	static ANY: RefCell<Option<Arc<dyn Observer>>> = const { RefCell::new(None) };
	/// Whether that observer asked for [`ObserveLevel::Arguments`].
	static DETAILS: Cell<bool> = const { Cell::new(false) };
}

/// Make `installed` the observer of this thread's host callbacks until the
/// returned guard drops, restoring whatever was there (commands nest: a
/// plugin's setup may trigger an arbitrary-data callback). Suite calls reach
/// it only at [`ObserveLevel::Calls`] and above.
pub(crate) fn enter(installed: Option<&Installed>) -> CallsGuard {
	let any = installed.map(|i| i.observer.clone());
	let calls = installed
		.filter(|i| i.level >= ObserveLevel::Calls)
		.map(|i| i.observer.clone());
	let details = installed.is_some_and(|i| i.level >= ObserveLevel::Arguments);
	CallsGuard {
		previous_any: ANY.with(|a| std::mem::replace(&mut *a.borrow_mut(), any)),
		previous: CALLS.with(|c| std::mem::replace(&mut *c.borrow_mut(), calls)),
		previous_details: DETAILS.with(|d| d.replace(details)),
	}
}

pub(crate) struct CallsGuard {
	previous_any: Option<Arc<dyn Observer>>,
	previous: Option<Arc<dyn Observer>>,
	previous_details: bool,
}

impl Drop for CallsGuard {
	fn drop(&mut self) {
		let previous_any = self.previous_any.take();
		ANY.with(|a| *a.borrow_mut() = previous_any);
		let previous = self.previous.take();
		CALLS.with(|c| *c.borrow_mut() = previous);
		DETAILS.with(|d| d.set(self.previous_details));
	}
}

/// Report that the plugin asked for something aexlo does not do: logged as
/// a warning, and handed to this thread's observer.
pub(crate) fn unsupported(kind: UnsupportedKind, name: impl Into<String>, detail: impl Into<String>) {
	let event = UnsupportedEvent::new(kind, name, detail);
	log::warn!("unsupported ({:?}): {} {}", event.kind, event.name, event.detail);
	ANY.with(|a| {
		if let Some(observer) = a.borrow().as_ref() {
			observer.unsupported(&event);
		}
	});
}

/// Whether this thread's observer wants arguments, so `diag!` should format
/// them.
pub(crate) fn wants_details() -> bool {
	DETAILS.with(Cell::get)
}

/// Report a suite call named `"Suite/Function"` to this thread's observer.
/// Called from every host callback through `diag!`; a thread-local check when
/// nobody listens.
pub(crate) fn suite_call(name: &'static str) {
	CALLS.with(|c| {
		if let Some(observer) = c.borrow().as_ref() {
			let (suite, function) = name.split_once('/').unwrap_or(("", name));
			observer.suite_call(&SuiteCallEvent {
				suite,
				function,
				args: Vec::new(),
				result: None,
			});
		}
	});
}

/// [`suite_call`] with the arguments and result `diag!` formatted.
pub(crate) fn suite_call_detailed(name: &'static str, args: Vec<(&'static str, String)>, result: Option<String>) {
	// The `diagnostics` feature formats them for every call; hand them only
	// to an observer that asked.
	let (args, result) = if wants_details() {
		(args, result)
	} else {
		(Vec::new(), None)
	};
	CALLS.with(|c| {
		if let Some(observer) = c.borrow().as_ref() {
			let (suite, function) = name.split_once('/').unwrap_or(("", name));
			observer.suite_call(&SuiteCallEvent {
				suite,
				function,
				args,
				result,
			});
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

		fn unsupported(&self, event: &UnsupportedEvent) {
			self.0
				.lock()
				.unwrap()
				.push(format!("{:?} {} {}", event.kind, event.name, event.detail));
		}
	}

	fn installed(observer: Arc<Recorder>, level: ObserveLevel) -> Installed {
		Installed { observer, level }
	}

	#[test]
	fn names_commands_by_sdk_name() {
		assert_eq!(command_name(crate::sys::PF_Cmd_SMART_RENDER), "SMART_RENDER");
		assert_eq!(command_name(-5), "UNKNOWN");
	}

	#[test]
	fn suite_calls_reach_only_the_entered_observer() {
		let recorder = Arc::new(Recorder::default());
		suite_call("Ignored/Before");
		{
			let _guard = enter(Some(&installed(recorder.clone(), ObserveLevel::Calls)));
			suite_call("PF_WorldSuite2/PF_NewWorld");
			{
				let _inner = enter(None);
				assert!(!wants_details(), "no observer, no details");
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

	#[test]
	fn unsupported_reaches_an_observer_at_every_level() {
		let recorder = Arc::new(Recorder::default());
		unsupported(UnsupportedKind::Stub, "Ignored/Before", "");
		{
			let _guard = enter(Some(&installed(recorder.clone(), ObserveLevel::Commands)));
			suite_call("Ignored/NotAtCommands");
			unsupported(UnsupportedKind::MissingSuite, "AEGP Mask Suite", "v6");
			{
				let _inner = enter(None);
				unsupported(UnsupportedKind::Stub, "Ignored/Nested", "");
			}
			unsupported(UnsupportedKind::Declined, "PF_WorldSuite2/PF_NewWorld", "format=7");
		}
		unsupported(UnsupportedKind::Stub, "Ignored/After", "");
		assert_eq!(
			*recorder.0.lock().unwrap(),
			[
				"MissingSuite AEGP Mask Suite v6",
				"Declined PF_WorldSuite2/PF_NewWorld format=7"
			]
		);
	}
}
