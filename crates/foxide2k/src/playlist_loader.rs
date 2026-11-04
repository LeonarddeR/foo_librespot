//! Registering a playlist loader: the service foobar2000 asks to expand an added path into
//! playlist entries.
//!
//! foobar2000 picks loaders by the content type of the file it opened for the path (or by
//! extension, which these loaders don't use). A loader that doesn't handle the path throws
//! `exception_io_unsupported_format`, and foobar2000 tries the next one.

use std::ffi::{CStr, CString, c_char, c_void};
use std::marker::PhantomData;

use crate::abi::abort_callback::AbortCallback;
use crate::abi::file::FileStats;
use crate::abi::input::ByValueServicePtr;
use crate::abi::playlist_loader::{
	ENTRY_USER_REQUESTED, IID, PlayableLocation, PlayableLocationVtbl, PlaylistLoaderCallbackVtbl,
	PlaylistLoaderVtbl,
};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abort::Abort;
use crate::boundary::guard;
use crate::component::Factory;
use crate::cxx::{ExceptionKind, try_host};
use crate::error::{Error, Result};
use crate::file_info_impl::FileInfoImpl;
use crate::guid::Guid;
use crate::service::{ServiceImpl, ServicePtr, new_service};

/// A component's playlist loader.
pub trait PlaylistLoader: 'static {
	fn is_our_content_type(content_type: &str) -> bool;
	/// Adds the entries `path` stands for through `callback`. Return
	/// [`Error::unsupported_format`] for a path this loader doesn't handle, so foobar2000 tries
	/// the next loader.
	fn open(path: &str, callback: &mut LoaderCallback<'_>, abort: &Abort) -> Result<()>;
}

/// The factory that registers `T` with foobar2000.
pub const fn playlist_loader_factory<T: PlaylistLoader>() -> Factory {
	Factory::per_instance(&IID, playlist_loader_service::<T>)
}

/// A new `playlist_loader` service for `T`.
pub fn playlist_loader_service<T: PlaylistLoader>() -> ServicePtr {
	new_service(LoaderService::<T>(PhantomData))
}

struct LoaderService<T>(PhantomData<fn() -> T>);

unsafe impl<T: PlaylistLoader> ServiceImpl for LoaderService<T> {
	type Vtbl = PlaylistLoaderVtbl;
	const IIDS: &'static [Guid] = &[IID];
	const VTBL: &'static PlaylistLoaderVtbl = &PlaylistLoaderVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		open: open::<T>,
		write,
		get_extension,
		can_write: no,
		is_our_content_type: is_our_content_type::<T>,
		is_associatable: no,
	};
}

unsafe fn text_of<'a>(text: *const c_char) -> &'a str {
	if text.is_null() { "" } else { unsafe { CStr::from_ptr(text) }.to_str().unwrap_or("") }
}

unsafe extern "C-unwind" fn open<T: PlaylistLoader>(
	_this: *mut ServiceBase,
	path: *const c_char,
	_file: *const *mut ServiceBase,
	callback: ByValueServicePtr,
	abort: *mut AbortCallback,
) {
	unsafe {
		guard(|| {
			// Passed by value: this reference is ours to release, whatever happens.
			let callback = ServicePtr::from_raw_owned(*callback).ok_or_else(|| {
				Error::new(ExceptionKind::InvalidParams, "no playlist_loader_callback")
			})?;
			let abort = Abort::from_raw(abort);
			let mut callback = LoaderCallback::from_raw(callback.as_raw());
			T::open(text_of(path), &mut callback, &abort)
		});
	}
}

unsafe extern "C-unwind" fn write(
	_this: *mut ServiceBase,
	_path: *const c_char,
	_file: *const *mut ServiceBase,
	_handles: *const c_void,
	_abort: *mut AbortCallback,
) {
	unsafe { guard(|| Err::<(), _>(Error::not_implemented())) }
}

