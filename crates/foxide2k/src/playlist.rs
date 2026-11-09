//! foobar2000's playlist tabs: creating and switching them, reading their names and locks,
//! keeping a value with one across restarts, and loading links into one the way pasting them
//! does. Main thread only.

use std::ffi::{c_char, c_void};
use std::sync::{Mutex, PoisonError};

use crate::abi::abort_callback::AbortCallback;
use crate::abi::file::{StreamReaderVtbl, StreamWriterVtbl};
use crate::abi::input::ByValueServicePtr;
use crate::abi::metadb_handle::HandleListRaw;
use crate::abi::pfc_list::{BitArray, BitArrayVtbl, ListBaseConst, ListBaseConstVtbl};
use crate::abi::playlist::{
	FILTER_ADD, INCOMING_IID, INCOMING_V2_IID, IncomingItemFilterV2Vtbl, MANAGER_IID,
	MANAGER_V2_IID, NOTIFY_IID, OP_FLAG_DELAY_UI, PlaylistManagerVtbl, ProcessLocationsNotifyVtbl,
};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abort;
use crate::boundary::guard;
use crate::cxx::{ExceptionKind, try_host};
use crate::error::{Error, Result};
use crate::guid::Guid;
use crate::host::host;
use crate::metadb::HandleList;
use crate::service::{ServiceImpl, ServiceObject, ServicePtr, new_service};
use crate::strings::{OwnedString, c_string};

