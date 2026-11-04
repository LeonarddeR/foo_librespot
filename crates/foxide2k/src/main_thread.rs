//! Running work on foobar2000's main thread, and making modeless windows keyboard-friendly.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, PoisonError};

use crate::abi::main_thread::{
	CALLBACK_IID, MANAGER_IID, MODELESS_IID, MainThreadCallbackManagerVtbl, MainThreadCallbackVtbl,
	ModelessDialogManagerVtbl,
};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::boundary::guard;
use crate::console;
use crate::cxx::{ExceptionKind, try_host};
use crate::error::{Error, Result};
use crate::guid::Guid;
use crate::host::host;
use crate::service::{ServiceImpl, ServiceObject, new_service};

type Work = Box<dyn FnOnce() + Send>;

/// The `main_thread_callback` that carries one piece of work.
struct MainThreadWork(Mutex<Option<Work>>);

unsafe impl ServiceImpl for MainThreadWork {
	type Vtbl = MainThreadCallbackVtbl;
	const IIDS: &'static [Guid] = &[CALLBACK_IID];
	const VTBL: &'static MainThreadCallbackVtbl =
		&MainThreadCallbackVtbl { base: ServiceBaseVtbl::new::<Self>(), callback_run };
}

unsafe extern "C-unwind" fn callback_run(this: *mut ServiceBase) {
	unsafe {
		guard(|| {
			let work = ServiceObject::<MainThreadWork>::value(this)
				.0
				.lock()
				.unwrap_or_else(PoisonError::into_inner)
				.take();
			// A panic would reach foobar2000's message loop as a C++ exception. The work is the
			// component's own, so it is reported instead.
			if let Some(work) = work
				&& catch_unwind(AssertUnwindSafe(work)).is_err()
			{
				console::print("Foxide2k: work on the main thread panicked");
			}
			Ok(())
		});
	}
}

/// Runs `work` on foobar2000's main thread, soon after this returns.
pub fn run(work: Work) -> Result<()> {
	let manager = host().core_service(&MANAGER_IID).ok_or_else(|| {
		Error::new(ExceptionKind::BugCheck, "foobar2000 has no main_thread_callback_manager")
	})?;
	let raw = manager.as_raw();
	// The by-value argument: a temporary holding one reference, which the callee releases.
	let mut temporary = new_service(MainThreadWork(Mutex::new(Some(work)))).into_raw();
	try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<MainThreadCallbackManagerVtbl>();
		(vtbl.add_callback)(raw, &raw mut temporary);
	})?;
	Ok(())
}

fn modeless(add: bool, window: isize) {
	let Some(manager) = host().core_service(&MODELESS_IID) else { return };
	let raw = manager.as_raw();
	let window = window as *mut c_void;
	let _ = try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<ModelessDialogManagerVtbl>();
		if add { (vtbl.add)(raw, window) } else { (vtbl.remove)(raw, window) }
	});
}

/// Lets foobar2000's message loop handle Tab and access keys in a modeless window. Main thread
/// only.
pub fn add_modeless(window: isize) {
	modeless(true, window);
}

/// Undoes [`add_modeless`], before the window is destroyed. Main thread only.
pub fn remove_modeless(window: isize) {
	modeless(false, window);
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

	use super::{add_modeless, remove_modeless, run};
	use crate::host::host;
	use crate::testing::FakeApi;

	#[test]
	fn main_thread_work_runs_once_when_the_host_runs_it() {
		let api = FakeApi::new();
		api.install();
		let runs = Arc::new(AtomicUsize::new(0));
		let counted = Arc::clone(&runs);
		run(Box::new(move || {
			counted.fetch_add(1, Ordering::SeqCst);
		}))
		.unwrap();
		assert_eq!(runs.load(Ordering::SeqCst), 0);
		assert_eq!(api.run_main_thread(), 1);
		assert_eq!(api.run_main_thread(), 0);
		assert_eq!(runs.load(Ordering::SeqCst), 1);
	}

	/// Sets its flag when dropped.
	struct DropFlag(Arc<AtomicBool>);

	impl Drop for DropFlag {
		fn drop(&mut self) {
			self.0.store(true, Ordering::SeqCst);
		}
	}

	#[test]
	fn main_thread_work_is_freed_when_the_host_drops_it() {
		let api = FakeApi::new();
		api.install();
		let dropped = Arc::new(AtomicBool::new(false));
		let flag = DropFlag(Arc::clone(&dropped));
		run(Box::new(move || drop(flag))).unwrap();
		assert!(!dropped.load(Ordering::SeqCst));
		api.discard_main_thread();
		assert!(dropped.load(Ordering::SeqCst), "the callback service kept a reference");
	}

	#[test]
	fn main_thread_without_host_fails() {
		assert!(run(Box::new(|| {})).is_err());
	}

	#[test]
	fn modeless_windows_are_added_and_removed() {
		let api = FakeApi::new();
		api.install();
		add_modeless(0x1234);
		assert_eq!(api.modeless_windows(), [0x1234]);
		remove_modeless(0x1234);
		assert!(api.modeless_windows().is_empty());
		assert_eq!(host().main_window(), 0);
	}
}
