//! AEGP registration scoped to an owning plugin instance.
#![allow(non_snake_case)]
use crate::sys::*;
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type IdleFn = unsafe extern "C" fn(AEGP_GlobalRefcon, AEGP_IdleRefcon, *mut A_long) -> A_Err;
struct Entry {
	owner: u64,
	global: usize,
	idle: Option<(IdleFn, usize)>,
}
static ENTRIES: OnceLock<Mutex<HashMap<AEGP_PluginID, Entry>>> = OnceLock::new();
static GATES: OnceLock<Mutex<HashMap<u64, Arc<Mutex<()>>>>> = OnceLock::new();
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
thread_local! { static CURRENT_OWNER: Cell<Option<u64>> = const { Cell::new(None) }; }
fn entries() -> &'static Mutex<HashMap<AEGP_PluginID, Entry>> {
	ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
}
pub(crate) fn new_owner() -> u64 {
	let owner = NEXT_OWNER.fetch_add(1, Ordering::Relaxed);
	if let Ok(mut gates) = GATES.get_or_init(|| Mutex::new(HashMap::new())).lock() {
		gates.insert(owner, Arc::new(Mutex::new(())));
	}
	owner
}
pub(crate) struct OwnerGuard(Option<u64>);
impl Drop for OwnerGuard {
	fn drop(&mut self) {
		CURRENT_OWNER.with(|o| o.set(self.0));
	}
}
pub(crate) fn enter_owner(owner: u64) -> OwnerGuard {
	OwnerGuard(CURRENT_OWNER.with(|o| o.replace(Some(owner))))
}
pub(crate) fn register_plugin(id: AEGP_PluginID, global: AEGP_GlobalRefcon) -> bool {
	let owner = CURRENT_OWNER.with(Cell::get);
	let Some(owner) = owner else {
		return false;
	};
	let Ok(mut entries) = entries().lock() else {
		return false;
	};
	entries.insert(
		id,
		Entry {
			owner,
			global: global as usize,
			idle: None,
		},
	);
	true
}
/// Dispatch only at host-controlled boundaries while the owning DLL is alive.
/// This is not a continuous AE UI event loop.
pub(crate) fn dispatch_idle(owner: u64) {
	let gate = GATES
		.get_or_init(|| Mutex::new(HashMap::new()))
		.lock()
		.ok()
		.and_then(|gates| gates.get(&owner).cloned());
	let Some(gate) = gate else {
		return;
	};
	let Ok(_guard) = gate.lock() else {
		return;
	};
	let callbacks: Vec<_> = match entries().lock() {
		Ok(entries) => entries
			.values()
			.filter(|e| e.owner == owner)
			.filter_map(|e| e.idle.map(|(f, r)| (f, e.global, r)))
			.collect(),
		Err(_) => return,
	};
	let _scope = enter_owner(owner);
	for (callback, global, reference) in callbacks {
		let mut sleep = 1000;
		let err = unsafe { callback(global as AEGP_GlobalRefcon, reference as AEGP_IdleRefcon, &mut sleep) };
		if err != 0 {
			log::warn!("AEGP idle hook returned {err}");
		}
	}
}
pub(crate) fn remove_owner(owner: u64) {
	let gate = GATES
		.get_or_init(|| Mutex::new(HashMap::new()))
		.lock()
		.ok()
		.and_then(|gates| gates.get(&owner).cloned());
	let Some(gate) = gate else {
		return;
	};
	let Ok(_guard) = gate.lock() else {
		return;
	};
	if let Ok(mut entries) = entries().lock() {
		entries.retain(|_, e| e.owner != owner);
	}
	if let Ok(mut gates) = GATES.get_or_init(|| Mutex::new(HashMap::new())).lock() {
		gates.remove(&owner);
	}
}

