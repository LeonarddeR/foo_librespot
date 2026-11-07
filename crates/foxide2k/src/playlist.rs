//! foobar2000's playlist tabs: creating and switching them, keeping a value with one across
//! restarts, and loading links into one the way pasting them does. Main thread only.

use std::ffi::{CString, c_char, c_void};
use std::sync::{Mutex, PoisonError};

use crate::abi::abort_callback::AbortCallback;
use crate::abi::file::{StreamReaderVtbl, StreamWriterVtbl};
use crate::abi::input::ByValueServicePtr;
use crate::abi::metadb_handle::HandleListRaw;
use crate::abi::pfc_list::{BitArray, BitArrayVtbl, ListBaseConst, ListBaseConstVtbl};
use crate::abi::playlist::{
	INCOMING_IID, INCOMING_V2_IID, IncomingItemFilterV2Vtbl, MANAGER_IID, MANAGER_V2_IID,
	NOTIFY_IID, PlaylistManagerVtbl, ProcessLocationsNotifyVtbl,
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

/// What runs with the tracks of a finished load, on the main thread.
type Done = Box<dyn FnOnce(&HandleList<'_>) -> Result<()> + Send>;

fn manager() -> Result<ServicePtr> {
	host()
		.core_service(&MANAGER_IID)
		.ok_or_else(|| Error::new(ExceptionKind::BugCheck, "foobar2000 has no playlist_manager"))
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
	let manager = manager()?;
	let raw = manager.as_raw();
	Ok(try_host(|| unsafe { (vtbl(&manager).get_playlist_count)(raw) })?)
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
	let manager = manager_v2()?;
	let raw = manager.as_raw();
	let mut writer = MemoryWriter { vtbl: &raw const WRITER_VTBL, data: Vec::new() };
	let stream = (&raw mut writer).cast::<c_void>();
	let found = try_host(|| unsafe {
		(vtbl(&manager).playlist_get_property)(
			raw,
			index,
			property,
			stream,
			abort::never().as_raw(),
		)
	})?;
	Ok(found.then_some(writer.data))
}

/// The first tab whose `property` is `value`.
pub fn find_by_property(property: &Guid, value: &[u8]) -> Result<Option<usize>> {
	for index in 0..count()? {
		if self::property(index, property)?.as_deref() == Some(value) {
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

/// Appends `items` to tab `index`, unselected.
pub fn append(index: usize, items: &HandleList<'_>) -> Result<()> {
	let manager = manager()?;
	let raw = manager.as_raw();
	try_host(|| unsafe {
		(vtbl(&manager).playlist_insert_items)(
			raw,
			index,
			usize::MAX,
			items.as_raw(),
			&raw const NO_SELECTION.0,
		)
	})?;
	Ok(())
}

/// The `process_locations_notify` that carries the work for a finished load.
struct Notify(Mutex<Option<Done>>);

impl Notify {
	fn take(this: *mut ServiceBase) -> Option<Done> {
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
			Some(done) => done(&HandleList::from_raw(items)),
			None => Ok(()),
		});
	}
}

unsafe extern "C-unwind" fn on_aborted(this: *mut ServiceBase) {
	unsafe {
		guard(|| {
			drop(Notify::take(this));
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
/// its progress window, then calls `done` on the main thread. A cancelled load calls nothing.
pub fn load_locations(locations: &[String], done: Done) -> Result<()> {
	let filter = host()
		.core_service(&INCOMING_IID)
		.and_then(|filter| filter.query(&INCOMING_V2_IID))
		.ok_or_else(|| {
			Error::new(
				ExceptionKind::BugCheck,
				"foobar2000 has no playlist_incoming_item_filter_v2",
			)
		})?;
	let strings = locations
		.iter()
		.map(|location| {
			CString::new(location.as_str())
				.map_err(|_| Error::new(ExceptionKind::InvalidParams, "a link contains a NUL"))
		})
		.collect::<Result<Vec<_>>>()?;
	let list = UrlList {
		vtbl: &raw const URL_LIST_VTBL,
		urls: strings.iter().map(|string| string.as_ptr()).collect(),
	};
	let raw = filter.as_raw();
	let parent = host().main_window() as *mut c_void;
	// The by-value argument: a temporary holding one reference, which the callee releases.
	let mut temporary = new_service(Notify(Mutex::new(Some(done)))).into_raw();
	try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<IncomingItemFilterV2Vtbl>();
		(vtbl.process_locations_async)(
			raw,
			std::ptr::from_ref(&list).cast(),
			0,
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
		MemoryReader, activate, append, count, create, find_by_property, load_locations, property,
		set_property,
	};
	use crate::cxx::{ExceptionKind, try_host};
	use crate::error::Error;
	use crate::guid::Guid;
	use crate::testing::{FakeAbort, FakeApi};

	const P: Guid = Guid::from_u128(0x6a1f_0c3e_5b7d_4f20_9e48_2d1c_b3a7_5e90);

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

	#[test]
	fn loaded_locations_reach_done_on_completion() {
		let api = FakeApi::new();
		api.install();
		api.add_tab("Default");
		api.add_tab("Sunday set");
		load_locations(&["sptf://spotify:playlist:x".into()], Box::new(|items| append(1, items)))
			.unwrap();
		assert_eq!(api.location_requests(), [["sptf://spotify:playlist:x"]]);
		api.finish_locations(0, &["sptf://spotify:track:a", "sptf://spotify:track:b"]).unwrap();
		let tab = &api.tabs()[1];
		assert_eq!(tab.paths, ["sptf://spotify:track:a", "sptf://spotify:track:b"]);
		assert_eq!(tab.selected, 0);
	}

	#[test]
	fn a_cancelled_load_calls_nothing() {
		let api = FakeApi::new();
		api.install();
		let called = Arc::new(AtomicBool::new(false));
		let dropped = Arc::new(AtomicBool::new(false));
		let (seen, flag) = (Arc::clone(&called), DropFlag(Arc::clone(&dropped)));
		load_locations(
			&["sptf://spotify:playlist:x".into()],
			Box::new(move |_| {
				let _keep = &flag;
				seen.store(true, Ordering::SeqCst);
				Ok(())
			}),
		)
		.unwrap();
		assert!(!dropped.load(Ordering::SeqCst));
		api.cancel_locations(0);
		assert!(!called.load(Ordering::SeqCst));
		assert!(dropped.load(Ordering::SeqCst));
	}

	#[test]
	fn a_failing_done_throws() {
		let api = FakeApi::new();
		api.install();
		load_locations(
			&["sptf://spotify:playlist:x".into()],
			Box::new(|_| Err(Error::new(ExceptionKind::Io, "no tab"))),
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
