//! `try_host`: catching foobar2000's C++ exceptions in pure Rust.
//!
//! The structure follows Rust's own `library/panic_unwind/src/seh.rs` and lithium's
//! `src/backend/seh.rs` (MIT OR Apache-2.0); the catching technique is MSVC's
//! `__try`/`__except`, as used by microseh (MIT).

use std::ffi::{CStr, c_char, c_void};
use std::fmt;

use crate::bindings::Windows::Win32::{
	CONTEXT, DISPATCHER_CONTEXT, EXCEPTION_DISPOSITION, EXCEPTION_RECORD, ExceptionContinueSearch,
	RtlUnwindEx,
};
use crate::cxx::kind::ExceptionKind;
use crate::cxx::rtti::{CatchableType, ThrowInfo};
use crate::cxx::throw::throw;
use crate::cxx::trampoline::{SLOT_OFFSET, landing, oxibar_cxx_try};

/// The exception code `_CxxThrowException` raises: "msc" in ASCII.
const CXX_EXCEPTION_CODE: u32 = 0xE06D7363;
/// `EH_MAGIC_NUMBER1` and its later revisions, the first exception parameter of a C++ throw.
const EH_MAGIC_NUMBERS: std::ops::RangeInclusive<usize> = 0x19930520..=0x19930522;
/// `EXCEPTION_UNWINDING | EXCEPTION_EXIT_UNWIND | EXCEPTION_TARGET_UNWIND | EXCEPTION_COLLIDED_UNWIND`.
const UNWIND_FLAGS: u32 = 0x66;
const STD_EXCEPTION: &str = ".?AVexception@std@@";
/// The type name of a Rust panic on MSVC, which must keep unwinding to `catch_unwind`.
const RUST_PANIC: &str = "rust_panic";

/// A C++ exception thrown by foobar2000 and caught by [`try_host`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostException {
	/// The most-derived class OxiBar recognises. A `std::exception` subclass it doesn't know,
	/// or a type that isn't a `std::exception` at all, gives [`ExceptionKind::Std`].
	pub kind: ExceptionKind,
	/// The MSVC RTTI name of the class that was actually thrown.
	pub type_name: String,
	/// `what()`, converted from UTF-8 lossily.
	pub message: String,
}

impl HostException {
	/// Throws the same kind of exception again, with the same message.
	///
	/// # Safety
	/// As for [`throw`].
	pub unsafe fn rethrow(self) -> ! {
		unsafe { throw(self.kind, &self.message) }
	}
}

impl fmt::Display for HostException {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{} ({})", self.message, self.type_name)
	}
}

impl std::error::Error for HostException {}

/// Where the handler leaves the exception it caught.
pub(crate) struct CatchSlot {
	caught: Option<HostException>,
}

struct Payload<F, R> {
	f: Option<F>,
	result: Option<R>,
}

unsafe extern "C-unwind" fn call_closure<F: FnOnce() -> R, R>(data: *mut c_void) {
	let payload = unsafe { &mut *data.cast::<Payload<F, R>>() };
	if let Some(f) = payload.f.take() {
		payload.result = Some(f());
	}
}

/// Runs `f`, catching any C++ exception thrown inside it, such as one thrown by foobar2000.
///
/// Rust panics and non-C++ structured exceptions (access violations) are not caught: they keep
/// propagating as if `try_host` weren't there.
pub fn try_host<R>(f: impl FnOnce() -> R) -> Result<R, HostException> {
	try_host_impl(f)
}

fn try_host_impl<F: FnOnce() -> R, R>(f: F) -> Result<R, HostException> {
	let mut payload = Payload { f: Some(f), result: None };
	let mut slot = CatchSlot { caught: None };
	let caught = unsafe {
		oxibar_cxx_try((&raw mut payload).cast(), call_closure::<F, R>, (&raw mut slot).cast())
	};
	if caught == 0 {
		Ok(payload.result.take().expect("the closure ran to completion"))
	} else {
		Err(slot.caught.take().unwrap_or_else(|| HostException {
			kind: ExceptionKind::Std,
			type_name: String::new(),
			message: "unknown C++ exception".to_owned(),
		}))
	}
}

