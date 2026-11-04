//! Registering a filesystem: the handler foobar2000 asks about paths with a component's scheme.
//!
//! foobar2000 opens a path through its filesystem before it looks for an input or playlist
//! loader for it: to check that it exists, to read ahead when buffering is on, and to learn its
//! content type. A component whose paths aren't files therefore registers a filesystem that
//! claims them and hands out stub files, as the SDK's `file_streamstub` does. foo_spotify did
//! the same for `sptf://`.

use std::ffi::{CStr, CString, c_char, c_void};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::abi::abort_callback::AbortCallback;
use crate::abi::file::{
	FILE_IID, FileStats, FileVtbl, OPEN_MODE_MASK, OPEN_MODE_READ, StreamReaderVtbl,
	StreamWriterVtbl,
};
use crate::abi::filesystem::{FilesystemVtbl, IID};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::boundary::guard;
use crate::component::Factory;
use crate::cxx::ExceptionKind;
use crate::error::Error;
use crate::guid::Guid;
use crate::service::{ServiceImpl, ServicePtr, new_service};
use crate::strings::set_string;

/// A component's filesystem. OxiBar answers everything else the same way for every
/// implementation: paths open as read-only stub files that read nothing, everything else that
/// would change them is denied, and none of them is a directory.
pub trait Filesystem: 'static {
	fn is_our_path(path: &str) -> bool;
	/// The one spelling of `path` foobar2000 should store, if it is ours.
	fn canonical_path(path: &str) -> Option<String>;
	/// How foobar2000 shows `path` to the user, if it is ours.
	fn display_path(path: &str) -> Option<String>;
	/// The content type of `path`'s stub file. foobar2000 asks every playlist loader that
	/// accepts this content type to expand the path; `None` sends it straight to the inputs.
	fn content_type(path: &str) -> Option<String>;
}

/// The factory that registers `T` with foobar2000.
pub const fn filesystem_factory<T: Filesystem>() -> Factory {
	Factory::per_instance(&IID, filesystem_service::<T>)
}

/// A new `filesystem` service for `T`.
pub fn filesystem_service<T: Filesystem>() -> ServicePtr {
	new_service(FilesystemService::<T>(PhantomData))
}

struct FilesystemService<T>(PhantomData<fn() -> T>);

unsafe impl<T: Filesystem> ServiceImpl for FilesystemService<T> {
	type Vtbl = FilesystemVtbl;
	const IIDS: &'static [Guid] = &[IID];
	const VTBL: &'static FilesystemVtbl = &FilesystemVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_canonical_path: canonical_path::<T>,
		is_our_path: is_our_path::<T>,
		get_display_path: display_path::<T>,
		open: open::<T>,
		remove: deny_path,
		move_file: deny_move,
		is_remote,
		get_stats,
		relative_path_create: no_relative_path,
		relative_path_parse: no_relative_path,
		create_directory: deny_path,
		list_directory,
		supports_content_types,
	};
}

unsafe fn path_of<'a>(path: *const c_char) -> &'a str {
	if path.is_null() { "" } else { unsafe { CStr::from_ptr(path) }.to_str().unwrap_or("") }
}

fn denied() -> Error {
	Error::new(ExceptionKind::IoDenied, "this path can't be written to")
}

unsafe extern "C-unwind" fn canonical_path<T: Filesystem>(
	_this: *mut ServiceBase,
	path: *const c_char,
	out: *mut StringBase,
) -> bool {
	unsafe {
		guard(|| match T::canonical_path(path_of(path)) {
			Some(canonical) => set_string(out, &canonical).map(|()| true),
			None => Ok(false),
		})
	}
}

unsafe extern "C-unwind" fn is_our_path<T: Filesystem>(
	_this: *mut ServiceBase,
	path: *const c_char,
) -> bool {
	unsafe { guard(|| Ok(T::is_our_path(path_of(path)))) }
}

unsafe extern "C-unwind" fn display_path<T: Filesystem>(
	_this: *mut ServiceBase,
	path: *const c_char,
	out: *mut StringBase,
) -> bool {
	unsafe {
		guard(|| match T::display_path(path_of(path)) {
			Some(display) => set_string(out, &display).map(|()| true),
			None => Ok(false),
		})
	}
}

