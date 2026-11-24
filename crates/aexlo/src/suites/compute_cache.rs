//! Compute cache with per-key single-flight and checked-out value lifetimes.
//! Entries remain resident until their class is unregistered; pressure-based
//! eviction is not yet implemented.
use std::collections::{HashMap, HashSet};
use std::ffi::CStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};

use crate::sys::*;

type Key = [A_long; 4];
struct Value {
	ptr: usize,
	delete: unsafe extern "C" fn(AEGP_CCComputeValueRefconP),
}
impl Drop for Value {
	fn drop(&mut self) {
		// The class is purged before its owner's plugin library is unloaded.
		unsafe { (self.delete)(self.ptr as AEGP_CCComputeValueRefconP) };
	}
}
enum Slot {
	Computing,
	Ready(Arc<Value>),
}
#[derive(Default)]
struct State {
	closing: bool,
	active: usize,
	entries: HashMap<Key, Slot>,
}
struct Class {
	owners: Mutex<HashSet<u64>>,
	callbacks: AEGP_ComputeCacheCallbacks,
	state: Mutex<State>,
	changed: Condvar,
}
static CLASSES: LazyLock<Mutex<HashMap<Vec<u8>, Arc<Class>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// A checked-out value by its receipt, with the class it came from.
type Receipts = HashMap<usize, (Arc<Class>, Arc<Value>)>;
static RECEIPTS: LazyLock<Mutex<Receipts>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_RECEIPT: AtomicUsize = AtomicUsize::new(1);

