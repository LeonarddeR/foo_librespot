//! Windows made from [`DialogTemplate`]s, driven by a Rust [`DialogHandler`].
//!
//! The dialog procedure is `extern "system"` and must never unwind into user32: every handler
//! call runs inside `catch_unwind`, and a panic is reported on the console.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};

use windows_core::PCWSTR;

use super::template::DialogTemplate;
use crate::bindings::Windows::Win32::{
	CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CreateDialogIndirectParamW, DWLP_MSGRESULT,
	DestroyWindow, EnableWindow, GetDlgCtrlID, GetDlgItem, GetFocus, GetNextDlgTabItem,
	GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HWND, IsWindow, IsWindowEnabled,
	LPARAM, PostMessageW, SW_SHOW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW,
	SetWindowTextW, ShowWindow, WM_APP, WM_COMMAND, WM_INITDIALOG, WM_NCDESTROY, WM_NEXTDLGCTL,
	WPARAM,
};
use crate::console;
use crate::cxx::ExceptionKind;
use crate::error::{Error, Result};
use crate::strings::wide;

/// `DWLP_USER` on 64-bit Windows: after the message result and the dialog procedure.
const DWLP_USER: i32 = DWLP_MSGRESULT + 16;
/// Asks the dialog to call [`DialogHandler::refresh`]; see [`RefreshPoster`].
const WM_REFRESH: u32 = WM_APP as u32 + 1;
const INIT: u32 = WM_INITDIALOG as u32;
const COMMAND: u32 = WM_COMMAND as u32;
const NC_DESTROY: u32 = WM_NCDESTROY as u32;

/// What a dialog does with its messages. Every call runs on the thread that made the dialog.
pub trait DialogHandler: 'static {
	/// The dialog's controls exist. Returns the control to focus first, or `None` to leave
	/// focus where it is, as a preferences page must.
	fn init(&mut self, dialog: &Dialog) -> Option<u16>;
	/// A control sent `WM_COMMAND`; `id` 2 is `IDCANCEL`, which Escape and the close box send.
	fn command(&mut self, dialog: &Dialog, id: u16, notification: u16);
	/// A [`RefreshPoster`] asked for the dialog to be brought up to date.
	fn refresh(&mut self, dialog: &Dialog);
}

/// A dialog window. Copyable; using it after the window is gone does nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dialog(isize);

impl Dialog {
	fn raw(self) -> HWND {
		HWND(self.0 as _)
	}

	pub(crate) fn control(self, id: u16) -> HWND {
		unsafe { GetDlgItem(Some(self.raw()), i32::from(id)) }
	}

	pub const fn hwnd(&self) -> isize {
		self.0
	}

	pub fn set_text(&self, id: u16, text: &str) {
		let text = wide(text);
		unsafe {
			let _ = SetWindowTextW(self.control(id), PCWSTR(text.as_ptr()));
		}
	}

	pub fn text(&self, id: u16) -> String {
		let control = self.control(id);
		let length = unsafe { GetWindowTextLengthW(control) };
		let mut buffer = vec![0u16; usize::try_from(length).unwrap_or(0) + 1];
		let copied = unsafe {
			GetWindowTextW(
				control,
				windows_core::PWSTR(buffer.as_mut_ptr()),
				i32::try_from(buffer.len()).unwrap_or(i32::MAX),
			)
		};
		String::from_utf16_lossy(&buffer[..usize::try_from(copied).unwrap_or(0)])
	}

	pub fn enable(&self, id: u16, enabled: bool) {
		unsafe {
			let _ = EnableWindow(self.control(id), enabled);
		}
	}

	pub fn is_enabled(&self, id: u16) -> bool {
		unsafe { IsWindowEnabled(self.control(id)) }.as_bool()
	}

	pub fn combo_add(&self, id: u16, text: &str) {
		let text = wide(text);
		unsafe {
			SendMessageW(
				self.control(id),
				CB_ADDSTRING as u32,
				WPARAM(0),
				LPARAM(text.as_ptr() as isize),
			);
		}
	}

	pub fn combo_select(&self, id: u16, index: usize) {
		unsafe { SendMessageW(self.control(id), CB_SETCURSEL as u32, WPARAM(index), LPARAM(0)) };
	}