/// What runs with the tracks of a finished load, on the main thread.
type Done = Box<dyn FnOnce(&HandleList<'_>) -> Result<()> + Send>;

/// What runs when a load is cancelled, on the main thread.
type Aborted = Box<dyn FnOnce() + Send>;

fn manager() -> Result<ServicePtr> {
	host().require(&MANAGER_IID, "playlist_manager")
}

fn manager_v2() -> Result<ServicePtr> {
	manager()?.query(&MANAGER_V2_IID).ok_or_else(|| {
		Error::new(ExceptionKind::BugCheck, "foobar2000's playlist_manager is not version 2")
	})
}

fn vtbl<'a>(service: &ServicePtr) -> &'a PlaylistManagerVtbl {
	unsafe { &*(*service.as_raw()).vtbl.cast::<PlaylistManagerVtbl>() }
}

/// How many tabs there are.
pub fn count() -> Result<usize> {
	count_of(&manager()?)
}

fn count_of(manager: &ServicePtr) -> Result<usize> {
	let raw = manager.as_raw();
	Ok(try_host(|| unsafe { (vtbl(manager).get_playlist_count)(raw) })?)
}

/// Creates a tab named `name` after the others and returns its index.
pub fn create(name: &str) -> Result<usize> {
	let manager = manager()?;
	let raw = manager.as_raw();
	let index = try_host(|| unsafe {
		(vtbl(&manager).create_playlist)(raw, name.as_ptr().cast(), name.len(), usize::MAX)
	})?;
	if index == usize::MAX {
		return Err(Error::new(ExceptionKind::Io, "foobar2000 did not create the playlist"));
	}
	Ok(index)
}

pub fn activate(index: usize) -> Result<()> {
	let manager = manager()?;
	let raw = manager.as_raw();
	Ok(try_host(|| unsafe { (vtbl(&manager).set_active_playlist)(raw, index) })?)
}

/// The active tab, `None` when no tab is active.
pub fn active() -> Result<Option<usize>> {
	let manager = manager()?;
	let raw = manager.as_raw();
	let index = try_host(|| unsafe { (vtbl(&manager).get_active_playlist)(raw) })?;
	Ok((index != usize::MAX).then_some(index))
}

/// The name of tab `index`.
pub fn name(index: usize) -> Result<String> {
	let manager = manager()?;
	let raw = manager.as_raw();
	let mut name = OwnedString::new();
	let out = name.as_raw();
	let found = try_host(|| unsafe { (vtbl(&manager).playlist_get_name)(raw, index, out) })?;
	if !found {
		return Err(Error::new(
			ExceptionKind::InvalidParams,
			format!("there is no playlist {index}"),
		));
	}
	Ok(name.value())
}

/// Whether a lock on tab `index` refuses tracks being added to it.
pub fn refuses_adding(index: usize) -> Result<bool> {
	let manager = manager()?;
	let raw = manager.as_raw();
	let mask = try_host(|| unsafe { (vtbl(&manager).playlist_lock_get_filter_mask)(raw, index) })?;
	Ok(mask & FILTER_ADD != 0)
}

/// Keeps `value` under `property` with tab `index`; foobar2000 saves it with the playlist.
pub fn set_property(index: usize, property: &Guid, value: &[u8]) -> Result<()> {
	let manager = manager_v2()?;
	let raw = manager.as_raw();
	let mut reader = MemoryReader::new(value);
	let stream = reader.as_raw();
	Ok(try_host(|| unsafe {
		(vtbl(&manager).playlist_set_property)(
			raw,
			index,
			property,
			stream,
			value.len(),
			abort::never().as_raw(),
		);
	})?)
}

/// What `set_property` kept under `property` with tab `index`, if anything.
pub fn property(index: usize, property: &Guid) -> Result<Option<Vec<u8>>> {
	property_of(&manager_v2()?, index, property)
}

/// [`property`] through `manager`, a `playlist_manager_v2`.
fn property_of(manager: &ServicePtr, index: usize, property: &Guid) -> Result<Option<Vec<u8>>> {
	let raw = manager.as_raw();
	let mut writer = MemoryWriter { vtbl: &raw const WRITER_VTBL, data: Vec::new() };
	let stream = (&raw mut writer).cast::<c_void>();
	let found = try_host(|| unsafe {
		(vtbl(manager).playlist_get_property)(raw, index, property, stream, abort::never().as_raw())
	})?;
	Ok(found.then_some(writer.data))
}

/// The first tab whose `property` is `value`.
pub fn find_by_property(property: &Guid, value: &[u8]) -> Result<Option<usize>> {
	let manager = manager_v2()?;
	for index in 0..count_of(&manager)? {
		if property_of(&manager, index, property)?.as_deref() == Some(value) {
			return Ok(Some(index));
		}
	}
	Ok(None)
}

/// No item selected: every bit is false.
struct NoSelection(BitArray);

// SAFETY: the vtable pointer is to a static and is never written.
unsafe impl Sync for NoSelection {}

static NO_SELECTION: NoSelection = NoSelection(BitArray { vtbl: &raw const NO_SELECTION_VTBL });

static NO_SELECTION_VTBL: BitArrayVtbl = BitArrayVtbl { get: bit_get, find: bit_find };

unsafe extern "C-unwind" fn bit_get(_this: *const BitArray, _n: usize) -> bool {
	false
}

unsafe extern "C-unwind" fn bit_find(
	_this: *const BitArray,
	value: bool,
	start: usize,
	count: isize,
) -> usize {
	if !value && count != 0 { start } else { start.wrapping_add_signed(count) }
}

/// Appends `items` to tab `index`, unselected. Fails when the tab refuses them, as a locked tab
/// does.
pub fn append(index: usize, items: &HandleList<'_>) -> Result<()> {
	let manager = manager()?;
	let raw = manager.as_raw();
	let at = try_host(|| unsafe {
		(vtbl(&manager).playlist_insert_items)(
			raw,
			index,
			usize::MAX,
			items.as_raw(),
			&raw const NO_SELECTION.0,
		)
	})?;
	if at == usize::MAX {
		return Err(Error::new(ExceptionKind::Std, "the playlist refused the tracks"));
	}
	Ok(())
}

/// The `process_locations_notify` that carries the work for a finished or cancelled load.
struct Notify(Mutex<Option<(Done, Aborted)>>);

impl Notify {
	fn take(this: *mut ServiceBase) -> Option<(Done, Aborted)> {
		unsafe { ServiceObject::<Self>::value(this) }
			.0
			.lock()
			.unwrap_or_else(PoisonError::into_inner)
			.take()
	}
}

unsafe impl ServiceImpl for Notify {
	type Vtbl = ProcessLocationsNotifyVtbl;
	const IIDS: &'static [Guid] = &[NOTIFY_IID];
	const VTBL: &'static ProcessLocationsNotifyVtbl = &ProcessLocationsNotifyVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		on_completion,
		on_aborted,
	};
}

unsafe extern "C-unwind" fn on_completion(this: *mut ServiceBase, items: *const HandleListRaw) {
	unsafe {
		guard(|| match Notify::take(this) {
			Some((done, _)) => done(&HandleList::from_raw(items)),
			None => Ok(()),
		});
	}
}

