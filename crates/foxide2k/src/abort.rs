//! foobar2000's cancellation signal, as passed to every long-running call.

use std::marker::PhantomData;

use crate::abi::abort_callback::AbortCallback;
use crate::cxx::try_host;
use crate::error::{Error, Result};

/// A borrowed `abort_callback`.
pub struct Abort<'a> {
	raw: *mut AbortCallback,
	_borrow: PhantomData<&'a AbortCallback>,
}

impl Abort<'_> {
	/// # Safety
	/// `raw` must be null or point to an `abort_callback` that outlives the returned value.
	pub unsafe fn from_raw(raw: *mut AbortCallback) -> Self {
		Self { raw, _borrow: PhantomData }
	}

	/// The `abort_callback &` to pass on to foobar2000.
	pub fn as_raw(&self) -> *mut AbortCallback {
		self.raw
	}

	/// Whether the operation should stop. A null or failing callback counts as "not aborting".
	pub fn is_aborting(&self) -> bool {
		if self.raw.is_null() {
			return false;
		}
		try_host(|| unsafe { ((*(*self.raw).vtbl).is_aborting)(self.raw) }).unwrap_or(false)
	}

	/// `Err(Error::aborted())` if the operation should stop, like the SDK's `check()`, which throws
	/// instead.
	pub fn check(&self) -> Result<()> {
		if self.is_aborting() { Err(Error::aborted()) } else { Ok(()) }
	}
}
