//! Fakes of foobar2000's playlist manager and incoming item filter: tabs that keep paths and
//! properties and can be locked against adding, and location loads that finish when a test says
//! so.

use std::ffi::{c_char, c_void};
use std::sync::{Mutex, PoisonError};

use super::FakeApi;
use crate::abi::abort_callback::AbortCallback;
use crate::abi::file::{StreamReaderVtbl, StreamWriterVtbl};
use crate::abi::input::ByValueServicePtr;
use crate::abi::metadb_handle::HandleListRaw;
use crate::abi::pfc_list::{BitArray, ListBaseConst};
use crate::abi::playlist::{
	FILTER_ADD, INCOMING_IID, INCOMING_V2_IID, IncomingItemFilterV2Vtbl, MANAGER_IID,
	MANAGER_V2_IID, PlaylistManagerVtbl, ProcessLocationsNotifyVtbl,
};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::cxx::{HostException, try_host};
use crate::guid::Guid;
use crate::metadb::HandleList;
use crate::service::{ServiceImpl, ServiceObject, ServicePtr};
use crate::strings::{bytes_of, set_string};
use crate::testing::FakeHandleList;

/// One playlist tab of the fake manager.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FakeTab {
	pub name: String,
	pub paths: Vec<String>,
	/// How many of the items inserted were selected.
	pub selected: usize,
	pub properties: Vec<(Guid, Vec<u8>)>,
	/// Whether a lock refuses items being added (`filter_add`).
	pub locked: bool,
}

impl FakeTab {
	fn new(name: String) -> Self {
		Self { name, ..Self::default() }
	}
}

/// A `process_locations_async` call, finished or not.
struct Request {
	locations: Vec<String>,
	flags: u32,
	parent: isize,
	notify: Option<ServicePtr>,
}

/// What the fake playlist services recorded.
#[derive(Default)]
pub(super) struct PlaylistsState {
	tabs: Mutex<Vec<FakeTab>>,
	active: Mutex<Option<usize>>,
	requests: Mutex<Vec<Request>>,
}

impl FakeApi {
	/// Adds a tab after the others and returns its index.
	pub fn add_tab(&self, name: &str) -> usize {
		let mut tabs = self.state.playlists.tabs.lock().unwrap_or_else(PoisonError::into_inner);
		tabs.push(FakeTab::new(name.to_owned()));
		tabs.len() - 1
	}

	pub fn tabs(&self) -> Vec<FakeTab> {
		self.state.playlists.tabs.lock().unwrap_or_else(PoisonError::into_inner).clone()
	}

	pub fn active_tab(&self) -> Option<usize> {
		*self.state.playlists.active.lock().unwrap_or_else(PoisonError::into_inner)
	}

	/// Makes tab `index` the active one, or no tab with `None`, as the user switching tabs does.
	pub fn set_active_tab(&self, index: Option<usize>) {
		*self.state.playlists.active.lock().unwrap_or_else(PoisonError::into_inner) = index;
	}

	/// Locks tab `index` against adding: its filter mask has `filter_add`, and inserting into it
	/// answers infinite and inserts nothing.
	pub fn lock_tab(&self, index: usize) {
		self.state.playlists.tabs.lock().unwrap_or_else(PoisonError::into_inner)[index].locked =
			true;
	}

	/// Removes tab `index`, as the user closing it does.
	pub fn remove_tab(&self, index: usize) {
		self.state.playlists.tabs.lock().unwrap_or_else(PoisonError::into_inner).remove(index);
		let mut active = self.state.playlists.active.lock().unwrap_or_else(PoisonError::into_inner);
		*active = match *active {
			Some(current) if current == index => None,
			Some(current) if current > index => Some(current - 1),
			other => other,
		};
	}

	/// The links of every `process_locations_async` call so far.
	pub fn location_requests(&self) -> Vec<Vec<String>> {
		let requests = self.state.playlists.requests.lock().unwrap_or_else(PoisonError::into_inner);
		requests.iter().map(|request| request.locations.clone()).collect()
	}

	/// The flags and parent window of every `process_locations_async` call so far.
	pub fn location_flags(&self) -> Vec<(u32, isize)> {
		let requests = self.state.playlists.requests.lock().unwrap_or_else(PoisonError::into_inner);
		requests.iter().map(|request| (request.flags, request.parent)).collect()
	}