unsafe extern "C-unwind" fn on_aborted(this: *mut ServiceBase) {
	unsafe {
		guard(|| {
			if let Some((done, aborted)) = Notify::take(this) {
				drop(done);
				aborted();
			}
			Ok(())
		});
	}
}

/// A `list_base_const_t<const char *>` over NUL-terminated strings.
#[repr(C)]
struct UrlList {
	vtbl: *const ListBaseConstVtbl<*const c_char>,
	urls: Vec<*const c_char>,
}

static URL_LIST_VTBL: ListBaseConstVtbl<*const c_char> =
	ListBaseConstVtbl { get_count: url_count, get_item_ex: url_item };

unsafe extern "C-unwind" fn url_count(this: *const ListBaseConst<*const c_char>) -> usize {
	unsafe { &*this.cast::<UrlList>() }.urls.len()
}

unsafe extern "C-unwind" fn url_item(
	this: *const ListBaseConst<*const c_char>,
	out: *mut *const c_char,
	n: usize,
) {
	unsafe {
		guard(|| {
			*out = *(&*this.cast::<UrlList>())
				.urls
				.get(n)
				.ok_or_else(|| Error::new(ExceptionKind::InvalidParams, "no such URL"))?;
			Ok(())
		});
	}
}

/// Turns `locations` into tracks as pasting them does: foobar2000 runs the playlist loaders with
/// its progress window, then calls `done` on the main thread. A cancelled load calls `aborted`
/// instead, once `done` is dropped.
pub fn load_locations(locations: &[String], done: Done, aborted: Aborted) -> Result<()> {
	process_locations(locations, 0, host().main_window(), done, aborted)
}

/// [`load_locations`], quietly: foobar2000 shows no progress window for a short load, and the
/// window of a longer one doesn't take focus. That window is owned by `owner`, which must stay
/// open until the load ends, as foobar2000's main window does.
pub fn load_locations_quietly(
	owner: isize,
	locations: &[String],
	done: Done,
	aborted: Aborted,
) -> Result<()> {
	process_locations(locations, OP_FLAG_DELAY_UI, owner, done, aborted)
}

/// `process_locations_async` with `flags`, any progress window owned by `parent`.
fn process_locations(
	locations: &[String],
	flags: u32,
	parent: isize,
	done: Done,
	aborted: Aborted,
) -> Result<()> {
	const FILTER: &str = "playlist_incoming_item_filter_v2";
	let filter =
		host().require(&INCOMING_IID, FILTER)?.query(&INCOMING_V2_IID).ok_or_else(|| {
			Error::new(ExceptionKind::BugCheck, format!("foobar2000 has no {FILTER}"))
		})?;
	let strings = locations
		.iter()
		.map(|location| c_string(location, "a link"))
		.collect::<Result<Vec<_>>>()?;
	let list = UrlList {
		vtbl: &raw const URL_LIST_VTBL,
		urls: strings.iter().map(|string| string.as_ptr()).collect(),
	};
	let raw = filter.as_raw();
	let parent = parent as *mut c_void;
	// The by-value argument: a temporary holding one reference, which the callee releases.
	let mut temporary = new_service(Notify(Mutex::new(Some((done, aborted))))).into_raw();
	try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<IncomingItemFilterV2Vtbl>();
		(vtbl.process_locations_async)(
			raw,
			std::ptr::from_ref(&list).cast(),
			flags,
			std::ptr::null(),
			std::ptr::null(),
			parent,
			ByValueServicePtr::for_callee(&raw mut temporary),
		);
	})?;
	Ok(())
}

/// A `stream_reader` over a slice.
#[repr(C)]
struct MemoryReader<'a> {
	vtbl: *const StreamReaderVtbl,
	data: &'a [u8],
}

static READER_VTBL: StreamReaderVtbl = StreamReaderVtbl {
	read: reader_read,
	read_object: reader_read_object,
	skip: reader_skip,
	skip_object: reader_skip_object,
};

impl<'a> MemoryReader<'a> {
	const fn new(data: &'a [u8]) -> Self {
		Self { vtbl: &raw const READER_VTBL, data }
	}

	/// The `stream_reader *` to pass on.
	fn as_raw(&mut self) -> *mut c_void {
		std::ptr::from_mut(self).cast()
	}

