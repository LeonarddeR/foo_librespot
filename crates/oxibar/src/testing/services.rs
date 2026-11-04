//! Fakes of foobar2000's core services that every [`FakeApi`](super::FakeApi) registers:
//! popups, the settings store, the main thread and the modeless window list.

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::sync::{Arc, Mutex, PoisonError};

use super::FakeApi;
use crate::abi::config_store::{self, ConfigStoreVtbl};
use crate::abi::input::ByValueServicePtr;
use crate::abi::main_thread::{
	MANAGER_IID, MODELESS_IID, MainThreadCallbackManagerVtbl, MainThreadCallbackVtbl,
	ModelessDialogManagerVtbl,
};
use crate::abi::popup::{self, PopupMessageVtbl};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::cxx::try_host;
use crate::guid::Guid;
use crate::popup::PopupIcon;
use crate::service::{ServiceImpl, ServiceObject, ServicePtr, new_service};
use crate::strings::bytes_of;

/// What the fake core services recorded.
#[derive(Default)]
pub(super) struct FakeState {
	popups: Mutex<Vec<(String, String, PopupIcon)>>,
	config: Mutex<HashMap<String, i64>>,
	main_thread: Mutex<Vec<ServicePtr>>,
	modeless: Mutex<Vec<isize>>,
}

pub(super) fn register(api: &FakeApi, state: &Arc<FakeState>) {
	api.add_service(popup::IID, new_service(FakePopup(Arc::clone(state))));
	api.add_service(config_store::IID, new_service(FakeConfigStore(Arc::clone(state))));
	api.add_service(MANAGER_IID, new_service(FakeMainThread(Arc::clone(state))));
	api.add_service(MODELESS_IID, new_service(FakeModeless(Arc::clone(state))));
}

impl FakeApi {
	/// Every popup shown so far, as (title, message, icon).
	pub fn popups(&self) -> Vec<(String, String, PopupIcon)> {
		self.state.popups.lock().unwrap_or_else(PoisonError::into_inner).clone()
	}

	/// The integer stored under `name` in the fake settings store.
	pub fn config_int(&self, name: &str) -> Option<i64> {
		self.state.config.lock().unwrap_or_else(PoisonError::into_inner).get(name).copied()
	}

	/// Runs the main-thread callbacks queued so far, on the calling thread, as foobar2000's main
	/// thread would, and returns how many ran. Callbacks they queue wait for the next call.
	pub fn run_main_thread(&self) -> usize {
		let pending = std::mem::take(
			&mut *self.state.main_thread.lock().unwrap_or_else(PoisonError::into_inner),
		);
		for callback in &pending {
			let raw = callback.as_raw();
			try_host(|| unsafe {
				let vtbl = &*(*raw).vtbl.cast::<MainThreadCallbackVtbl>();
				(vtbl.callback_run)(raw);
			})
			.expect("callback_run threw");
		}
		pending.len()
	}

	/// Drops the queued main-thread callbacks without running them, as foobar2000 does when it
	/// quits.
	pub fn discard_main_thread(&self) {
		drop(std::mem::take(
			&mut *self.state.main_thread.lock().unwrap_or_else(PoisonError::into_inner),
		));
	}

	/// The windows registered with the modeless dialog manager.
	pub fn modeless_windows(&self) -> Vec<isize> {
		self.state.modeless.lock().unwrap_or_else(PoisonError::into_inner).clone()
	}
}

fn text(pointer: *const c_char, length: usize) -> String {
	String::from_utf8_lossy(unsafe { bytes_of(pointer, length) }).into_owned()
}

struct FakePopup(Arc<FakeState>);

unsafe impl ServiceImpl for FakePopup {
	type Vtbl = PopupMessageVtbl;
	const IIDS: &'static [Guid] = &[popup::IID];
	const VTBL: &'static PopupMessageVtbl =
		&PopupMessageVtbl { base: ServiceBaseVtbl::new::<Self>(), show_ex: popup_show };
}