	/// The selected item, or `None` when nothing is selected.
	pub fn combo_selection(&self, id: u16) -> Option<usize> {
		let selected =
			unsafe { SendMessageW(self.control(id), CB_GETCURSEL as u32, WPARAM(0), LPARAM(0)) };
		usize::try_from(selected.0).ok()
	}

	/// The ids of the controls Tab visits, starting at the first tab stop. Disabled controls
	/// are skipped, as Tab skips them.
	pub fn tab_order(&self) -> Vec<u16> {
		let first = unsafe { GetNextDlgTabItem(self.raw(), None, false) };
		let mut order = Vec::new();
		let mut current = first;
		while !current.0.is_null() && order.len() < 256 {
			order.push(u16::try_from(unsafe { GetDlgCtrlID(current) }).unwrap_or(0));
			current = unsafe { GetNextDlgTabItem(self.raw(), Some(current), false) };
			if current == first {
				break;
			}
		}
		order
	}

	/// The id of the control in this dialog that has keyboard focus.
	pub fn focused(&self) -> Option<u16> {
		let focus = unsafe { GetFocus() };
		if focus.0.is_null() {
			return None;
		}
		let id = u16::try_from(unsafe { GetDlgCtrlID(focus) }).ok()?;
		(self.control(id) == focus).then_some(id)
	}

	pub fn is_alive(&self) -> bool {
		unsafe { IsWindow(Some(self.raw())) }.as_bool()
	}

	/// Shows a top-level dialog.
	pub fn show(&self) {
		unsafe {
			let _ = ShowWindow(self.raw(), SW_SHOW);
		}
	}

	/// Brings a top-level dialog to the front.
	pub fn activate(&self) {
		unsafe {
			let _ = SetForegroundWindow(self.raw());
		}
	}
}

/// Asks a dialog, from any thread, to call [`DialogHandler::refresh`] on its own thread.
#[derive(Clone, Debug)]
pub struct RefreshPoster(isize);

impl RefreshPoster {
	/// Posting after the dialog is gone does nothing.
	pub fn post(&self) {
		unsafe {
			let _ = PostMessageW(Some(HWND(self.0 as _)), WM_REFRESH, WPARAM(0), LPARAM(0));
		}
	}
}

pub const fn refresh_poster(dialog: &Dialog) -> RefreshPoster {
	RefreshPoster(dialog.0)
}

/// Where a dialog keeps its handler, behind `DWLP_USER`.
struct Slot {
	/// Taken out while a call runs, so a message that arrives during the call is ignored.
	handler: Cell<Option<Box<dyn DialogHandler>>>,
	busy: Cell<bool>,
	/// The window is gone. A running call drops the handler and the slot when it returns.
	destroyed: Cell<bool>,
}

thread_local! {
	/// The slot the last `WM_INITDIALOG` on this thread took over, so a failed creation knows
	/// whether it still owns its slot.
	static CLAIMED: Cell<usize> = const { Cell::new(0) };
}

fn slot_of(window: HWND) -> *mut Slot {
	unsafe { GetWindowLongPtrW(window, DWLP_USER) as *mut Slot }
}

/// Calls the handler, unless it is busy with another message or the dialog has none.
fn call<R>(window: HWND, f: impl FnOnce(&mut dyn DialogHandler, &Dialog) -> R) -> Option<R> {
	let slot = slot_of(window);
	if slot.is_null() {
		return None;
	}
	let mut handler = unsafe { &*slot }.handler.take()?;
	unsafe { &*slot }.busy.set(true);
	let result = catch_unwind(AssertUnwindSafe(|| f(&mut *handler, &Dialog(window.0 as isize))));
	unsafe { &*slot }.busy.set(false);
	if unsafe { &*slot }.destroyed.get() {
		drop(handler);
		drop(unsafe { Box::from_raw(slot) });
	} else {
		unsafe { &*slot }.handler.set(Some(handler));
	}
	result.map_err(|_| console::print("OxiBar: a dialog handler panicked")).ok()
}

unsafe extern "system" fn dialog_proc(
	window: HWND,
	message: u32,
	wparam: WPARAM,
	lparam: LPARAM,
) -> isize {
	catch_unwind(AssertUnwindSafe(|| handle(window, message, wparam, lparam))).unwrap_or_else(
		|_| {
			console::print("OxiBar: a dialog procedure panicked");
			0
		},
	)
}

