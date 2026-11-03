//! Throwing foobar2000's C++ exceptions from Rust.

use std::ffi::{CString, c_char, c_void};

use crate::cxx::kind::ExceptionKind;
use crate::cxx::rtti;

/// vcruntime's `__std_exception_data`, the data members of MSVC's `std::exception`.
#[repr(C)]
pub(crate) struct StdExceptionData {
	pub what: *const c_char,
	pub do_free: bool,
}

/// The vtable of MSVC's `std::exception`: the scalar deleting destructor, then `what()`.
#[repr(C)]
pub(crate) struct StdExceptionVtbl {
	pub scalar_deleting_destructor:
		unsafe extern "C" fn(this: *mut StdException, flags: u32) -> *mut StdException,
	pub what: unsafe extern "C" fn(this: *const StdException) -> *const c_char,
}

/// An object with the layout and behaviour of MSVC's `std::exception`. Every foobar2000
/// exception class has exactly this layout, so the same object is thrown as any of them; only
/// the ThrowInfo differs.
#[repr(C)]
pub struct StdException {
	pub(crate) vtbl: *const StdExceptionVtbl,
	pub(crate) data: StdExceptionData,
}

unsafe extern "C" {
	// vcruntime's own helpers, so that copies made by foobar2000 with std::exception's copy
	// constructor allocate and free the message exactly as ours do.
	fn __std_exception_copy(from: *const StdExceptionData, to: *mut StdExceptionData);
	fn __std_exception_destroy(data: *mut StdExceptionData);

	fn free(block: *mut c_void);
}

unsafe extern "C-unwind" {
	// Declared C-unwind: the exception it raises unwinds through the Rust frames that call it.
	fn _CxxThrowException(object: *mut c_void, throw_info: *const rtti::ThrowInfo) -> !;
}

static VTBL: StdExceptionVtbl = StdExceptionVtbl { scalar_deleting_destructor, what };

#[cfg(test)]
thread_local! {
	// Per thread, so tests running in parallel threads don't see each other's exceptions.
	static DESTROYED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many exception objects this thread has destroyed so far.
#[cfg(test)]
pub(crate) fn destroyed_on_this_thread() -> usize {
	DESTROYED.get()
}

impl StdException {
	/// A `std::exception` owning a copy of `message`. Interior NUL bytes end the message early,
	/// as they would in C++.
	pub fn new(message: &str) -> Self {
		let message = CString::new(message).unwrap_or_else(|error| {
			let end = error.nul_position();
			CString::new(&message.as_bytes()[..end]).expect("no NUL before the first NUL")
		});
		// With do_free set, __std_exception_copy allocates its own copy of the string.
		let source = StdExceptionData { what: message.as_ptr(), do_free: true };
		let mut data = StdExceptionData { what: std::ptr::null(), do_free: false };
		unsafe { __std_exception_copy(&raw const source, &raw mut data) };
		Self { vtbl: &raw const VTBL, data }
	}

	/// `what()`, through the vtable as C++ would call it.
	pub fn what(&self) -> *const c_char {
		unsafe { ((*self.vtbl).what)(self) }
	}
}

/// The destructor ThrowInfo names: frees the message.
///
/// # Safety
/// `this` must point to a live `StdException`, which must not be used again.
pub(crate) unsafe extern "C" fn destroy_exception(this: *mut StdException) {
	unsafe { __std_exception_destroy(&raw mut (*this).data) };
	#[cfg(test)]
	DESTROYED.set(DESTROYED.get() + 1);
}

/// The copy constructor the catchable types name, used when C++ catches by value or copies the
/// exception into a `std::exception_ptr`.
///
/// # Safety
/// `to` must be writable storage for a `StdException`; `from` must be live.
pub(crate) unsafe extern "C" fn copy_exception(
	to: *mut StdException,
	from: *const StdException,
) -> *mut StdException {
	unsafe {
		let mut data = StdExceptionData { what: std::ptr::null(), do_free: false };
		__std_exception_copy(&raw const (*from).data, &raw mut data);
		to.write(StdException { vtbl: &raw const VTBL, data });
	}
	to
}

unsafe extern "C" fn scalar_deleting_destructor(
	this: *mut StdException,
	flags: u32,
) -> *mut StdException {
	unsafe {
		destroy_exception(this);
		// Bit 0 asks the destructor to free the object too, as `delete` would.
		if flags & 1 != 0 {
			free(this.cast());
		}
	}
	this
}

unsafe extern "C" fn what(this: *const StdException) -> *const c_char {
	let what = unsafe { (*this).data.what };
	if what.is_null() { c"Unknown exception".as_ptr() } else { what }
}

pub(crate) fn destructor_address() -> usize {
	destroy_exception as unsafe extern "C" fn(*mut StdException) as usize
}

pub(crate) fn copy_function_address() -> usize {
	copy_exception
		as unsafe extern "C" fn(*mut StdException, *const StdException) -> *mut StdException as usize
}

/// Throws a foobar2000 C++ exception of class `kind` with `message` as its `what()`.
///
/// # Safety
/// Must not be called inside `catch_unwind`: Rust aborts when a foreign exception reaches it.
/// Every frame between here and the C++ (or `try_host`) catch must allow unwinding, which means
/// they are Rust frames or `extern "C-unwind"` functions.
pub unsafe fn throw(kind: ExceptionKind, message: &str) -> ! {
	let mut exception = StdException::new(message);
	// MSVC keeps the thrown object in the throwing frame until the catch completes, so this
	// local outlives every handler.
	unsafe { _CxxThrowException((&raw mut exception).cast(), rtti::throw_info(kind)) }
}

#[cfg(test)]
mod tests {
	use std::ffi::CStr;
	use std::mem::MaybeUninit;

	use super::{StdException, copy_exception, destroy_exception};

	#[test]
	fn std_exception_what_returns_message() {
		let mut exception = StdException::new("boom");
		let what = unsafe { CStr::from_ptr(exception.what()) };
		assert_eq!(what.to_str().unwrap(), "boom");
		unsafe { destroy_exception(&raw mut exception) };
	}

	#[test]
	fn copy_function_is_deep() {
		let mut original = StdException::new("boom");
		let mut copy = MaybeUninit::<StdException>::uninit();
		unsafe {
			copy_exception(copy.as_mut_ptr(), &raw const original);
			destroy_exception(&raw mut original);
			let what = CStr::from_ptr((*copy.as_ptr()).what());
			assert_eq!(what.to_str().unwrap(), "boom");
			destroy_exception(copy.as_mut_ptr());
		}
	}
}