unsafe extern "C-unwind" fn popup_show(
	this: *mut ServiceBase,
	message: *const c_char,
	message_length: usize,
	title: *const c_char,
	title_length: usize,
	icon: u32,
) {
	let state = &unsafe { ServiceObject::<FakePopup>::value(this) }.0;
	state.popups.lock().unwrap_or_else(PoisonError::into_inner).push((
		text(title, title_length),
		text(message, message_length),
		PopupIcon::from_raw(icon),
	));
}

struct FakeConfigStore(Arc<FakeState>);

unsafe impl ServiceImpl for FakeConfigStore {
	type Vtbl = ConfigStoreVtbl;
	const IIDS: &'static [Guid] = &[config_store::IID];
	const VTBL: &'static ConfigStoreVtbl = &ConfigStoreVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		acquire_transaction_scope: config_transaction,
		commit_blocking: config_commit,
		get_config_int: config_get_int,
		set_config_int: config_set_int,
	};
}

unsafe extern "C-unwind" fn config_transaction(
	_this: *mut ServiceBase,
	ret: *mut *mut ServiceBase,
) -> *mut *mut ServiceBase {
	unsafe { ret.write(std::ptr::null_mut()) };
	ret
}

unsafe extern "C-unwind" fn config_commit(_this: *mut ServiceBase) {}

fn config_name(name: *const c_char) -> String {
	unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned()
}

unsafe extern "C-unwind" fn config_get_int(
	this: *mut ServiceBase,
	name: *const c_char,
	default: i64,
) -> i64 {
	let state = &unsafe { ServiceObject::<FakeConfigStore>::value(this) }.0;
	let config = state.config.lock().unwrap_or_else(PoisonError::into_inner);
	config.get(&config_name(name)).copied().unwrap_or(default)
}

unsafe extern "C-unwind" fn config_set_int(
	this: *mut ServiceBase,
	name: *const c_char,
	value: i64,
) {
	let state = &unsafe { ServiceObject::<FakeConfigStore>::value(this) }.0;
	state.config.lock().unwrap_or_else(PoisonError::into_inner).insert(config_name(name), value);
}

struct FakeMainThread(Arc<FakeState>);

unsafe impl ServiceImpl for FakeMainThread {
	type Vtbl = MainThreadCallbackManagerVtbl;
	const IIDS: &'static [Guid] = &[MANAGER_IID];
	const VTBL: &'static MainThreadCallbackManagerVtbl = &MainThreadCallbackManagerVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		add_callback: main_thread_add,
	};
}

unsafe extern "C-unwind" fn main_thread_add(this: *mut ServiceBase, callback: ByValueServicePtr) {
	let state = &unsafe { ServiceObject::<FakeMainThread>::value(this) }.0;
	unsafe {
		// Keep a reference of our own, then destroy the caller's temporary, as a callee must.
		if let Some(kept) = ServicePtr::from_raw_add_ref(*callback) {
			state.main_thread.lock().unwrap_or_else(PoisonError::into_inner).push(kept);
		}
		drop(ServicePtr::from_raw_owned(*callback));
	}
}

struct FakeModeless(Arc<FakeState>);

unsafe impl ServiceImpl for FakeModeless {
	type Vtbl = ModelessDialogManagerVtbl;
	const IIDS: &'static [Guid] = &[MODELESS_IID];
	const VTBL: &'static ModelessDialogManagerVtbl = &ModelessDialogManagerVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		add: modeless_add,
		remove: modeless_remove,
	};
}

unsafe extern "C-unwind" fn modeless_add(this: *mut ServiceBase, window: *mut c_void) {
	let state = &unsafe { ServiceObject::<FakeModeless>::value(this) }.0;
	state.modeless.lock().unwrap_or_else(PoisonError::into_inner).push(window as isize);
}

unsafe extern "C-unwind" fn modeless_remove(this: *mut ServiceBase, window: *mut c_void) {
	let state = &unsafe { ServiceObject::<FakeModeless>::value(this) }.0;
	state.modeless.lock().unwrap_or_else(PoisonError::into_inner).retain(|&w| w != window as isize);
}
