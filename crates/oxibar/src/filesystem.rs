//! Calling foobar2000's filesystems.

use std::ffi::CString;

use crate::abi::file::FileStats;
use crate::abi::filesystem::{FilesystemVtblPrefix, IID};
use crate::abort::Abort;
use crate::cxx::{ExceptionKind, try_host};
use crate::error::{Error, Result};
use crate::host::host;

/// The size and timestamp of whatever is at `path`, from the first registered filesystem that
/// claims the path. foobar2000's own exceptions, such as `exception_io_not_found`, come back as
/// errors that keep their RTTI type name.
pub fn get_stats(path: &str, abort: &Abort) -> Result<FileStats> {
	let c_path = CString::new(path)
		.map_err(|_| Error::new(ExceptionKind::IoNoHandlerForPath, "path contains NUL"))?;
	for filesystem in host().enumerate(&IID) {
		let raw = filesystem.as_raw();
		let vtbl = unsafe { &*(*raw).vtbl.cast::<FilesystemVtblPrefix>() };
		if !try_host(|| unsafe { (vtbl.is_our_path)(raw, c_path.as_ptr()) })? {
			continue;
		}
		let mut stats = FileStats::INVALID;
		let mut is_writeable = false;
		try_host(|| unsafe {
			(vtbl.get_stats)(
				raw,
				c_path.as_ptr(),
				&raw mut stats,
				&raw mut is_writeable,
				abort.as_raw(),
			);
		})?;
		return Ok(stats);
	}
	Err(Error::new(
		ExceptionKind::IoNoHandlerForPath,
		ExceptionKind::IoNoHandlerForPath.default_message(),
	))
}

#[cfg(test)]
mod tests {
	use super::get_stats;
	use crate::abi::filesystem::IID as FILESYSTEM;
	use crate::abort::Abort;
	use crate::cxx::ExceptionKind;
	use crate::testing::{FakeAbort, FakeApi, FakeFilesystem};

	#[test]
	fn get_stats_maps_host_not_found() {
		let api = FakeApi::new();
		api.add_service(FILESYSTEM, FakeFilesystem::service());
		api.install();
		let fake_abort = FakeAbort::new(false);
		let abort = unsafe { Abort::from_raw(fake_abort.as_raw()) };
		let error = get_stats("file://C:\\does-not-exist.wav", &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::IoNotFound);
		assert_eq!(error.host_type.as_deref(), Some(".?AVexception_io_not_found@foobar2000_io@@"));
	}

	#[test]
	fn get_stats_without_handler() {
		FakeApi::new().install();
		let fake_abort = FakeAbort::new(false);
		let abort = unsafe { Abort::from_raw(fake_abort.as_raw()) };
		let error = get_stats("file://C:\\does-not-exist.wav", &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::IoNoHandlerForPath);
	}
}