fn handle(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
	match message {
		INIT => {
			CLAIMED.set(lparam.0.cast_unsigned());
			unsafe { SetWindowLongPtrW(window, DWLP_USER, lparam.0) };
			if let Some(Some(id)) = call(window, DialogHandler::init) {
				let control = Dialog(window.0 as isize).control(id);
				unsafe {
					SendMessageW(
						window,
						WM_NEXTDLGCTL as u32,
						WPARAM(control.0 as usize),
						LPARAM(1),
					)
				};
			}
			// FALSE: focus moves only when `init` asked for it.
			0
		}
		COMMAND => {
			#[allow(clippy::cast_possible_truncation, reason = "LOWORD and HIWORD")]
			let (id, notification) = (wparam.0 as u16, (wparam.0 >> 16) as u16);
			call(window, |handler, dialog| handler.command(dialog, id, notification));
			1
		}
		WM_REFRESH => {
			call(window, DialogHandler::refresh);
			1
		}
		NC_DESTROY => {
			let slot = slot_of(window);
			if !slot.is_null() {
				unsafe { SetWindowLongPtrW(window, DWLP_USER, 0) };
				unsafe { &*slot }.destroyed.set(true);
				if !unsafe { &*slot }.busy.get() {
					drop(unsafe { Box::from_raw(slot) });
				}
			}
			0
		}
		_ => 0,
	}
}

fn create(
	parent: isize,
	template: &DialogTemplate,
	handler: Box<dyn DialogHandler>,
) -> Result<Dialog> {
	let words = template.to_bytes();
	// Windows reads templates from DWORD-aligned memory.
	let mut aligned = vec![0u32; words.len().div_ceil(2)];
	unsafe {
		std::ptr::copy_nonoverlapping(words.as_ptr(), aligned.as_mut_ptr().cast(), words.len());
	}
	let slot = Box::into_raw(Box::new(Slot {
		handler: Cell::new(Some(handler)),
		busy: Cell::new(false),
		destroyed: Cell::new(false),
	}));
	CLAIMED.set(0);
	let parent = (parent != 0).then_some(HWND(parent as _));
	let window = unsafe {
		CreateDialogIndirectParamW(
			None,
			aligned.as_ptr().cast(),
			parent,
			Some(dialog_proc),
			LPARAM(slot as isize),
		)
	};
	if window.0.is_null() {
		let error = std::io::Error::last_os_error();
		if CLAIMED.get() != slot as usize {
			drop(unsafe { Box::from_raw(slot) });
		}
		return Err(Error::new(ExceptionKind::Std, format!("can't create a dialog: {error}")));
	}
	Ok(Dialog(window.0 as isize))
}

/// A dialog inside `parent`, such as a preferences page. Its template carries `WS_CHILD`.
pub fn create_child_dialog(
	parent: isize,
	template: &DialogTemplate,
	handler: Box<dyn DialogHandler>,
) -> Result<Dialog> {
	create(parent, template, handler)
}

/// A top-level dialog owned by `owner`, hidden until [`Dialog::show`]. Its template carries
/// `WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME`. Tab and access keys only work once
/// the caller registers it with foobar2000's modeless dialog manager.
pub fn create_modeless_dialog(
	owner: isize,
	template: &DialogTemplate,
	handler: Box<dyn DialogHandler>,
) -> Result<Dialog> {
	create(owner, template, handler)
}

/// Destroys the dialog; its handler is dropped once no call to it is running.
pub fn destroy(dialog: Dialog) {
	unsafe {
		let _ = DestroyWindow(dialog.raw());
	}
}

#[cfg(test)]
pub(crate) mod tests {
	use std::cell::RefCell;
	use std::rc::Rc;

	use super::{Dialog, DialogHandler, create_child_dialog, create_modeless_dialog, destroy};
	use crate::bindings::Windows::Win32::{
		BN_CLICKED, CBS_DROPDOWNLIST, COINIT_APARTMENTTHREADED, CoInitializeEx, CreateWindowExW,
		DS_CONTROL, DS_MODALFRAME, DispatchMessageW, ES_READONLY, GetWindowTextW, HWND, LPARAM,
		MSG, PM_REMOVE, PeekMessageW, SendMessageW, TranslateMessage, WM_CLOSE, WM_COMMAND, WPARAM,
		WS_CAPTION, WS_CHILD, WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VSCROLL,
	};
	use crate::dialog::template::{ControlClass, DialogItem, DialogTemplate};