struct Operation(Arc<Class>);
impl Drop for Operation {
	fn drop(&mut self) {
		let mut state = self.0.state.lock().unwrap();
		state.active -= 1;
		self.0.changed.notify_all();
	}
}
unsafe fn class_name(ptr: AEGP_CCComputeClassIdP) -> Option<Vec<u8>> {
	if ptr.is_null() {
		return None;
	}
	// SDK callers provide a NUL-terminated class ID.
	Some(unsafe { CStr::from_ptr(ptr) }.to_bytes().to_vec())
}
unsafe fn begin(id: AEGP_CCComputeClassIdP) -> Result<Operation, A_Err> {
	let name = unsafe { class_name(id) }.ok_or(PF_Err_BAD_CALLBACK_PARAM)?;
	let class = CLASSES
		.lock()
		.unwrap()
		.get(&name)
		.cloned()
		.ok_or(PF_Err_BAD_CALLBACK_PARAM)?;
	{
		let mut state = class.state.lock().unwrap();
		if state.closing {
			return Err(PF_Err_BAD_CALLBACK_PARAM);
		}
		state.active += 1;
	}
	Ok(Operation(class))
}
unsafe extern "C" fn register(id: AEGP_CCComputeClassIdP, callbacks: *const AEGP_ComputeCacheCallbacks) -> A_Err {
	crate::core::ffi::guard("compute_class_register", || {
		let Some(name) = (unsafe { class_name(id) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(owner) = super::register::current_owner() else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if name.is_empty() || callbacks.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let cb = unsafe { *callbacks };
		if cb.generate_key.is_none()
			|| cb.compute.is_none()
			|| cb.approx_size_value.is_none()
			|| cb.delete_compute_value.is_none()
		{
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let mut classes = CLASSES.lock().unwrap();
		if let Some(class) = classes.get(&name) {
			let old = class.callbacks;
			if old.generate_key.map(|f| f as usize) != cb.generate_key.map(|f| f as usize)
				|| old.compute.map(|f| f as usize) != cb.compute.map(|f| f as usize)
				|| old.approx_size_value.map(|f| f as usize) != cb.approx_size_value.map(|f| f as usize)
				|| old.delete_compute_value.map(|f| f as usize) != cb.delete_compute_value.map(|f| f as usize)
			{
				return PF_Err_BAD_CALLBACK_PARAM;
			}
			return if class.owners.lock().unwrap().insert(owner) {
				PF_Err_NONE
			} else {
				PF_Err_BAD_CALLBACK_PARAM
			};
		}
		classes.insert(
			name,
			Arc::new(Class {
				owners: Mutex::new(HashSet::from([owner])),
				callbacks: cb,
				state: Mutex::new(State::default()),
				changed: Condvar::new(),
			}),
		);
		PF_Err_NONE
	})
}
fn purge(class: Arc<Class>) {
	let mut state = class.state.lock().unwrap();
	state.closing = true;
	class.changed.notify_all();
	while state.active != 0 {
		state = class.changed.wait(state).unwrap();
	}
	let entries = std::mem::take(&mut state.entries);
	drop(state);
	let removed = {
		let mut receipts = RECEIPTS.lock().unwrap();
		let ids: Vec<_> = receipts
			.iter()
			.filter_map(|(id, (c, _))| Arc::ptr_eq(c, &class).then_some(*id))
			.collect();
		ids.into_iter()
			.filter_map(|id| receipts.remove(&id))
			.collect::<Vec<_>>()
	};
	drop(removed);
	drop(entries);
}
unsafe extern "C" fn unregister(id: AEGP_CCComputeClassIdP) -> A_Err {
	crate::core::ffi::guard("compute_class_unregister", || {
		let Some(name) = (unsafe { class_name(id) }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let Some(owner) = super::register::current_owner() else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		let class = {
			let mut classes = CLASSES.lock().unwrap();
			let Some(class) = classes.get(&name) else {
				return PF_Err_BAD_CALLBACK_PARAM;
			};
			let mut owners = class.owners.lock().unwrap();
			if !owners.remove(&owner) {
				return PF_Err_BAD_CALLBACK_PARAM;
			}
			let empty = owners.is_empty();
			drop(owners);
			if empty { classes.remove(&name) } else { None }
		};
		if let Some(class) = class {
			purge(class);
		}
		PF_Err_NONE
	})
}
/// Remove plugin callbacks while its library is still loaded, even after failed setup.
pub(crate) fn remove_owner(owner: u64) {
	let classes = {
		let mut registry = CLASSES.lock().unwrap();
		let names: Vec<_> = registry
			.iter()
			.filter_map(|(name, class)| {
				let mut owners = class.owners.lock().unwrap();
				(owners.remove(&owner) && owners.is_empty()).then_some(name.clone())
			})
			.collect();
		names
			.into_iter()
			.filter_map(|name| registry.remove(&name))
			.collect::<Vec<_>>()
	};
	for class in classes {
		purge(class);
	}
}
unsafe fn key(class: &Class, options: AEGP_CCComputeOptionsRefconP) -> Result<Key, A_Err> {
	let mut key = AEGP_GUID { bytes: [0; 4] };
	let err = unsafe { (class.callbacks.generate_key.unwrap())(options, &mut key) };
	if err == 0 { Ok(key.bytes) } else { Err(err) }
}
fn receipt(class: Arc<Class>, value: Arc<Value>, out: *mut AEGP_CCCheckoutReceiptP) -> A_Err {
	let id = NEXT_RECEIPT.fetch_add(1, Ordering::Relaxed);
	if id == 0 {
		return PF_Err_INTERNAL_STRUCT_DAMAGED;
	}
	RECEIPTS.lock().unwrap().insert(id, (class, value));
	unsafe {
		*out = id as AEGP_CCCheckoutReceiptP;
	}
	PF_Err_NONE
}
unsafe fn checkout(
	id: AEGP_CCComputeClassIdP,
	options: AEGP_CCComputeOptionsRefconP,
	wait: bool,
	compute: bool,
	out: *mut AEGP_CCCheckoutReceiptP,
) -> A_Err {
	if out.is_null() {
		return PF_Err_BAD_CALLBACK_PARAM;
	}
	unsafe {
		*out = std::ptr::null_mut();
	}
	let op = match unsafe { begin(id) } {
		Ok(op) => op,
		Err(err) => return err,
	};
	let class = &op.0;
	let key = match unsafe { key(class, options) } {
		Ok(key) => key,
		Err(err) => return err,
	};
	let mut state = class.state.lock().unwrap();
	loop {
		if state.closing {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		match state.entries.get(&key) {
			Some(Slot::Ready(value)) => {
				let value = value.clone();
				drop(state);
				return receipt(class.clone(), value, out);
			}
			Some(Slot::Computing) if compute && wait => state = class.changed.wait(state).unwrap(),
			Some(Slot::Computing) | None if !compute || !wait && state.entries.contains_key(&key) => {
				return A_Err_NOT_IN_CACHE_OR_COMPUTE_PENDING as A_Err;
			}
			None => {
				state.entries.insert(key, Slot::Computing);
				drop(state);
				let mut value = std::ptr::null_mut();
				let err = unsafe { (class.callbacks.compute.unwrap())(options, &mut value) };
				state = class.state.lock().unwrap();
				if err != 0 {
					state.entries.remove(&key);
					class.changed.notify_all();
					return err;
				}
				let value = Arc::new(Value {
					ptr: value as usize,
					delete: class.callbacks.delete_compute_value.unwrap(),
				});
				state.entries.insert(key, Slot::Ready(value.clone()));
				class.changed.notify_all();
				drop(state);
				return receipt(class.clone(), value, out);
			}
			Some(Slot::Computing) => return A_Err_NOT_IN_CACHE_OR_COMPUTE_PENDING as A_Err,
		}
	}
}
unsafe extern "C" fn compute(
	id: AEGP_CCComputeClassIdP,
	options: AEGP_CCComputeOptionsRefconP,
	wait: bool,
	out: *mut AEGP_CCCheckoutReceiptP,
) -> A_Err {
	crate::core::ffi::guard("compute_if_needed", || unsafe {
		checkout(id, options, wait, true, out)
	})
}
unsafe extern "C" fn cached(
	id: AEGP_CCComputeClassIdP,
	options: AEGP_CCComputeOptionsRefconP,
	out: *mut AEGP_CCCheckoutReceiptP,
) -> A_Err {
	crate::core::ffi::guard("checkout_cached", || unsafe {
		checkout(id, options, false, false, out)
	})
}
unsafe extern "C" fn get(receipt: AEGP_CCCheckoutReceiptP, out: *mut AEGP_CCComputeValueRefconP) -> A_Err {
	crate::core::ffi::guard("get_receipt_compute_value", || {
		if out.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		unsafe {
			*out = std::ptr::null_mut();
		}
		let receipts = RECEIPTS.lock().unwrap();
		let Some((_, value)) = receipts.get(&(receipt as usize)) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		unsafe {
			*out = value.ptr as AEGP_CCComputeValueRefconP;
		}
		PF_Err_NONE
	})
}
unsafe extern "C" fn checkin(receipt: AEGP_CCCheckoutReceiptP) -> A_Err {
	crate::core::ffi::guard("checkin_compute_receipt", || {
		let removed = RECEIPTS.lock().unwrap().remove(&(receipt as usize));
		if removed.is_some() {
			PF_Err_NONE
		} else {
			PF_Err_BAD_CALLBACK_PARAM
		}
	})
}
pub(crate) static SUITE: AEGP_ComputeCacheSuite1 = AEGP_ComputeCacheSuite1 {
	AEGP_ClassRegister: Some(register),
	AEGP_ClassUnregister: Some(unregister),
	AEGP_ComputeIfNeededAndCheckout: Some(compute),
	AEGP_CheckoutCached: Some(cached),
	AEGP_GetReceiptComputeValue: Some(get),
	AEGP_CheckinComputeReceipt: Some(checkin),
};

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
	use std::time::Duration;

	static STARTED: AtomicBool = AtomicBool::new(false);
	static RELEASE: AtomicBool = AtomicBool::new(false);
	static COMPUTED: AtomicUsize = AtomicUsize::new(0);
	static DELETED: AtomicUsize = AtomicUsize::new(0);
	const ID: &[u8] = b"aexlo.test.compute-cache\0";
	unsafe extern "C" fn generate(options: AEGP_CCComputeOptionsRefconP, out: AEGP_CCComputeKeyP) -> A_Err {
		unsafe { (*out).bytes = [options as usize as i32, 0, 0, 0] };
		0
	}
	unsafe extern "C" fn calculate(
		options: AEGP_CCComputeOptionsRefconP,
		out: *mut AEGP_CCComputeValueRefconP,
	) -> A_Err {
		COMPUTED.fetch_add(1, Ordering::SeqCst);
		STARTED.store(true, Ordering::SeqCst);
		while !RELEASE.load(Ordering::SeqCst) {
			std::thread::yield_now();
		}
		unsafe { *out = Box::into_raw(Box::new(options as usize)).cast() };
		0
	}
	unsafe extern "C" fn size(_: AEGP_CCComputeValueRefconP) -> usize {
		std::mem::size_of::<usize>()
	}
	unsafe extern "C" fn delete(value: AEGP_CCComputeValueRefconP) {
		DELETED.fetch_add(1, Ordering::SeqCst);
		drop(unsafe { Box::from_raw(value.cast::<usize>()) });
	}

	#[test]
	fn single_flight_receipts_and_owner_cleanup() {
		let owner = super::super::register::new_owner();
		let _scope = super::super::register::enter_owner(owner);
		let callbacks = AEGP_ComputeCacheCallbacks {
			generate_key: Some(generate),
			compute: Some(calculate),
			approx_size_value: Some(size),
			delete_compute_value: Some(delete),
		};
		let id = ID.as_ptr().cast();
		assert_eq!(unsafe { register(id, &callbacks) }, 0);
		let other_owner = super::super::register::new_owner();
		{
			let _other_scope = super::super::register::enter_owner(other_owner);
			assert_eq!(unsafe { register(id, &callbacks) }, 0);
		}
		let worker = std::thread::spawn(|| {
			let mut receipt = std::ptr::null_mut();
			let result = unsafe { compute(ID.as_ptr().cast(), 7usize as _, true, &mut receipt) };
			(result, receipt as usize)
		});
		while !STARTED.load(Ordering::SeqCst) {
			std::thread::sleep(Duration::from_millis(1));
		}
		let mut pending = std::ptr::null_mut();
		assert_eq!(
			unsafe { compute(id, 7usize as _, false, &mut pending) },
			A_Err_NOT_IN_CACHE_OR_COMPUTE_PENDING as A_Err
		);
		let waiter = std::thread::spawn(|| {
			let mut receipt = std::ptr::null_mut();
			let result = unsafe { compute(ID.as_ptr().cast(), 7usize as _, true, &mut receipt) };
			(result, receipt as usize)
		});
		let class = CLASSES.lock().unwrap().get(&ID[..ID.len() - 1]).unwrap().clone();
		while class.state.lock().unwrap().active < 2 {
			std::thread::sleep(Duration::from_millis(1));
		}
		RELEASE.store(true, Ordering::SeqCst);
		let (result, first) = worker.join().unwrap();
		assert_eq!(result, 0);
		let (result, waited) = waiter.join().unwrap();
		assert_eq!(result, 0);
		let mut second = std::ptr::null_mut();
		assert_eq!(unsafe { cached(id, 7usize as _, &mut second) }, 0);
		assert_eq!(COMPUTED.load(Ordering::SeqCst), 1);
		let mut value = std::ptr::null_mut();
		assert_eq!(unsafe { get(second, &mut value) }, 0);
		assert_eq!(unsafe { *value.cast::<usize>() }, 7);
		assert_eq!(unsafe { checkin(first as _) }, 0);
		assert_eq!(unsafe { checkin(waited as _) }, 0);
		assert_eq!(DELETED.load(Ordering::SeqCst), 0);
		remove_owner(owner);
		assert_eq!(DELETED.load(Ordering::SeqCst), 0);
		remove_owner(other_owner);
		assert_eq!(DELETED.load(Ordering::SeqCst), 1);
		assert_ne!(unsafe { get(second, &mut value) }, 0);
		super::super::register::remove_owner(owner);
		super::super::register::remove_owner(other_owner);
	}
}