/// The language-specific handler named in `oxibar_cxx_try`'s unwind info. Windows calls it while
/// searching for a handler for an exception raised inside the closure.
// "system-unwind": RtlUnwindEx unwinds from its caller, so it unwinds through this frame too.
pub(crate) unsafe extern "system-unwind" fn oxibar_cxx_handler(
	record: *mut EXCEPTION_RECORD,
	establisher_frame: *mut c_void,
	_context: *mut CONTEXT,
	dispatcher: *mut DISPATCHER_CONTEXT,
) -> EXCEPTION_DISPOSITION {
	unsafe {
		if (*record).ExceptionFlags & UNWIND_FLAGS != 0 {
			return ExceptionContinueSearch;
		}
		// A panic must never escape an exception handler; treat it as "not ours".
		let Ok(Some(caught)) = std::panic::catch_unwind(|| inspect(&*record)) else {
			return ExceptionContinueSearch;
		};
		let slot = *establisher_frame.byte_add(SLOT_OFFSET).cast::<*mut CatchSlot>();
		(*slot).caught = Some(caught.exception);
		// The thrown object lives in the throwing frame, which is still on the stack; destroy it
		// now, as the C++ runtime would after its catch block.
		if let Some(destructor) = caught.destructor {
			destructor(caught.object);
		}
		RtlUnwindEx(
			Some(establisher_frame.cast_const()),
			Some(landing()),
			Some(record.cast_const()),
			std::ptr::without_provenance(1),
			(*dispatcher).ContextRecord,
			Some((*dispatcher).HistoryTable.cast_const()),
		);
		// RtlUnwindEx resumes at the landing pad and never returns here.
		std::process::abort()
	}
}

struct Caught {
	exception: HostException,
	object: *mut c_void,
	destructor: Option<unsafe extern "C" fn(*mut c_void)>,
}

unsafe extern "C" {
	// vcruntime's per-thread pointer to the C++ exception record being handled, the one a `throw;`
	// rethrows. msvcp's std::current_exception reads it the same way.
	fn __current_exception() -> *mut *mut c_void;
}

fn is_cxx_record(record: &EXCEPTION_RECORD) -> bool {
	record.ExceptionCode == CXX_EXCEPTION_CODE
		&& record.NumberParameters == 4
		&& EH_MAGIC_NUMBERS.contains(&record.ExceptionInformation[0])
}

