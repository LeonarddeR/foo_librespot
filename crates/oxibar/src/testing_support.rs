//! Helpers for OxiBar's own unit tests.

use crate::bindings::Windows::Win32::{
	SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
};

/// Stops Windows from showing a crash dialog when a test child process crashes on purpose.
pub fn suppress_crash_dialogs() {
	#[allow(clippy::cast_sign_loss)]
	let mode = (SEM_NOGPFAULTERRORBOX | SEM_FAILCRITICALERRORS) as u32;
	unsafe { SetErrorMode(mode) };
}
