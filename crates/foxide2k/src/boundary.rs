//! The guard every Rust-implemented vtable slot runs inside.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::cxx::{ExceptionKind, throw};
use crate::error::Result;

/// Runs a vtable slot's body and reports its outcome to foobar2000 the C++ way: `Ok` returns the
/// value, `Err` throws the matching foobar2000 exception, and a panic throws
/// `pfc::exception_bug_check` instead of unwinding into foobar2000.
///
/// # Safety
/// Must be called directly from an `extern "C-unwind"` slot that foobar2000 called, with no
/// `catch_unwind` around it.
pub unsafe fn guard<T>(f: impl FnOnce() -> Result<T>) -> T {
	// The throw must happen after catch_unwind has returned: inside it, a C++ exception aborts.
	let (kind, message) = match catch_unwind(AssertUnwindSafe(f)) {
		Ok(Ok(value)) => return value,
		Ok(Err(error)) => (error.kind, error.message.into_owned()),
		Err(payload) => (
			ExceptionKind::BugCheck,
			format!("panic in foobar2000 component: {}", panic_message(&*payload)),
		),
	};
	unsafe { throw(kind, &message) }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
	payload
		.downcast_ref::<&str>()
		.copied()
		.or_else(|| payload.downcast_ref::<String>().map(String::as_str))
		.unwrap_or("unknown panic")
}

#[cfg(test)]
mod tests {
	use super::guard;
	use crate::cxx::{ExceptionKind, throw, try_host};
	use crate::error::{Error, Result};

	// Only ever called from Rust, so the Rust fn pointer argument is fine.
	#[allow(improper_ctypes_definitions)]
	unsafe extern "C-unwind" fn shim(body: fn() -> Result<i32>) -> i32 {
		unsafe { guard(body) }
	}

	#[test]
	fn guard_returns_value() {
		assert_eq!(try_host(|| unsafe { shim(|| Ok(7)) }).unwrap(), 7);
	}

	#[test]
	fn guard_throws_error_kind_and_message() {
		let error =
			try_host(|| unsafe { shim(|| Err(Error::unsupported_format("nope"))) }).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::IoUnsupportedFormat);
		assert_eq!(error.message, "nope");
	}

	#[test]
	fn guard_turns_panic_into_bug_check() {
		let error = try_host(|| unsafe { shim(|| panic!("kaboom")) }).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::BugCheck);
		assert!(error.message.contains("kaboom"), "{}", error.message);
	}

	#[test]
	fn guard_rethrows_host_exception_kind() {
		let error = try_host(|| unsafe {
			shim(|| {
				try_host(|| throw(ExceptionKind::Aborted, "u"))?;
				Ok(0)
			})
		})
		.unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Aborted);
		assert_eq!(error.message, "u");
	}

	#[test]
	fn error_from_host_exception_keeps_type_name() {
		let host = try_host(|| unsafe { throw(ExceptionKind::IoNotFound, "gone") }).unwrap_err();
		let error = Error::from(host);
		assert_eq!(error.kind, ExceptionKind::IoNotFound);
		assert_eq!(error.host_type.as_deref(), Some(".?AVexception_io_not_found@foobar2000_io@@"));
	}
}