	fn take_notify(&self, request: usize) -> ServicePtr {
		let mut requests =
			self.state.playlists.requests.lock().unwrap_or_else(PoisonError::into_inner);
		requests[request].notify.take().expect("the request already finished")
	}

	/// Finishes request `request` with tracks at `paths`: runs `on_completion`.
	pub fn finish_locations(&self, request: usize, paths: &[&str]) -> Result<(), HostException> {
		let notify = self.take_notify(request);
		let list = FakeHandleList::new(paths);
		let raw = notify.as_raw();
		try_host(|| unsafe {
			let vtbl = &*(*raw).vtbl.cast::<ProcessLocationsNotifyVtbl>();
			(vtbl.on_completion)(raw, list.as_raw());
		})
	}

	/// Cancels request `request`: runs `on_aborted`.
	pub fn cancel_locations(&self, request: usize) {
		let notify = self.take_notify(request);
		let raw = notify.as_raw();
		try_host(|| unsafe {
			let vtbl = &*(*raw).vtbl.cast::<ProcessLocationsNotifyVtbl>();
			(vtbl.on_aborted)(raw);
		})
		.expect("on_aborted threw");
	}
}

pub(super) struct FakeManager(pub(super) std::sync::Arc<super::services::FakeState>);

unsafe impl ServiceImpl for FakeManager {
	type Vtbl = PlaylistManagerVtbl;
	const IIDS: &'static [Guid] = &[MANAGER_IID, MANAGER_V2_IID];
	const VTBL: &'static PlaylistManagerVtbl = &PlaylistManagerVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_playlist_count: manager_count,
		get_active_playlist: manager_get_active,
		set_active_playlist: manager_set_active,
		unused_6_8: [None; 3],
		create_playlist: manager_create,
		unused_10_13: [None; 4],
		playlist_get_name: manager_get_name,
		unused_15_19: [None; 5],
		playlist_insert_items: manager_insert_items,
		unused_21_37: [None; 17],
		playlist_lock_get_filter_mask: manager_filter_mask,
		unused_39_56: [None; 18],
		playlist_set_property: manager_set_property,
		playlist_get_property: manager_get_property,
	};
}

fn state<'a>(this: *mut ServiceBase) -> &'a PlaylistsState {
	&unsafe { ServiceObject::<FakeManager>::value(this) }.0.playlists
}

unsafe extern "C-unwind" fn manager_count(this: *mut ServiceBase) -> usize {
	state(this).tabs.lock().unwrap_or_else(PoisonError::into_inner).len()
}

unsafe extern "C-unwind" fn manager_get_active(this: *mut ServiceBase) -> usize {
	state(this).active.lock().unwrap_or_else(PoisonError::into_inner).unwrap_or(usize::MAX)
}

unsafe extern "C-unwind" fn manager_set_active(this: *mut ServiceBase, index: usize) {
	let state = state(this);
	let exists = index < state.tabs.lock().unwrap_or_else(PoisonError::into_inner).len();
	*state.active.lock().unwrap_or_else(PoisonError::into_inner) = exists.then_some(index);
}

unsafe extern "C-unwind" fn manager_create(
	this: *mut ServiceBase,
	name: *const c_char,
	length: usize,
	index: usize,
) -> usize {
	let name = String::from_utf8_lossy(unsafe { bytes_of(name, length) }).into_owned();
	let mut tabs = state(this).tabs.lock().unwrap_or_else(PoisonError::into_inner);
	let index = index.min(tabs.len());
	tabs.insert(index, FakeTab::new(name));
	index
}

unsafe extern "C-unwind" fn manager_get_name(
	this: *mut ServiceBase,
	playlist: usize,
	out: *mut StringBase,
) -> bool {
	let Some(name) = state(this)
		.tabs
		.lock()
		.unwrap_or_else(PoisonError::into_inner)
		.get(playlist)
		.map(|tab| tab.name.clone())
	else {
		return false;
	};
	// The caller's string takes any text.
	let _ = unsafe { set_string(out, &name) };
	true
}

unsafe extern "C-unwind" fn manager_filter_mask(this: *mut ServiceBase, playlist: usize) -> u32 {
	let tabs = state(this).tabs.lock().unwrap_or_else(PoisonError::into_inner);
	if tabs.get(playlist).is_some_and(|tab| tab.locked) { FILTER_ADD } else { 0 }
}

