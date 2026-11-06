//! Controlling foobar2000's playback. Main thread only: from another thread, use
//! [`main_thread::run`](crate::main_thread::run).

use crate::abi::playback_control::{IID, PlaybackControlVtbl};
use crate::cxx::{ExceptionKind, try_host};
use crate::error::{Error, Result};
use crate::host::host;

/// Stops playback, as the Stop button does. Main thread only.
pub fn stop() -> Result<()> {
	let control = host()
		.core_service(&IID)
		.ok_or_else(|| Error::new(ExceptionKind::BugCheck, "foobar2000 has no playback_control"))?;
	let raw = control.as_raw();
	try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<PlaybackControlVtbl>();
		(vtbl.stop)(raw);
	})?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::stop;
	use crate::testing::FakeApi;

	#[test]
	fn stop_reaches_the_host_once() {
		let api = FakeApi::new();
		api.install();
		assert_eq!(api.playback_stops(), 0);
		stop().unwrap();
		assert_eq!(api.playback_stops(), 1);
	}

	#[test]
	fn stop_without_host_fails() {
		assert!(stop().is_err());
	}
}