macro_rules! hook {
	($name:ident, $callback:ty, $refcon:ty) => {
		unsafe extern "C" fn $name(_: AEGP_PluginID, _: $callback, _: $refcon) -> A_Err {
			log::warn!("AEGP {} is unsupported", stringify!($name));
			PF_Err_INTERNAL_STRUCT_DAMAGED
		}
	};
}
hook!(update, AEGP_UpdateMenuHook, AEGP_UpdateMenuRefcon);
hook!(death, AEGP_DeathHook, AEGP_DeathRefcon);
hook!(version, AEGP_VersionHook, AEGP_VersionRefcon);
hook!(about_string, AEGP_AboutStringHook, AEGP_AboutStringRefcon);
hook!(about, AEGP_AboutHook, AEGP_AboutRefcon);
unsafe extern "C" fn idle(id: AEGP_PluginID, cb: AEGP_IdleHook, reference: AEGP_IdleRefcon) -> A_Err {
	crate::core::ffi::guard("idle", || {
		let Some(cb) = cb else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Ok(mut entries) = entries().lock() else {
			return PF_Err_INTERNAL_STRUCT_DAMAGED;
		};
		let Some(entry) = entries.get_mut(&id) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if CURRENT_OWNER.with(Cell::get) != Some(entry.owner) {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		entry.idle = Some((cb, reference as usize));
		PF_Err_NONE
	})
}
unsafe extern "C" fn command(
	_: AEGP_PluginID,
	_: AEGP_HookPriority,
	_: AEGP_Command,
	_: AEGP_CommandHook,
	_: AEGP_CommandRefcon,
) -> A_Err {
	crate::core::ffi::guard("command", || {
		log::warn!("AEGP command hook is unsupported");
		PF_Err_INTERNAL_STRUCT_DAMAGED
	})
}
unsafe extern "C" fn artisan(
	_: A_Version,
	_: A_Version,
	_: AEGP_PluginID,
	_: *mut c_void,
	_: *const A_char,
	_: *const A_char,
	_: *mut PR_ArtisanEntryPoints,
) -> A_Err {
	crate::core::ffi::guard("artisan", || {
		log::warn!("AEGP artisan registration is unsupported");
		PF_Err_INTERNAL_STRUCT_DAMAGED
	})
}
unsafe extern "C" fn io(
	_: AEGP_PluginID,
	_: AEGP_IORefcon,
	_: *const AEIO_ModuleInfo,
	_: *const AEIO_FunctionBlock4,
) -> A_Err {
	crate::core::ffi::guard("io", || {
		log::warn!("AEGP IO registration is unsupported");
		PF_Err_INTERNAL_STRUCT_DAMAGED
	})
}
unsafe extern "C" fn tracker(
	_: A_Version,
	_: A_Version,
	_: AEGP_PluginID,
	_: AEGP_GlobalRefcon,
	_: *const A_char,
	_: *const A_char,
	_: *const PT_TrackerEntryPoints,
) -> A_Err {
	crate::core::ffi::guard("tracker", || {
		log::warn!("AEGP tracker registration is unsupported");
		PF_Err_INTERNAL_STRUCT_DAMAGED
	})
}
unsafe extern "C" fn preset(_: *const A_char, _: *const A_char) -> A_Err {
	crate::core::ffi::guard("preset", || {
		log::warn!("AEGP preset localization is unsupported");
		PF_Err_INTERNAL_STRUCT_DAMAGED
	})
}
pub static REGISTER: AEGP_RegisterSuite5 = AEGP_RegisterSuite5 {
	AEGP_RegisterCommandHook: Some(command),
	AEGP_RegisterUpdateMenuHook: Some(update),
	AEGP_RegisterDeathHook: Some(death),
	AEGP_RegisterVersionHook: Some(version),
	AEGP_RegisterAboutStringHook: Some(about_string),
	AEGP_RegisterAboutHook: Some(about),
	AEGP_RegisterArtisan: Some(artisan),
	AEGP_RegisterIO: Some(io),
	AEGP_RegisterIdleHook: Some(idle),
	AEGP_RegisterTracker: Some(tracker),
	AEGP_RegisterInteractiveArtisan: Some(artisan),
	AEGP_RegisterPresetLocalizationString: Some(preset),
};

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicUsize, Ordering};
	static SEEN: AtomicUsize = AtomicUsize::new(0);
	unsafe extern "C" fn count(global: AEGP_GlobalRefcon, reference: AEGP_IdleRefcon, _: *mut A_long) -> A_Err {
		SEEN.fetch_add(global as usize + reference as usize, Ordering::SeqCst);
		0
	}
	#[test]
	fn idle_hooks_are_owned_and_removed() {
		let owner = new_owner();
		let other = new_owner();
		let id = -24567;
		SEEN.store(0, Ordering::SeqCst);
		assert!(!register_plugin(id, 2usize as AEGP_GlobalRefcon));
		{
			let _scope = enter_owner(owner);
			assert!(register_plugin(id, 2usize as AEGP_GlobalRefcon));
			assert_eq!(unsafe { idle(id, Some(count), 3usize as AEGP_IdleRefcon) }, 0);
		}
		dispatch_idle(other);
		assert_eq!(SEEN.load(Ordering::SeqCst), 0);
		dispatch_idle(owner);
		assert_eq!(SEEN.load(Ordering::SeqCst), 5);
		remove_owner(owner);
		dispatch_idle(owner);
		assert_eq!(SEEN.load(Ordering::SeqCst), 5);
		let _scope = enter_owner(other);
		assert_ne!(unsafe { idle(id, Some(count), std::ptr::null_mut()) }, 0);
	}
}