unsafe extern "C-unwind" fn manager_insert_items(
	this: *mut ServiceBase,
	playlist: usize,
	base: usize,
	data: *const HandleListRaw,
	selection: *const BitArray,
) -> usize {
	let paths = unsafe { HandleList::from_raw(data) }.paths().expect("the handles' paths");
	let selected =
		(0..paths.len()).filter(|&n| unsafe { ((*(*selection).vtbl).get)(selection, n) }).count();
	let mut tabs = state(this).tabs.lock().unwrap_or_else(PoisonError::into_inner);
	let Some(tab) = tabs.get_mut(playlist).filter(|tab| !tab.locked) else { return usize::MAX };
	let base = base.min(tab.paths.len());
	tab.paths.splice(base..base, paths);
	tab.selected += selected;
	base
}

unsafe extern "C-unwind" fn manager_set_property(
	this: *mut ServiceBase,
	playlist: usize,
	property: *const Guid,
	stream: *mut c_void,
	_size_hint: usize,
	abort: *mut AbortCallback,
) {
	let reader = unsafe { &*(*stream.cast::<*const StreamReaderVtbl>()) };
	let mut value = Vec::new();
	loop {
		let mut piece = [0u8; 4];
		let read = unsafe { (reader.read)(stream, piece.as_mut_ptr().cast(), piece.len(), abort) };
		if read == 0 {
			break;
		}
		value.extend_from_slice(&piece[..read]);
	}
	let property = unsafe { *property };
	let mut tabs = state(this).tabs.lock().unwrap_or_else(PoisonError::into_inner);
	let Some(tab) = tabs.get_mut(playlist) else { return };
	tab.properties.retain(|(guid, _)| *guid != property);
	tab.properties.push((property, value));
}

unsafe extern "C-unwind" fn manager_get_property(
	this: *mut ServiceBase,
	playlist: usize,
	property: *const Guid,
	stream: *mut c_void,
	abort: *mut AbortCallback,
) -> bool {
	let property = unsafe { *property };
	let value = {
		let tabs = state(this).tabs.lock().unwrap_or_else(PoisonError::into_inner);
		tabs.get(playlist).and_then(|tab| {
			tab.properties
				.iter()
				.find(|(guid, _)| *guid == property)
				.map(|(_, value)| value.clone())
		})
	};
	let Some(value) = value else { return false };
	let writer = unsafe { &*(*stream.cast::<*const StreamWriterVtbl>()) };
	unsafe { (writer.write)(stream, value.as_ptr().cast(), value.len(), abort) };
	true
}

pub(super) struct FakeIncoming(pub(super) std::sync::Arc<super::services::FakeState>);

unsafe impl ServiceImpl for FakeIncoming {
	type Vtbl = IncomingItemFilterV2Vtbl;
	const IIDS: &'static [Guid] = &[INCOMING_IID, INCOMING_V2_IID];
	const VTBL: &'static IncomingItemFilterV2Vtbl = &IncomingItemFilterV2Vtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		unused_3_10: [None; 8],
		process_locations_async: incoming_process_locations,
	};
}

unsafe extern "C-unwind" fn incoming_process_locations(
	this: *mut ServiceBase,
	urls: *const ListBaseConst<*const c_char>,
	op_flags: u32,
	_restrict: *const c_char,
	_exclude: *const c_char,
	parent: *mut c_void,
	notify: ByValueServicePtr,
) {
	let state = &unsafe { ServiceObject::<FakeIncoming>::value(this) }.0.playlists;
	let vtbl = unsafe { &*(*urls).vtbl };
	let locations = (0..unsafe { (vtbl.get_count)(urls) })
		.map(|n| {
			let mut url: *const c_char = std::ptr::null();
			unsafe { (vtbl.get_item_ex)(urls, &raw mut url, n) };
			String::from_utf8_lossy(unsafe { bytes_of(url, usize::MAX) }).into_owned()
		})
		.collect();
	state.requests.lock().unwrap_or_else(PoisonError::into_inner).push(Request {
		locations,
		flags: op_flags,
		parent: parent.addr().cast_signed(),
		notify: notify.into_ptr(),
	});
}