	/// Takes up to `bytes` bytes off the front.
	fn take(&mut self, bytes: usize) -> &'a [u8] {
		let (taken, rest) = self.data.split_at(bytes.min(self.data.len()));
		self.data = rest;
		taken
	}

	/// Takes exactly `bytes` bytes off the front, or fails without taking any.
	fn take_exact(&mut self, bytes: usize) -> Result<&'a [u8]> {
		if self.data.len() < bytes {
			return Err(Error::new(
				ExceptionKind::IoDataTruncation,
				ExceptionKind::IoDataTruncation.default_message(),
			));
		}
		Ok(self.take(bytes))
	}
}

unsafe fn reader<'a>(this: *mut c_void) -> &'a mut MemoryReader<'a> {
	unsafe { &mut *this.cast::<MemoryReader<'_>>() }
}

unsafe extern "C-unwind" fn reader_read(
	this: *mut c_void,
	buffer: *mut c_void,
	bytes: usize,
	_abort: *mut AbortCallback,
) -> usize {
	unsafe {
		guard(|| {
			if bytes == 0 {
				return Ok(0);
			}
			let taken = reader(this).take(bytes);
			std::ptr::copy_nonoverlapping(taken.as_ptr(), buffer.cast::<u8>(), taken.len());
			Ok(taken.len())
		})
	}
}

