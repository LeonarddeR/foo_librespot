//! Fakes of foobar2000's core services that every [`FakeApi`](super::FakeApi) registers:
//! popups and questions, the settings store, the main thread, the modeless window list and the player.

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use super::FakeApi;
use super::contextmenu::{FakeFields, fake_handle};
use super::playlists::{FakeIncoming, FakeManager};
use crate::abi::config_store::{self, ConfigStoreVtbl};
use crate::abi::input::ByValueServicePtr;
use crate::abi::main_thread::{
	MANAGER_IID, MODELESS_IID, MainThreadCallbackManagerVtbl, MainThreadCallbackVtbl,
	ModelessDialogManagerVtbl,
};
use crate::abi::playback_control::{self, PlaybackControlVtbl};
use crate::abi::playlist;
use crate::abi::popup::{self, BUTTON_NO, BUTTON_YES, PopupMessageVtbl, PopupV3Vtbl, Query};
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
	questions: Mutex<Vec<(String, String, u32)>>,
	answer_yes: AtomicBool,
	config: Mutex<HashMap<String, i64>>,
	main_thread: Mutex<Vec<ServicePtr>>,
	modeless: Mutex<Vec<isize>>,
	pub(super) playlists: super::playlists::PlaylistsState,
	playback_stops: AtomicUsize,
	now_playing: Mutex<Option<ServicePtr>>,
}

pub(super) fn register(api: &FakeApi, state: &Arc<FakeState>) {
	api.add_service(popup::IID, new_service(FakePopup(Arc::clone(state))));
	api.add_service(popup::V3_IID, new_service(FakePopupV3(Arc::clone(state))));
	api.add_service(config_store::IID, new_service(FakeConfigStore(Arc::clone(state))));
	api.add_service(MANAGER_IID, new_service(FakeMainThread(Arc::clone(state))));
	api.add_service(MODELESS_IID, new_service(FakeModeless(Arc::clone(state))));
	api.add_service(playback_control::IID, new_service(FakePlayback(Arc::clone(state))));
	api.add_service(playlist::MANAGER_IID, new_service(FakeManager(Arc::clone(state))));
	api.add_service(playlist::INCOMING_IID, new_service(FakeIncoming(Arc::clone(state))));
}

impl FakeApi {
	/// Every popup shown so far, as (title, message, icon).
	pub fn popups(&self) -> Vec<(String, String, PopupIcon)> {
		self.state.popups.lock().unwrap_or_else(PoisonError::into_inner).clone()
	}

	/// Makes later yes/no questions come back answered Yes or No; the default answer is No.
	pub fn answer_questions(&self, yes: bool) {
		self.state.answer_yes.store(yes, Ordering::SeqCst);
	}

	/// Every yes/no question asked so far, as (title, message, default button).
	pub fn questions(&self) -> Vec<(String, String, u32)> {
		self.state.questions.lock().unwrap_or_else(PoisonError::into_inner).clone()
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

	/// How many times playback was stopped.
	pub fn playback_stops(&self) -> usize {
		self.state.playback_stops.load(Ordering::SeqCst)
	}

	/// Makes `track`, a path and the fields its info holds, the playing track; `None` means nothing
	/// plays.
	pub fn set_now_playing(&self, track: Option<(&str, FakeFields<'_>)>) {
		let handle = track.map(|(path, fields)| fake_handle(path, Some(fields)));
		*self.state.now_playing.lock().unwrap_or_else(PoisonError::into_inner) = handle;
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

struct FakePopupV3(Arc<FakeState>);

unsafe impl ServiceImpl for FakePopupV3 {
	type Vtbl = PopupV3Vtbl;
	const IIDS: &'static [Guid] = &[popup::V3_IID];
	const VTBL: &'static PopupV3Vtbl = &PopupV3Vtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		unused_3: [None; 1],
		show_query_modal: popup_query_modal,
	};
}

unsafe extern "C-unwind" fn popup_query_modal(this: *mut ServiceBase, query: *const Query) -> u32 {
	let state = &unsafe { ServiceObject::<FakePopupV3>::value(this) }.0;
	let query = unsafe { &*query };
	let cstr =
		|pointer: *const c_char| unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned();
	state.questions.lock().unwrap_or_else(PoisonError::into_inner).push((
		cstr(query.title),
		cstr(query.message),
		query.default_button,
	));
	if state.answer_yes.load(Ordering::SeqCst) { BUTTON_YES } else { BUTTON_NO }
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
	// Passed by value: the reference is ours to keep, as a callee's.
	if let Some(kept) = callback.into_ptr() {
		state.main_thread.lock().unwrap_or_else(PoisonError::into_inner).push(kept);
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

struct FakePlayback(Arc<FakeState>);

unsafe impl ServiceImpl for FakePlayback {
	type Vtbl = PlaybackControlVtbl;
	const IIDS: &'static [Guid] = &[playback_control::IID];
	const VTBL: &'static PlaybackControlVtbl = &PlaybackControlVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_now_playing: playback_get_now_playing,
		start: playback_start,
		stop: playback_stop,
	};
}

/// Copy assignment, as `service_ptr_t`'s is: `out` takes a reference to the playing track and
/// releases the one it held. With nothing playing, `out` is left as it was.
unsafe extern "C-unwind" fn playback_get_now_playing(
	this: *mut ServiceBase,
	out: *mut *mut ServiceBase,
) -> bool {
	let state = &unsafe { ServiceObject::<FakePlayback>::value(this) }.0;
	let Some(playing) = state.now_playing.lock().unwrap_or_else(PoisonError::into_inner).clone()
	else {
		return false;
	};
	unsafe {
		drop(ServicePtr::from_raw_owned(*out));
		*out = playing.into_raw();
	}
	true
}

unsafe extern "C-unwind" fn playback_start(_this: *mut ServiceBase, _command: i32, _paused: bool) {}

unsafe extern "C-unwind" fn playback_stop(this: *mut ServiceBase) {
	let state = &unsafe { ServiceObject::<FakePlayback>::value(this) }.0;
	state.playback_stops.fetch_add(1, Ordering::SeqCst);
}
