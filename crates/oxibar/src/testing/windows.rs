//! Windows for dialog tests: a hidden owner, the message pump, and the messages a user's
//! clicks and keys would send.

use crate::bindings::Windows::Win32::{
	COINIT_APARTMENTTHREADED, CoInitializeEx, CreateWindowExW, DispatchMessageW, HWND, LPARAM, MSG,
	PM_REMOVE, PeekMessageW, SendMessageW, TranslateMessage, WM_CLOSE, WM_COMMAND, WPARAM,
	WS_POPUP,
};
use crate::dialog::host::Dialog;

/// A hidden top-level window to own the dialogs under test. Also initialises COM on this
/// thread, as foobar2000's main thread has it, for live regions.
pub fn hidden_window() -> isize {
	unsafe {
		let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED as u32);
		CreateWindowExW(
			0,
			windows_core::w!("STATIC"),
			windows_core::w!(""),
			WS_POPUP,
			0,
			0,
			100,
			100,
			None,
			None,
			None,
			None,
		)
		.0 as isize
	}
}

/// Handles every message waiting for this thread, such as posted refreshes.
pub fn pump_messages() {
	let mut message = MSG::default();
	while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE.cast_unsigned()) }.as_bool()
	{
		unsafe {
			let _ = TranslateMessage(&raw const message);
			DispatchMessageW(&raw const message);
		}
	}
}

/// Sends `WM_COMMAND` from control `id`, as a click or a selection change does.
pub fn send_command(dialog: &Dialog, id: u16, notification: u16) {
	let wparam = usize::from(id) | (usize::from(notification) << 16);
	let control = dialog.control(id);
	unsafe {
		SendMessageW(
			HWND(dialog.hwnd() as _),
			WM_COMMAND.cast_unsigned(),
			WPARAM(wparam),
			LPARAM(control.0 as isize),
		)
	};
}

/// Sends `WM_CLOSE`, as the close box and Alt+F4 do. The dialog manager turns it into a
/// posted `IDCANCEL`, so pump messages afterwards.
pub fn send_close(dialog: &Dialog) {
	unsafe {
		SendMessageW(HWND(dialog.hwnd() as _), WM_CLOSE.cast_unsigned(), WPARAM(0), LPARAM(0))
	};
}
