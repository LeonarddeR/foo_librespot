//! foobar2000's cancellation signal, as passed to every long-running call.

use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::abi::abort_callback::AbortCallback;
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

#[cfg(test)]
mod tests {
	use super::Abort;

	#[test]
	#[should_panic(expected = "never null")]
	fn an_abort_from_a_null_pointer_is_refused() {
		// foobar2000 passes abort_callback by reference, and get_stats hands it on as one.
		let _ = unsafe { Abort::from_raw(std::ptr::null_mut()) };
	}
}
