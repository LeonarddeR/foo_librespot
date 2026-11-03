//! Printing to foobar2000's console.

use std::ffi::CString;

use crate::abi::console::{ConsoleReceiverVtbl, IID};
use crate::cxx::try_host;
use crate::host::host;

/// Prints one message to foobar2000's console, like the SDK's `console::print`. Does nothing
/// before foobar2000 has made its services available.
pub fn print(message: &str) {
	let host = host();
	if !host.is_available() {
		return;
	}
	let Ok(message) = CString::new(message.replace('\0', " ")) else { return };
	for receiver in host.enumerate(&IID) {
		let raw = receiver.as_raw();
		// A failing receiver must not stop the others, as in the SDK.
		let _ = try_host(|| unsafe {
			let vtbl = &*(*raw).vtbl.cast::<ConsoleReceiverVtbl>();
			(vtbl.print)(raw, message.as_ptr(), message.as_bytes().len());
		});
	}
}