	/// A hidden top-level window to own the dialogs under test. Also initialises COM for the
	/// live-region tests, as foobar2000's main thread has it.
	pub(crate) fn hidden_parent() -> isize {
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

	/// Handles every message waiting for this thread.
	pub(crate) fn pump() {
		let mut message = MSG::default();
		while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE as u32) }.as_bool() {
			unsafe {
				let _ = TranslateMessage(&raw const message);
				DispatchMessageW(&raw const message);
			}
		}
	}

	fn item(id: u16, class: ControlClass, text: &str, style: i32) -> DialogItem {
		DialogItem {
			id,
			class,
			text: text.to_owned(),
			style: style.cast_unsigned(),
			x: 7,
			y: 7 + 16 * i16::try_from(id % 10).unwrap(),
			cx: 80,
			cy: 14,
		}
	}

	const MODELESS: u32 = WS_POPUP | (WS_CAPTION | WS_SYSMENU | DS_MODALFRAME) as u32;

	fn template(style: u32) -> DialogTemplate {
		DialogTemplate {
			style,
			title: "Log in to Spotify".to_owned(),
			cx: 200,
			cy: 140,
			font: (8, "MS Shell Dlg".to_owned()),
			items: vec![
				item(101, ControlClass::Static, "Not logged in to Spotify.", 0),
				item(102, ControlClass::Button, "&Log in", WS_TABSTOP),
				item(103, ControlClass::Static, "Co&de and page:", 0),
				item(104, ControlClass::Edit, "", WS_TABSTOP | ES_READONLY),
				item(107, ControlClass::ComboBox, "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST),
				item(2, ControlClass::Button, "Close", WS_TABSTOP),
			],
		}
	}

	fn child_template() -> DialogTemplate {
		DialogTemplate { title: String::new(), ..template((WS_CHILD | DS_CONTROL) as u32) }
	}

	#[derive(Default)]
	pub(crate) struct Record {
		pub inits: usize,
		pub commands: Vec<(u16, u16)>,
		pub refreshes: usize,
		pub dropped: bool,
	}

	#[derive(Default)]
	struct Recorder {
		record: Rc<RefCell<Record>>,
		focus: Option<u16>,
		panic_on_command: bool,
		destroy_on_command: bool,
	}

	impl DialogHandler for Recorder {
		fn init(&mut self, _dialog: &Dialog) -> Option<u16> {
			self.record.borrow_mut().inits += 1;
			self.focus
		}

		fn command(&mut self, dialog: &Dialog, id: u16, notification: u16) {
			assert!(!self.panic_on_command, "a handler bug");
			self.record.borrow_mut().commands.push((id, notification));
			if self.destroy_on_command {
				destroy(*dialog);
				// Still running after the window is gone: the handler must still be alive.
				self.record.borrow_mut().commands.push((0, 0));
			}
		}

		fn refresh(&mut self, _dialog: &Dialog) {
			self.record.borrow_mut().refreshes += 1;
		}
	}

	impl Drop for Recorder {
		fn drop(&mut self) {
			self.record.borrow_mut().dropped = true;
		}
	}

	fn child(recorder: Recorder) -> (Dialog, Rc<RefCell<Record>>) {
		let record = Rc::clone(&recorder.record);
		let dialog =
			create_child_dialog(hidden_parent(), &child_template(), Box::new(recorder)).unwrap();
		(dialog, record)
	}

	fn click(dialog: Dialog, id: u16) {
		let wparam = usize::from(id) | (usize::try_from(BN_CLICKED).unwrap() << 16);
		unsafe {
			SendMessageW(HWND(dialog.hwnd() as _), WM_COMMAND as u32, WPARAM(wparam), LPARAM(0))
		};
	}

	#[test]
	fn child_dialog_creates_its_controls_and_calls_init() {
		let (dialog, record) = child(Recorder::default());
		assert_eq!(dialog.text(102), "&Log in");
		assert_eq!(dialog.text(101), "Not logged in to Spotify.");
		assert_eq!(record.borrow().inits, 1);
		destroy(dialog);
	}

	#[test]
	fn modeless_dialog_has_its_caption_and_focuses_what_init_asks() {
		let mut recorder = Recorder::default();
		recorder.focus = Some(104);
		let dialog =
			create_modeless_dialog(hidden_parent(), &template(MODELESS), Box::new(recorder))
				.unwrap();
		let mut caption = [0u16; 64];
		let length = unsafe {
			GetWindowTextW(HWND(dialog.hwnd() as _), windows_core::PWSTR(caption.as_mut_ptr()), 64)
		};
		assert_eq!(
			String::from_utf16_lossy(&caption[..usize::try_from(length).unwrap()]),
			"Log in to Spotify"
		);
		assert_eq!(dialog.focused(), Some(104));
		destroy(dialog);
	}

	#[test]
	fn commands_reach_the_handler() {
		let (dialog, record) = child(Recorder::default());
		click(dialog, 102);
		assert_eq!(record.borrow().commands, [(102, u16::try_from(BN_CLICKED).unwrap())]);
		destroy(dialog);
	}

	#[test]
	fn escape_reaches_the_handler_as_idcancel() {
		let recorder = Recorder::default();
		let record = Rc::clone(&recorder.record);
		let dialog =
			create_modeless_dialog(hidden_parent(), &template(MODELESS), Box::new(recorder))
				.unwrap();
		unsafe { SendMessageW(HWND(dialog.hwnd() as _), WM_CLOSE as u32, WPARAM(0), LPARAM(0)) };
		pump();
		assert!(
			record.borrow().commands.iter().any(|&(id, _)| id == 2),
			"{:?}",
			record.borrow().commands
		);
		destroy(dialog);
	}

	#[test]
	fn set_text_enable_and_combo_round_trip() {
		let (dialog, _record) = child(Recorder::default());
		dialog.set_text(104, "Code: KKGT-2024\r\nPage: https://spotify.com/pair");
		assert_eq!(dialog.text(104), "Code: KKGT-2024\r\nPage: https://spotify.com/pair");
		dialog.enable(102, false);
		assert!(!dialog.is_enabled(102));
		dialog.enable(102, true);
		assert!(dialog.is_enabled(102));
		assert_eq!(dialog.combo_selection(107), None);
		for text in ["96 kbit/s", "160 kbit/s", "320 kbit/s"] {
			dialog.combo_add(107, text);
		}
		dialog.combo_select(107, 2);
		assert_eq!(dialog.combo_selection(107), Some(2));
		destroy(dialog);
	}

	#[test]
	fn tab_order_follows_creation_order() {
		let (dialog, _record) = child(Recorder::default());
		assert_eq!(dialog.tab_order(), [102, 104, 107, 2]);
		dialog.enable(104, false);
		assert_eq!(dialog.tab_order(), [102, 107, 2]);
		destroy(dialog);
	}

	#[test]
	fn refresh_poster_reaches_the_handler_and_is_harmless_after_destroy() {
		let (dialog, record) = child(Recorder::default());
		let poster = super::refresh_poster(&dialog);
		let from_another_thread = poster.clone();
		std::thread::spawn(move || from_another_thread.post()).join().unwrap();
		pump();
		assert_eq!(record.borrow().refreshes, 1);
		destroy(dialog);
		poster.post();
		pump();
		assert_eq!(record.borrow().refreshes, 1);
	}

	#[test]
	fn destroy_drops_the_handler() {
		let (dialog, record) = child(Recorder::default());
		assert!(dialog.is_alive());
		destroy(dialog);
		assert!(record.borrow().dropped);
		assert!(!dialog.is_alive());
	}

	#[test]
	fn handler_that_destroys_its_dialog_is_dropped_after_its_call() {
		let mut recorder = Recorder::default();
		recorder.destroy_on_command = true;
		let (dialog, record) = child(recorder);
		click(dialog, 102);
		assert_eq!(record.borrow().commands, [(102, u16::try_from(BN_CLICKED).unwrap()), (0, 0)]);
		assert!(record.borrow().dropped);
		assert!(!dialog.is_alive());
	}

	#[test]
	fn panicking_handler_does_not_unwind_into_user32() {
		let mut recorder = Recorder::default();
		recorder.panic_on_command = true;
		let (dialog, record) = child(recorder);
		click(dialog, 102);
		click(dialog, 102);
		assert!(record.borrow().commands.is_empty());
		assert!(dialog.is_alive());
		destroy(dialog);
		assert!(record.borrow().dropped);
	}
}
