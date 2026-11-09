//! Windows for dialog tests: a hidden owner, the message pump, and the messages a user's
//! clicks and keys would send.

use crate::bindings::Windows::Win32::{
	CB_GETEDITSEL, COINIT_APARTMENTTHREADED, CoInitializeEx, DLGC_WANTALLKEYS, DispatchMessageW,
	HWND, LPARAM, MSG, NMHDR, PM_REMOVE, PeekMessageW, SendMessageW, TranslateMessage, WM_CLOSE,
	WM_COMMAND, WM_GETDLGCODE, WM_NOTIFY, WPARAM,
};
use crate::host::{Dialog, DialogHandler, create_modeless_dialog};
use crate::style::WS_POPUP;
use crate::template::{DialogTemplate, shell_font};

/// The owner's handler, which does nothing.
struct Owner;

impl DialogHandler for Owner {
	fn init(&mut self, _dialog: &Dialog) -> Option<u16> {
		None
	}

	fn command(&mut self, _dialog: &Dialog, _id: u16, _notification: u16) {}

	fn refresh(&mut self, _dialog: &Dialog) {}
}

/// A hidden dialog to own the dialogs under test, as a host program's settings dialog owns its
/// pages. It is a dialog like that owner: when Windows activates it, a dialog gives focus back
/// to the control that had it, where a plain window takes focus itself. Also initialises COM on
/// this thread, which live regions need.
pub fn hidden_window() -> isize {
	unsafe {
		let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED as u32);
	}
	let template = DialogTemplate {
		style: WS_POPUP,
		title: String::new(),
		cx: 100,
		cy: 100,
		font: shell_font(),
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

/// Runs `look` on `dialog`'s handler, as the dialog runs it for one of its own messages, and
/// answers what `look` returns. `None` when the dialog is gone, its handler is busy with a message,
/// or `look` panicked. A handler is a [`std::any::Any`], so `look` can downcast it to its type.
pub fn with_handler<R>(
	dialog: &Dialog,
	look: impl FnOnce(&mut dyn DialogHandler, &Dialog) -> R,
) -> Option<R> {
	crate::host::call(HWND(dialog.hwnd() as _), look)
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

/// Sends `WM_NOTIFY` from control `id`, as a double click in a list view does.
pub fn send_notify(dialog: &Dialog, id: u16, code: i32) {
	let header =
		NMHDR { hwndFrom: dialog.control(id), idFrom: usize::from(id), code: code.cast_unsigned() };
	unsafe {
		SendMessageW(
			HWND(dialog.hwnd() as _),
			WM_NOTIFY.cast_unsigned(),
			WPARAM(usize::from(id)),
			LPARAM((&raw const header) as isize),
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

/// Where the selected part of editable combo box `id`'s text starts and ends, in UTF-16 units.
/// Both are the caret's place when nothing is selected.
pub fn text_selection(dialog: &Dialog, id: u16) -> (usize, usize) {
	let (mut start, mut end) = (0u32, 0u32);
	unsafe {
		SendMessageW(
			dialog.control(id),
			CB_GETEDITSEL.cast_unsigned(),
			WPARAM((&raw mut start) as usize),
			LPARAM((&raw mut end) as isize),
		)
	};
	(start as usize, end as usize)
}

/// Sends `WM_CLOSE`, as the close box and Alt+F4 do. The dialog manager turns it into a
/// posted `IDCANCEL`, so pump messages afterwards.
pub fn send_close(dialog: &Dialog) {
	unsafe {
		SendMessageW(HWND(dialog.hwnd() as _), WM_CLOSE.cast_unsigned(), WPARAM(0), LPARAM(0))
	};
}