/// Decides whether `record` is a C++ exception `try_host` should catch, and reads it.
unsafe fn inspect(record: &EXCEPTION_RECORD) -> Option<Caught> {
	if !is_cxx_record(record) {
		return None;
	}
	let mut record = record;
	// A bare `throw;` raises a record with no object and no ThrowInfo. C++ frame handlers then
	// handle vcruntime's current exception instead; so must we, or the rethrow escapes try_host
	// and aborts in catch_unwind.
	let rethrown = record.ExceptionInformation[1] == 0 && record.ExceptionInformation[2] == 0;
	if rethrown {
		let current = unsafe { *__current_exception() }.cast::<EXCEPTION_RECORD>();
		if current.is_null() {
			return None;
		}
		record = unsafe { &*current };
		if !is_cxx_record(record) {
			return None;
		}
	}
	let object = record.ExceptionInformation[1] as *mut c_void;
	let throw_info = record.ExceptionInformation[2] as *const ThrowInfo;
	let image_base = record.ExceptionInformation[3];
	if object.is_null() || throw_info.is_null() {
		return None;
	}
	unsafe {
		let at = |offset: i32| image_base.wrapping_add_signed(offset as isize);
		let array = at((*throw_info).catchable_type_array());
		let count = usize::try_from(*(array as *const i32)).ok()?;
		let mut classes = Vec::with_capacity(count);
		for index in 0..count {
			let entry = *((array + 4 + index * 4) as *const i32);
			let catchable = &*(at(entry) as *const CatchableType);
			// TypeDescriptor: vftable pointer, spare pointer, then the NUL-terminated name.
			let name = CStr::from_ptr((at(catchable.type_descriptor()) + 16) as *const c_char);
			classes.push((name.to_string_lossy().into_owned(), catchable));
		}
		if classes.iter().any(|(name, _)| name == RUST_PANIC) {
			return None;
		}
		let type_name = classes.first().map(|(name, _)| name.clone()).unwrap_or_default();
		let kind = classes.iter().find_map(|(name, _)| ExceptionKind::from_type_name(name));
		let std_base =
			classes.iter().find(|(name, _)| name == STD_EXCEPTION).map(|(_, catchable)| *catchable);
		let message = match std_base {
			// A non-negative pdisp means std::exception is a virtual base, which no foobar2000
			// exception has; leave the message out rather than guess its address.
			Some(catchable) if catchable.this_displacement.pdisp < 0 => {
				let base = object.byte_offset(catchable.this_displacement.mdisp as isize);
				let vtbl =
					*base.cast::<*const unsafe extern "C" fn(*const c_void) -> *const c_char>();
				let what = (*vtbl.add(1))(base.cast_const());
				if what.is_null() {
					String::new()
				} else {
					CStr::from_ptr(what).to_string_lossy().into_owned()
				}
			}
			Some(_) => String::new(),
			None => format!("non-standard C++ exception {type_name}"),
		};
		let unwind = (*throw_info).unwind();
		// A rethrown object still belongs to the catch block that rethrew it, and vcruntime may
		// destroy it when that block's frame unwinds. Destroying it here too would be a double
		// destruction; leaving it at worst leaks one exception object.
		let destructor = (unwind != 0 && !rethrown)
			.then(|| std::mem::transmute::<usize, unsafe extern "C" fn(*mut c_void)>(at(unwind)));
		Some(Caught {
			exception: HostException {
				kind: kind.unwrap_or(ExceptionKind::Std),
				type_name,
				message,
			},
			object,
			destructor,
		})
	}
}

#[cfg(test)]
mod tests {
	use std::process::Command;
	use std::sync::atomic::{AtomicUsize, Ordering};

	use super::try_host;
	use crate::cxx::kind::ExceptionKind;
	use crate::cxx::throw::{self, throw};