unsafe extern "C-unwind" fn get_extension(_this: *mut ServiceBase) -> *const c_char {
	// Matched by content type only.
	c"".as_ptr()
}

unsafe extern "C-unwind" fn no(_this: *mut ServiceBase) -> bool {
	false
}

unsafe extern "C-unwind" fn is_our_content_type<T: PlaylistLoader>(
	_this: *mut ServiceBase,
	content_type: *const c_char,
) -> bool {
	unsafe { guard(|| Ok(T::is_our_content_type(text_of(content_type)))) }
}

/// The `playlist_loader_callback` foobar2000 passed to `open`.
pub struct LoaderCallback<'a> {
	raw: *mut ServiceBase,
	_borrow: PhantomData<&'a ServiceBase>,
}

impl LoaderCallback<'_> {
	/// # Safety
	/// `raw` must be a `playlist_loader_callback` that outlives the returned value.
	pub unsafe fn from_raw(raw: *mut ServiceBase) -> Self {
		Self { raw, _borrow: PhantomData }
	}

	fn vtbl(&self) -> &PlaylistLoaderCallbackVtbl {
		unsafe { &*(*self.raw).vtbl.cast::<PlaylistLoaderCallbackVtbl>() }
	}

	/// Shows `path` in foobar2000's progress display.
	pub fn on_progress(&mut self, path: &str) -> Result<()> {
		let path = c_string(path)?;
		let (vtbl, raw) = (self.vtbl(), self.raw);
		try_host(|| unsafe { (vtbl.on_progress)(raw, path.as_ptr()) })?;
		Ok(())
	}

	/// Adds an entry for `path` that the user asked for, with `info` already known, so
	/// foobar2000 shows it without opening the track.
	pub fn add_entry(&mut self, path: &str, info: &mut FileInfoImpl) -> Result<()> {
		let path = c_string(path)?;
		let location =
			PlayableLocation { vtbl: &raw const LOCATION_VTBL, path: path.as_ptr(), subsong: 0 };
		let (vtbl, raw) = (self.vtbl(), self.raw);
		let mut handle: *mut ServiceBase = std::ptr::null_mut();
		let handle_slot = &raw mut handle;
		try_host(|| unsafe { (vtbl.handle_create)(raw, handle_slot, &raw const location) })?;
		// The new handle's reference is ours; releasing it when done is the caller's job.
		let handle = unsafe { ServicePtr::from_raw_owned(handle) }.ok_or_else(|| {
			Error::new(ExceptionKind::Io, "foobar2000 created no handle for the entry")
		})?;
		let handle_raw = handle.as_raw();
		let info = info.as_raw();
		let stats = FileStats::INVALID;
		try_host(|| unsafe {
			(vtbl.on_entry_info)(
				raw,
				&raw const handle_raw,
				ENTRY_USER_REQUESTED,
				&raw const stats,
				info,
				false,
			);
		})?;
		Ok(())
	}
}

fn c_string(text: &str) -> Result<CString> {
	CString::new(text).map_err(|_| Error::new(ExceptionKind::InvalidParams, "text contains NUL"))
}

/// The `playable_location` vtable for [`LoaderCallback::add_entry`], like the SDK's
/// `make_playable_location`: read-only, the setters do nothing.
static LOCATION_VTBL: PlayableLocationVtbl = PlayableLocationVtbl {
	get_path: location_get_path,
	set_path: location_set_path,
	get_subsong: location_get_subsong,
	set_subsong: location_set_subsong,
};

unsafe extern "C-unwind" fn location_get_path(this: *const PlayableLocation) -> *const c_char {
	unsafe { (*this).path }
}

unsafe extern "C-unwind" fn location_set_path(_this: *mut PlayableLocation, _path: *const c_char) {}

unsafe extern "C-unwind" fn location_get_subsong(this: *const PlayableLocation) -> u32 {
	unsafe { (*this).subsong }
}