unsafe extern "C-unwind" fn reader_read_object(
	this: *mut c_void,
	buffer: *mut c_void,
	bytes: usize,
	_abort: *mut AbortCallback,
) {
	unsafe {
		guard(|| {
			let taken = reader(this).take_exact(bytes)?;
			std::ptr::copy_nonoverlapping(taken.as_ptr(), buffer.cast::<u8>(), taken.len());
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn reader_skip(
	this: *mut c_void,
	bytes: u64,
	_abort: *mut AbortCallback,
) -> u64 {
	unsafe {
		guard(|| Ok(reader(this).take(usize::try_from(bytes).unwrap_or(usize::MAX)).len() as u64))
	}
}

unsafe extern "C-unwind" fn reader_skip_object(
	this: *mut c_void,
	bytes: u64,
	_abort: *mut AbortCallback,
) {
	unsafe {
		guard(|| {
			reader(this).take_exact(usize::try_from(bytes).unwrap_or(usize::MAX))?;
			Ok(())
		});
	}
}

/// A `stream_writer` that collects what is written.
#[repr(C)]
struct MemoryWriter {
	vtbl: *const StreamWriterVtbl,
	data: Vec<u8>,
}

static WRITER_VTBL: StreamWriterVtbl = StreamWriterVtbl { write: writer_write };

unsafe extern "C-unwind" fn writer_write(
	this: *mut c_void,
	buffer: *const c_void,
	bytes: usize,
	_abort: *mut AbortCallback,
) {
	unsafe {
		guard(|| {
			if bytes == 0 {
				return Ok(());
			}
			let written = std::slice::from_raw_parts(buffer.cast::<u8>(), bytes);
			(*this.cast::<MemoryWriter>()).data.extend_from_slice(written);
			Ok(())
		});
	}
}

#[cfg(test)]
mod tests {
	use std::ffi::c_void;
	use std::sync::Arc;
	use std::sync::atomic::{AtomicBool, Ordering};

	use super::{
		MemoryReader, activate, active, append, count, create, find_by_property, load_locations,
		load_locations_quietly, name, property, refuses_adding, set_property,
	};
	use crate::cxx::{ExceptionKind, try_host};
	use crate::error::Error;
	use crate::guid::Guid;
	use crate::metadb::HandleList;
	use crate::testing::{FakeAbort, FakeApi, FakeHandleList};

	const P: Guid = Guid::from_u128(0x6a1f_0c3e_5b7d_4f20_9e48_2d1c_b3a7_5e90);

	const GETTY: &str = "sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO";

	/// Sets its flag when dropped.
	struct DropFlag(Arc<AtomicBool>);

	impl Drop for DropFlag {
		fn drop(&mut self) {
			self.0.store(true, Ordering::SeqCst);
		}
	}

	fn tab_names(api: &FakeApi) -> Vec<String> {
		api.tabs().into_iter().map(|tab| tab.name).collect()
	}

	#[test]
	fn create_appends_a_tab_and_returns_its_index() {
		let api = FakeApi::new();
		api.install();
		api.add_tab("Default");
		assert_eq!(create("Sunday set").unwrap(), 1);
		assert_eq!(tab_names(api), ["Default", "Sunday set"]);
	}

	#[test]
	fn activate_makes_a_tab_active() {
		let api = FakeApi::new();
		api.install();
		api.add_tab("Default");
		api.add_tab("Sunday set");
		assert_eq!(api.active_tab(), None);
		activate(1).unwrap();
		assert_eq!(api.active_tab(), Some(1));
	}

	#[test]
	fn the_active_tab_is_none_until_one_is_active() {
		let api = FakeApi::new();
		api.install();
		assert_eq!(active().unwrap(), None);
		api.add_tab("Default");
		activate(0).unwrap();
		assert_eq!(active().unwrap(), Some(0));
		api.set_active_tab(None);
		assert_eq!(active().unwrap(), None);
	}

	#[test]
	fn a_tab_says_its_name() {
		let api = FakeApi::new();
		api.install();
		api.add_tab("Default");
		api.add_tab("Sunday");
		assert_eq!(name(1).unwrap(), "Sunday");
		assert_eq!(name(2).unwrap_err().kind, ExceptionKind::InvalidParams);
	}

	#[test]
	fn a_locked_tab_refuses_adding_and_append_says_so() {
		let api = FakeApi::new();
		api.install();
		api.add_tab("Default");
		api.add_tab("Sunday");
		api.lock_tab(0);
		assert!(refuses_adding(0).unwrap());
		assert!(!refuses_adding(1).unwrap());
		let list = FakeHandleList::new(&[GETTY]);
		let items = unsafe { HandleList::from_raw(list.as_raw()) };
		let error = append(0, &items).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Std);
		assert_eq!(error.message, "the playlist refused the tracks");
		assert!(api.tabs()[0].paths.is_empty());
		append(1, &items).unwrap();
		assert_eq!(api.tabs()[1].paths, [GETTY]);
	}

	#[test]
	fn a_quiet_load_delays_its_progress_window_under_its_owner() {
		let api = FakeApi::new();
		api.install();
		api.set_main_window(0x1234);
		let owner = 0x5678;
		load_locations_quietly(owner, &[GETTY.into()], Box::new(|_| Ok(())), Box::new(|| {}))
			.unwrap();
		assert_eq!(api.location_flags(), [(4, owner)]);
		load_locations(&[GETTY.into()], Box::new(|_| Ok(())), Box::new(|| {})).unwrap();
		assert_eq!(api.location_flags(), [(4, owner), (0, 0x1234)]);
		assert_eq!(api.location_requests(), [[GETTY], [GETTY]]);
	}

	#[test]
	fn a_property_reads_back_what_was_stored() {
		let api = FakeApi::new();
		api.install();
		api.add_tab("Default");
		api.add_tab("Sunday set");
		set_property(1, &P, b"spotify:playlist:x").unwrap();
		assert_eq!(property(1, &P).unwrap(), Some(b"spotify:playlist:x".to_vec()));
		assert_eq!(property(0, &P).unwrap(), None);
		assert_eq!(api.tabs()[1].properties, [(P, b"spotify:playlist:x".to_vec())]);
	}

	#[test]
	fn find_by_property_finds_the_tab_holding_the_value() {
		let api = FakeApi::new();
		api.install();
		api.add_tab("Default");
		api.add_tab("Sunday set");
		set_property(1, &P, b"spotify:playlist:x").unwrap();
		assert_eq!(find_by_property(&P, b"spotify:playlist:x").unwrap(), Some(1));
		assert_eq!(find_by_property(&P, b"spotify:playlist:y").unwrap(), None);
	}

	/// An `aborted` that sets its flag.
	fn flagging(flag: &Arc<AtomicBool>) -> Box<dyn FnOnce() + Send> {
		let flag = Arc::clone(flag);
		Box::new(move || flag.store(true, Ordering::SeqCst))
	}

	#[test]
	fn loaded_locations_reach_done_on_completion() {
		let api = FakeApi::new();
		api.install();
		api.add_tab("Default");
		api.add_tab("Sunday set");
		let aborted = Arc::new(AtomicBool::new(false));
		load_locations(
			&["sptf://spotify:playlist:x".into()],
			Box::new(|items| append(1, items)),
			flagging(&aborted),
		)
		.unwrap();
		assert_eq!(api.location_requests(), [["sptf://spotify:playlist:x"]]);
		api.finish_locations(0, &["sptf://spotify:track:a", "sptf://spotify:track:b"]).unwrap();
		let tab = &api.tabs()[1];
		assert_eq!(tab.paths, ["sptf://spotify:track:a", "sptf://spotify:track:b"]);
		assert_eq!(tab.selected, 0);
		assert!(!aborted.load(Ordering::SeqCst));
	}

	#[test]
	fn a_cancelled_load_calls_aborted_and_drops_done() {
		let api = FakeApi::new();
		api.install();
		let called = Arc::new(AtomicBool::new(false));
		let dropped = Arc::new(AtomicBool::new(false));
		let aborted = Arc::new(AtomicBool::new(false));
		let (seen, flag) = (Arc::clone(&called), DropFlag(Arc::clone(&dropped)));
		let dropped_at_abort = Arc::new(AtomicBool::new(false));
		let on_aborted = {
			let (aborted, dropped, at_abort) =
				(Arc::clone(&aborted), Arc::clone(&dropped), Arc::clone(&dropped_at_abort));
			Box::new(move || {
				at_abort.store(dropped.load(Ordering::SeqCst), Ordering::SeqCst);
				aborted.store(true, Ordering::SeqCst);
			})
		};
		load_locations(
			&["sptf://spotify:playlist:x".into()],
			Box::new(move |_| {
				let _keep = &flag;
				seen.store(true, Ordering::SeqCst);
				Ok(())
			}),
			on_aborted,
		)
		.unwrap();
		assert!(!dropped.load(Ordering::SeqCst));
		api.cancel_locations(0);
		assert!(aborted.load(Ordering::SeqCst));
		assert!(!called.load(Ordering::SeqCst));
		assert!(dropped.load(Ordering::SeqCst));
		assert!(dropped_at_abort.load(Ordering::SeqCst));
	}

	#[test]
	fn a_failing_done_throws() {
		let api = FakeApi::new();
		api.install();
		load_locations(
			&["sptf://spotify:playlist:x".into()],
			Box::new(|_| Err(Error::new(ExceptionKind::Io, "no tab"))),
			Box::new(|| {}),
		)
		.unwrap();
		let error = api.finish_locations(0, &[]).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Io);
		assert_eq!(error.message, "no tab");
	}

	#[test]
	fn a_short_read_of_an_object_throws_truncation() {
		let mut reader = MemoryReader::new(b"abc");
		let abort = FakeAbort::new(false);
		let this = reader.as_raw();
		let mut buffer = [0u8; 4];
		let error = try_host(|| unsafe {
			((*(*this.cast::<*const crate::abi::file::StreamReaderVtbl>())).read_object)(
				this,
				buffer.as_mut_ptr().cast::<c_void>(),
				4,
				abort.as_raw(),
			);
		})
		.unwrap_err();
		assert_eq!(error.kind, ExceptionKind::IoDataTruncation);
	}

	#[test]
	fn empty_reads_and_writes_accept_a_null_buffer() {
		let abort = FakeAbort::new(false);
		let mut writer =
			super::MemoryWriter { vtbl: &raw const super::WRITER_VTBL, data: Vec::new() };
		let this = (&raw mut writer).cast::<c_void>();
		try_host(|| unsafe {
			((*writer.vtbl).write)(this, std::ptr::null(), 0, abort.as_raw());
		})
		.unwrap();
		assert!(writer.data.is_empty());

		let mut reader = MemoryReader::new(b"abc");
		let this = reader.as_raw();
		let read = try_host(|| unsafe {
			((*(*this.cast::<*const crate::abi::file::StreamReaderVtbl>())).read)(
				this,
				std::ptr::null_mut(),
				0,
				abort.as_raw(),
			)
		})
		.unwrap();
		assert_eq!(read, 0);
		assert_eq!(reader.data, b"abc");
	}

	#[test]
	fn nothing_is_selected() {
		let selection = &raw const super::NO_SELECTION.0;
		let vtbl = unsafe { &*(*selection).vtbl };
		assert!(!unsafe { (vtbl.get)(selection, 3) });
		assert_eq!(unsafe { (vtbl.find)(selection, false, 2, 5) }, 2);
		assert_eq!(unsafe { (vtbl.find)(selection, true, 2, 5) }, 7);
		assert_eq!(unsafe { (vtbl.find)(selection, true, 2, -2) }, 0);
		assert_eq!(unsafe { (vtbl.find)(selection, false, 2, 0) }, 2);
	}

	#[test]
	fn without_foobar2000_there_are_no_tabs() {
		assert!(count().is_err());
	}
}