	struct DropCounter<'a>(&'a AtomicUsize);

	impl Drop for DropCounter<'_> {
		fn drop(&mut self) {
			self.0.fetch_add(1, Ordering::SeqCst);
		}
	}

	#[test]
	fn returns_ok_when_closure_completes() {
		assert_eq!(try_host(|| 42).unwrap(), 42);
	}

	#[test]
	fn catches_aborted_with_message() {
		let error = try_host(|| unsafe { throw(ExceptionKind::Aborted, "stop") }).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Aborted);
		assert_eq!(error.message, "stop");
		assert_eq!(error.type_name, ".?AVexception_aborted@foobar2000_io@@");
	}

	#[test]
	fn catches_most_derived_kind() {
		let error =
			try_host(|| unsafe { throw(ExceptionKind::IoUnsupportedFormat, "fmt") }).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::IoUnsupportedFormat);
	}

	#[test]
	fn drops_rust_values_between_throw_and_catch() {
		let drops = AtomicUsize::new(0);
		let result = try_host(|| {
			let _guard = DropCounter(&drops);
			unsafe { throw(ExceptionKind::Io, "io") }
		});
		assert!(result.is_err());
		assert_eq!(drops.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn destroys_exception_object_once() {
		let before = throw::destroyed_on_this_thread();
		let result = try_host(|| unsafe { throw(ExceptionKind::Io, "io") });
		assert!(result.is_err());
		assert_eq!(throw::destroyed_on_this_thread() - before, 1);
	}

	#[test]
	fn nested_try_host() {
		let outer = try_host(|| try_host(|| unsafe { throw(ExceptionKind::IoData, "inner") }));
		let inner = outer.unwrap().unwrap_err();
		assert_eq!(inner.kind, ExceptionKind::IoData);
		assert_eq!(inner.message, "inner");
	}

	#[test]
	fn rethrow_preserves_kind_and_message() {
		let outer = try_host(|| {
			try_host(|| unsafe { throw(ExceptionKind::IoNotFound, "x") })
				.unwrap_or_else(|error| unsafe { error.rethrow() })
		});
		let error = outer.unwrap_err();
		assert_eq!(error.kind, ExceptionKind::IoNotFound);
		assert_eq!(error.message, "x");
	}

	#[test]
	fn rust_panic_passes_through_try_host() {
		let result = std::panic::catch_unwind(|| try_host(|| panic!("p")));
		let payload = result.unwrap_err();
		assert_eq!(*payload.downcast_ref::<&str>().unwrap(), "p");
	}

	#[test]
	#[ignore = "child process of access_violation_is_not_caught"]
	fn child_access_violation() {
		if std::env::var_os("OXIBAR_CHILD").is_none() {
			return;
		}
		crate::testing_support::suppress_crash_dialogs();
		// A small non-null address: read_volatile's debug checks reject only null and misalignment.
		let _ = try_host(|| unsafe { std::ptr::read_volatile(0x10 as *const u8) });
	}

	#[test]
	fn access_violation_is_not_caught() {
		let status = Command::new(std::env::current_exe().unwrap())
			.args([
				"--exact",
				"cxx::catch::tests::child_access_violation",
				"--ignored",
				"--nocapture",
			])
			.env("OXIBAR_CHILD", "1")
			.status()
			.unwrap();
		#[allow(clippy::cast_possible_wrap)]
		let access_violation = 0xC0000005_u32 as i32;
		assert_eq!(status.code(), Some(access_violation));
	}

	unsafe extern "C" {
		// vcruntime's per-thread "exception being handled", which a C++ `throw;` rethrows.
		fn __current_exception() -> *mut *mut std::ffi::c_void;
	}

	#[test]
	fn catches_rethrow_of_the_current_exception() {
		use crate::bindings::Windows::Win32::{
			EXCEPTION_NONCONTINUABLE, EXCEPTION_RECORD, RaiseException,
		};
		use crate::cxx::rtti::{image_base, throw_info};
		use crate::cxx::throw::{StdException, destroy_exception};

		// Inside a C++ catch block, vcruntime records the exception being handled. `throw;` then
		// raises a record with no object and no ThrowInfo, meaning "that exception again".
		let mut object = StdException::new("again");
		let mut information = [0usize; 15];
		information[0] = 0x19930520;
		information[1] = (&raw mut object) as usize;
		information[2] = std::ptr::from_ref(throw_info(ExceptionKind::Aborted)) as usize;
		information[3] = image_base();
		#[allow(clippy::cast_sign_loss)]
		let original = EXCEPTION_RECORD {
			ExceptionCode: 0xE06D7363,
			ExceptionFlags: EXCEPTION_NONCONTINUABLE as u32,
			ExceptionRecord: std::ptr::null_mut(),
			ExceptionAddress: std::ptr::null_mut(),
			NumberParameters: 4,
			ExceptionInformation: information,
		};
		let result = unsafe {
			let current = __current_exception();
			let saved = *current;
			*current = (&raw const original).cast_mut().cast();
			#[allow(clippy::cast_sign_loss)]
			let result = try_host(|| {
				RaiseException(
					0xE06D7363,
					EXCEPTION_NONCONTINUABLE as u32,
					Some(&[0x19930520, 0, 0, 0]),
				);
			});
			*current = saved;
			result
		};
		let error = result.unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Aborted);
		assert_eq!(error.message, "again");
		// The catch block that rethrew owns the original object, so the test destroys it.
		unsafe { destroy_exception(&raw mut object) };
	}

	#[test]
	fn throw_and_catch_on_many_threads() {
		let handles: Vec<_> = (0..8)
			.map(|_| {
				std::thread::spawn(|| {
					for round in 0..1000 {
						let error =
							try_host(|| unsafe { throw(ExceptionKind::IoNotFound, "busy") })
								.unwrap_err();
						assert_eq!(error.kind, ExceptionKind::IoNotFound, "round {round}");
					}
				})
			})
			.collect();
		for handle in handles {
			handle.join().unwrap();
		}
	}
}