unsafe extern "C-unwind" fn location_set_subsong(_this: *mut PlayableLocation, _subsong: u32) {}

#[cfg(test)]
mod tests {
	use super::{LoaderCallback, PlaylistLoader, playlist_loader_service};
	use crate::abi::file::FileStats;
	use crate::abi::playlist_loader::ENTRY_USER_REQUESTED;
	use crate::abort::Abort;
	use crate::cxx::ExceptionKind;
	use crate::error::{Error, Result};
	use crate::file_info_impl::FileInfoImpl;
	use crate::testing::{FakeAbort, FakeLoaderCallback, PlaylistLoaderHandle};

	struct TestLoader;

	impl PlaylistLoader for TestLoader {
		fn is_our_content_type(content_type: &str) -> bool {
			content_type == "application/x-test"
		}

		fn open(path: &str, callback: &mut LoaderCallback<'_>, _abort: &Abort) -> Result<()> {
			if path != "test://album" {
				return Err(Error::unsupported_format("not a test album"));
			}
			callback.on_progress(path)?;
			for (entry, title) in [("test://1", "One"), ("test://2", "Two")] {
				let mut info = FileInfoImpl::new();
				info.file_info().meta_set("TITLE", title)?;
				callback.add_entry(entry, &mut info)?;
			}
			Ok(())
		}
	}

	fn loader() -> PlaylistLoaderHandle {
		PlaylistLoaderHandle::new(playlist_loader_service::<TestLoader>())
	}

	#[test]
	fn loader_is_matched_by_content_type_only() {
		let loader = loader();
		assert_eq!(loader.extension(), "");
		assert!(loader.is_our_content_type("application/x-test"));
		assert!(!loader.is_our_content_type("text/html"));
		assert!(!loader.can_write());
	}

	#[test]
	fn open_adds_entries_with_prefilled_info_in_order() {
		let (callback, record) = FakeLoaderCallback::new();
		loader().open("test://album", callback, &FakeAbort::new(false)).unwrap();
		let record = record.lock().unwrap();
		assert_eq!(record.progress, ["test://album"]);
		let paths: Vec<&str> = record.entries.iter().map(|entry| entry.path.as_str()).collect();
		assert_eq!(paths, ["test://1", "test://2"]);
		for (entry, title) in record.entries.iter().zip(["One", "Two"]) {
			assert_eq!(entry.meta("title"), [title]);
			assert_eq!(entry.entry_type, ENTRY_USER_REQUESTED);
			assert!(!entry.fresh);
			assert_eq!(entry.stats, FileStats::INVALID);
		}
	}

	#[test]
	fn every_created_handle_is_released() {
		let (callback, record) = FakeLoaderCallback::new();
		loader().open("test://album", callback, &FakeAbort::new(false)).unwrap();
		let record = record.lock().unwrap();
		assert_eq!(record.handles_created, 2);
		assert_eq!(record.live_handles(), 0);
	}

	#[test]
	fn loader_error_reaches_the_host_as_its_kind() {
		let (callback, _record) = FakeLoaderCallback::new();
		let error =
			loader().open("https://example.com/", callback, &FakeAbort::new(false)).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::IoUnsupportedFormat);
	}

	#[test]
	fn the_by_value_callback_is_released() {
		let (callback, record) = FakeLoaderCallback::new();
		loader().open("test://album", callback, &FakeAbort::new(false)).unwrap();
		assert!(record.lock().unwrap().callback_dropped);
	}

	#[test]
	fn the_callback_is_released_when_open_fails() {
		let (callback, record) = FakeLoaderCallback::new();
		let _ = loader().open("other://", callback, &FakeAbort::new(false));
		assert!(record.lock().unwrap().callback_dropped);
	}

	#[test]
	fn write_is_not_implemented() {
		let error = loader().write("test://album").unwrap_err();
		assert_eq!(error.kind, ExceptionKind::NotImplemented);
	}
}
