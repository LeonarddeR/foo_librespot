//! Windows for dialog tests: a hidden owner, the message pump, and the messages a user's
//! clicks and keys would send.

use crate::bindings::Windows::Win32::{
	COINIT_APARTMENTTHREADED, CoInitializeEx, DLGC_WANTALLKEYS, DispatchMessageW, HWND, LPARAM,
	MSG, PM_REMOVE, PeekMessageW, SendMessageW, TranslateMessage, WM_CLOSE, WM_COMMAND,
	WM_GETDLGCODE, WPARAM,
};
use crate::dialog::host::{Dialog, DialogHandler, create_modeless_dialog};
use crate::dialog::style::WS_POPUP;
use crate::dialog::template::DialogTemplate;

/// The owner's handler, which does nothing.
struct Owner;

impl DialogHandler for Owner {
	fn init(&mut self, _dialog: &Dialog) -> Option<u16> {
		None
	}

	fn command(&mut self, _dialog: &Dialog, _id: u16, _notification: u16) {}

	fn refresh(&mut self, _dialog: &Dialog) {}
}

/// A hidden dialog to own the dialogs under test, as foobar2000's Preferences window owns its
/// pages. It is a dialog like that window: when Windows activates it, a dialog gives focus back
/// to the control that had it, where a plain window takes focus itself. Also initialises COM on
/// this thread, as foobar2000's main thread has it, for live regions.
pub fn hidden_window() -> isize {
	unsafe {
		let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED as u32);
	}
	let template = DialogTemplate {
		style: WS_POPUP,
		title: String::new(),
		cx: 100,
		cy: 100,
		font: (8, "MS Shell Dlg".to_owned()),
		items: Vec::new(),
	};
	create_modeless_dialog(0, &template, Box::new(Owner)).expect("a hidden owner dialog").hwnd()
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

/// Whether control `id` takes Escape and Enter for itself, as a multi-line edit does. The
/// dialog manager asks each control this before it turns those keys into Cancel and OK, so a
/// control that wants all keys keeps Escape from closing the window it is in.
pub fn wants_all_keys(dialog: &Dialog, id: u16) -> bool {
	let code = unsafe {
		SendMessageW(dialog.control(id), WM_GETDLGCODE.cast_unsigned(), WPARAM(0), LPARAM(0))
	};
	code.0 & DLGC_WANTALLKEYS as isize != 0
}

/// Sends `WM_CLOSE`, as the close box and Alt+F4 do. The dialog manager turns it into a
/// posted `IDCANCEL`, so pump messages afterwards.
pub fn send_close(dialog: &Dialog) {
	unsafe {
		SendMessageW(HWND(dialog.hwnd() as _), WM_CLOSE.cast_unsigned(), WPARAM(0), LPARAM(0))
	};
}
