//! foobar2000's cancellation signal, as passed to every long-running call.

use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::OnceLock;

use crate::abi::abort_callback::{AbortCallback, AbortCallbackVtbl};
use crate::bindings::Windows::Win32::CreateEventW;
use crate::cxx::try_host;
use crate::error::{Error, Result};

/// A borrowed `abort_callback`. Never null: foobar2000 passes it by reference, and Foxide2k passes
/// it on to foobar2000 the same way.
pub struct Abort<'a> {
	raw: NonNull<AbortCallback>,
	_borrow: PhantomData<&'a AbortCallback>,
}

impl Abort<'_> {
	/// # Panics
	/// If `raw` is null.
	///
	/// # Safety
	/// `raw` must point to an `abort_callback` that outlives the returned value.
	pub unsafe fn from_raw(raw: *mut AbortCallback) -> Self {
		let raw = NonNull::new(raw).expect("an abort_callback is passed by reference, never null");
		Self { raw, _borrow: PhantomData }
	}

	/// The `abort_callback &` to pass on to foobar2000.
	pub fn as_raw(&self) -> *mut AbortCallback {
		self.raw.as_ptr()
	}

	/// Whether the operation should stop. A failing callback counts as "not aborting".
	pub fn is_aborting(&self) -> bool {
		let raw = self.raw.as_ptr();
		try_host(|| unsafe { ((*(*raw).vtbl).is_aborting)(raw) }).unwrap_or(false)
	}

	/// `Err(Error::aborted())` if the operation should stop, like the SDK's `check()`, which throws
	/// instead.
	pub fn check(&self) -> Result<()> {
		if self.is_aborting() { Err(Error::aborted()) } else { Ok(()) }
	}
}

/// The `abort_callback` that never aborts.
struct Never(AbortCallback);

// SAFETY: the vtable pointer is to a static and is never written.
unsafe impl Sync for Never {}

static NEVER: Never = Never(AbortCallback { vtbl: &raw const NEVER_VTBL });

static NEVER_VTBL: AbortCallbackVtbl =
	AbortCallbackVtbl { is_aborting: never_is_aborting, get_abort_event: never_event };

unsafe extern "C-unwind" fn never_is_aborting(_this: *const AbortCallback) -> bool {
	false
}

/// A manual-reset event that nothing sets, made on first use and kept for the process, held
/// as an integer.
unsafe extern "C-unwind" fn never_event(_this: *const AbortCallback) -> *mut c_void {
	static EVENT: OnceLock<usize> = OnceLock::new();
	*EVENT.get_or_init(|| {
		unsafe { CreateEventW(None, true, false, windows_core::PCWSTR::null()) }.0 as usize
	}) as *mut c_void
}

/// An `abort_callback` that never aborts, for calls nothing can cancel.
pub fn never() -> Abort<'static> {
	// SAFETY: `NEVER` is a static, so it outlives everything; the pointer is only read.
	unsafe { Abort::from_raw(std::ptr::from_ref(&NEVER.0).cast_mut()) }
}

#[cfg(test)]
mod tests {
	use super::{Abort, never};

	#[test]
	#[should_panic(expected = "never null")]
	fn an_abort_from_a_null_pointer_is_refused() {
		// foobar2000 passes abort_callback by reference, and get_stats hands it on as one.
		let _ = unsafe { Abort::from_raw(std::ptr::null_mut()) };
	}

	#[test]
	fn never_is_never_aborting_and_has_an_event() {
		let abort = never();
		assert!(!abort.is_aborting());
		assert!(abort.check().is_ok());
		let raw = abort.as_raw();
		let event = unsafe { ((*(*raw).vtbl).get_abort_event)(raw) };
		assert!(!event.is_null());
	}
}