unsafe extern "C-unwind" fn open<T: Filesystem>(
	_this: *mut ServiceBase,
	out: *mut *mut ServiceBase,
	path: *const c_char,
	mode: u32,
	_abort: *mut AbortCallback,
) {
	unsafe {
		guard(|| {
			if mode & OPEN_MODE_MASK != OPEN_MODE_READ {
				return Err(denied());
			}
			let file = new_stub_file(T::content_type(path_of(path)));
			// `out` is a `service_ptr_t<file> &`: replace whatever it held.
			if let Some(previous) = ServicePtr::from_raw_owned(*out) {
				drop(previous);
			}
			*out = file.into_raw();
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn deny_path(
	_this: *mut ServiceBase,
	_path: *const c_char,
	_abort: *mut AbortCallback,
) {
	unsafe { guard(|| Err::<(), _>(denied())) }
}

unsafe extern "C-unwind" fn deny_move(
	_this: *mut ServiceBase,
	_source: *const c_char,
	_destination: *const c_char,
	_abort: *mut AbortCallback,
) {
	unsafe { guard(|| Err::<(), _>(denied())) }
}

unsafe extern "C-unwind" fn is_remote(_this: *mut ServiceBase, _path: *const c_char) -> bool {
	false
}

unsafe extern "C-unwind" fn get_stats(
	_this: *mut ServiceBase,
	_path: *const c_char,
	stats: *mut FileStats,
	is_writeable: *mut bool,
	_abort: *mut AbortCallback,
) {
	unsafe {
		guard(|| {
			stats.write(FileStats::INVALID);
			is_writeable.write(false);
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn no_relative_path(
	_this: *mut ServiceBase,
	_first: *const c_char,
	_second: *const c_char,
	_out: *mut StringBase,
) -> bool {
	false
}

unsafe extern "C-unwind" fn list_directory(
	_this: *mut ServiceBase,
	_path: *const c_char,
	_callback: *mut c_void,
	_abort: *mut AbortCallback,
) {
	// foobar2000 silently skips exception_io_not_directory when it walks an added path.
	unsafe { guard(|| Err::<(), _>(Error::new(ExceptionKind::IoNotDirectory, "not a directory"))) }
}

unsafe extern "C-unwind" fn supports_content_types(_this: *mut ServiceBase) -> bool {
	true
}

/// How many stub files exist, so tests can check that the last release frees one.
#[cfg(test)]
pub(crate) static LIVE_STUB_FILES: std::sync::atomic::AtomicUsize =
	std::sync::atomic::AtomicUsize::new(0);

/// A `file` that reads nothing. `file` has three bases, so the object starts with three vtable
/// pointers, at the offsets MSVC gives them.
#[repr(C)]
struct StubFile {
	primary: *const FileVtbl,
	reader: *const StreamReaderVtbl,
	writer: *const StreamWriterVtbl,
	refs: AtomicI32,
	content_type: Option<CString>,
}

fn new_stub_file(content_type: Option<String>) -> ServicePtr {
	#[cfg(test)]
	LIVE_STUB_FILES.fetch_add(1, Ordering::SeqCst);
	let file = Box::new(StubFile {
		primary: &raw const STUB_FILE_VTBL,
		reader: &raw const STUB_READER_VTBL,
		writer: &raw const STUB_WRITER_VTBL,
		refs: AtomicI32::new(1),
		content_type: content_type.and_then(|content_type| CString::new(content_type).ok()),
	});
	let raw = Box::into_raw(file).cast::<ServiceBase>();
	// SAFETY: a fresh object with one reference, which the pointer takes over.
	unsafe { ServicePtr::from_raw_owned(raw) }.expect("Box::into_raw is never null")
}

unsafe fn stub<'a>(this: *mut ServiceBase) -> &'a StubFile {
	unsafe { &*this.cast::<StubFile>() }
}

static STUB_FILE_VTBL: FileVtbl = FileVtbl {
	base: ServiceBaseVtbl {
		service_release: stub_release,
		service_add_ref: stub_add_ref,
		service_query: stub_query,
	},
	get_size: stub_get_size,
	get_position: stub_zero,
	resize: stub_resize,
	seek: stub_seek,
	seek_ex: stub_seek_ex,
	can_seek: stub_false,
	get_content_type: stub_get_content_type,
	is_in_memory: stub_false,
	on_idle: stub_nothing,
	get_timestamp: stub_zero,
	reopen: stub_nothing,
	is_remote: stub_true,
};

static STUB_READER_VTBL: StreamReaderVtbl = StreamReaderVtbl {
	read: stub_read,
	read_object: stub_read_object,
	skip: stub_skip,
	skip_object: stub_skip_object,
};

static STUB_WRITER_VTBL: StreamWriterVtbl = StreamWriterVtbl { write: stub_write };

unsafe extern "C-unwind" fn stub_release(this: *mut ServiceBase) -> i32 {
	unsafe {
		let remaining = stub(this).refs.fetch_sub(1, Ordering::AcqRel) - 1;
		if remaining == 0 {
			drop(Box::from_raw(this.cast::<StubFile>()));
			#[cfg(test)]
			LIVE_STUB_FILES.fetch_sub(1, Ordering::SeqCst);
		}
		remaining
	}
}

unsafe extern "C-unwind" fn stub_add_ref(this: *mut ServiceBase) -> i32 {
	unsafe { stub(this).refs.fetch_add(1, Ordering::AcqRel) + 1 }
}

unsafe extern "C-unwind" fn stub_query(
	this: *mut ServiceBase,
	out: *mut *mut ServiceBase,
	guid: *const Guid,
) -> bool {
	unsafe {
		if *guid != FILE_IID {
			return false;
		}
		// Same order as service_ptr_t::copy: add the new reference before releasing the old one.
		stub_add_ref(this);
		let old = *out;
		*out = this;
		if !old.is_null() {
			((*(*old).vtbl).service_release)(old);
		}
		true
	}
}

unsafe extern "C-unwind" fn stub_get_size(
	_this: *mut ServiceBase,
	_abort: *mut AbortCallback,
) -> u64 {
	u64::MAX
}

unsafe extern "C-unwind" fn stub_zero(_this: *mut ServiceBase, _abort: *mut AbortCallback) -> u64 {
	0
}

unsafe extern "C-unwind" fn stub_resize(
	_this: *mut ServiceBase,
	_size: u64,
	_abort: *mut AbortCallback,
) {
	unsafe { guard(|| Err::<(), _>(denied())) }
}

fn not_seekable() -> Error {
	Error::new(
		ExceptionKind::IoObjectNotSeekable,
		ExceptionKind::IoObjectNotSeekable.default_message(),
	)
}

unsafe extern "C-unwind" fn stub_seek(
	_this: *mut ServiceBase,
	_position: u64,
	_abort: *mut AbortCallback,
) {
	unsafe { guard(|| Err::<(), _>(not_seekable())) }
}

unsafe extern "C-unwind" fn stub_seek_ex(
	_this: *mut ServiceBase,
	_position: i64,
	_mode: i32,
	_abort: *mut AbortCallback,
) {
	unsafe { guard(|| Err::<(), _>(not_seekable())) }
}

unsafe extern "C-unwind" fn stub_false(_this: *mut ServiceBase) -> bool {
	false
}

unsafe extern "C-unwind" fn stub_true(_this: *mut ServiceBase) -> bool {
	// As the SDK's file_streamstub.
	true
}

unsafe extern "C-unwind" fn stub_nothing(_this: *mut ServiceBase, _abort: *mut AbortCallback) {}

unsafe extern "C-unwind" fn stub_get_content_type(
	this: *mut ServiceBase,
	out: *mut StringBase,
) -> bool {
	unsafe {
		guard(|| match &stub(this).content_type {
			Some(content_type) => set_string(out, &content_type.to_string_lossy()).map(|()| true),
			None => Ok(false),
		})
	}
}

fn truncated() -> Error {
	Error::new(ExceptionKind::IoDataTruncation, "the stream is empty")
}

unsafe extern "C-unwind" fn stub_read(
	_this: *mut c_void,
	_buffer: *mut c_void,
	_bytes: usize,
	_abort: *mut AbortCallback,
) -> usize {
	0
}

unsafe extern "C-unwind" fn stub_read_object(
	_this: *mut c_void,
	_buffer: *mut c_void,
	bytes: usize,
	_abort: *mut AbortCallback,
) {
	unsafe { guard(|| if bytes == 0 { Ok(()) } else { Err(truncated()) }) }
}

unsafe extern "C-unwind" fn stub_skip(
	_this: *mut c_void,
	_bytes: u64,
	_abort: *mut AbortCallback,
) -> u64 {
	0
}

unsafe extern "C-unwind" fn stub_skip_object(
	_this: *mut c_void,
	bytes: u64,
	_abort: *mut AbortCallback,
) {
	unsafe { guard(|| if bytes == 0 { Ok(()) } else { Err(truncated()) }) }
}

unsafe extern "C-unwind" fn stub_write(
	_this: *mut c_void,
	_buffer: *const c_void,
	_bytes: usize,
	_abort: *mut AbortCallback,
) {
	unsafe { guard(|| Err::<(), _>(denied())) }
}

#[cfg(test)]
mod tests {
	use super::{Filesystem, LIVE_STUB_FILES, filesystem_service};
	use crate::abi::file::{FileStats, OPEN_MODE_READ, OPEN_MODE_WRITE_NEW, OPEN_SHAREABLE};
	use crate::cxx::ExceptionKind;
	use crate::testing::FilesystemHandle;

	struct TestFs;

	impl Filesystem for TestFs {
		fn is_our_path(path: &str) -> bool {
			path.starts_with("test://")
		}

		fn canonical_path(path: &str) -> Option<String> {
			Self::is_our_path(path).then(|| path.to_ascii_lowercase())
		}

		fn display_path(path: &str) -> Option<String> {
			path.strip_prefix("test://").map(str::to_owned)
		}

		fn content_type(path: &str) -> Option<String> {
			(path == "test://playlist").then(|| "application/x-test".to_owned())
		}
	}

	fn filesystem() -> FilesystemHandle {
		FilesystemHandle::new(filesystem_service::<TestFs>())
	}

	#[test]
	fn filesystem_claims_only_its_paths() {
		let filesystem = filesystem();
		assert!(filesystem.is_our_path("test://track"));
		assert!(!filesystem.is_our_path("file://C:\\a.mp3"));
		assert!(filesystem.supports_content_types());
	}

	#[test]
	fn canonical_and_display_paths_come_from_the_trait() {
		let filesystem = filesystem();
		assert_eq!(filesystem.canonical_path("test://TRACK").as_deref(), Some("test://track"));
		assert_eq!(filesystem.canonical_path("other://x"), None);
		assert_eq!(filesystem.display_path("test://track").as_deref(), Some("track"));
	}

	#[test]
	fn open_for_reading_returns_a_stub_file_even_when_shareable() {
		let file = filesystem().open("test://track", OPEN_MODE_READ | OPEN_SHAREABLE).unwrap();
		assert_eq!(file.size(), u64::MAX);
	}

	#[test]
	fn open_for_writing_throws_denied() {
		let error = filesystem().open("test://track", OPEN_MODE_WRITE_NEW).err().unwrap();
		assert_eq!(error.kind, ExceptionKind::IoDenied);
	}

	#[test]
	fn stub_file_reads_nothing_and_cannot_seek() {
		let file = filesystem().open("test://track", OPEN_MODE_READ).unwrap();
		assert!(!file.can_seek());
		assert_eq!(file.seek(10).unwrap_err().kind, ExceptionKind::IoObjectNotSeekable);
		assert_eq!(file.read(16).unwrap(), 0);
		assert_eq!(file.read_object(16).unwrap_err().kind, ExceptionKind::IoDataTruncation);
		assert_eq!(file.write(b"x").unwrap_err().kind, ExceptionKind::IoDenied);
		assert!(file.is_remote());
	}

	#[test]
	fn stub_file_reports_the_content_type() {
		let filesystem = filesystem();
		let playlist = filesystem.open("test://playlist", OPEN_MODE_READ).unwrap();
		assert_eq!(playlist.content_type().as_deref(), Some("application/x-test"));
		let track = filesystem.open("test://track", OPEN_MODE_READ).unwrap();
		assert_eq!(track.content_type(), None);
	}

	#[test]
	fn list_directory_throws_not_directory() {
		let error = filesystem().list_directory("test://track").unwrap_err();
		assert_eq!(error.kind, ExceptionKind::IoNotDirectory);
	}

	#[test]
	fn get_stats_is_invalid_and_not_writeable() {
		assert_eq!(filesystem().get_stats("test://track").unwrap(), (FileStats::INVALID, false));
	}

	#[test]
	fn stub_file_is_freed_by_its_last_release() {
		let file = filesystem().open("test://track", OPEN_MODE_READ).unwrap();
		assert_eq!(LIVE_STUB_FILES.load(std::sync::atomic::Ordering::SeqCst), 1);
		let copy = file.service().clone();
		drop(copy);
		assert_eq!(LIVE_STUB_FILES.load(std::sync::atomic::Ordering::SeqCst), 1);
		drop(file);
		assert_eq!(LIVE_STUB_FILES.load(std::sync::atomic::Ordering::SeqCst), 0);
	}

	#[test]
	fn stub_file_answers_service_query_for_file_only() {
		let file = filesystem().open("test://track", OPEN_MODE_READ).unwrap();
		assert!(file.service().query(&crate::abi::file::FILE_IID).is_some());
		assert!(file.service().query(&crate::abi::filesystem::IID).is_none());
	}
}
