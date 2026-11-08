//! Windows made from [`DialogTemplate`]s, driven by a Rust [`DialogHandler`].
//!
//! The dialog procedure is `extern "system"` and must never unwind into user32: every handler
//! call runs inside `catch_unwind`, and a panic is reported on the console.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Once;

use windows_core::{PCWSTR, PWSTR};

use super::template::{ControlClass, DialogTemplate};
use crate::bindings::Windows::Win32::{
	BM_GETCHECK, BM_SETCHECK, BST_CHECKED, BST_UNCHECKED, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL,
	CreateDialogIndirectParamW, DWLP_MSGRESULT, DestroyWindow, EnableWindow, GWL_STYLE,
	GetClientRect, GetDlgCtrlID, GetDlgItem, GetFocus, GetNextDlgTabItem, GetSystemMetrics,
	GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HWND, ICC_LISTVIEW_CLASSES,
	INITCOMMONCONTROLSEX, InitCommonControlsEx, IsWindow, IsWindowEnabled, LB_ADDSTRING,
	LB_GETCOUNT, LB_GETCURSEL, LB_GETTEXT, LB_GETTEXTLEN, LB_RESETCONTENT, LB_SETCURSEL, LPARAM,
	LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVIS_FOCUSED, LVIS_SELECTED, LVIS_STATEIMAGEMASK, LVITEMW,
	LVM_DELETEALLITEMS, LVM_ENSUREVISIBLE, LVM_GETEXTENDEDLISTVIEWSTYLE, LVM_GETITEMCOUNT,
	LVM_GETITEMSTATE, LVM_GETITEMTEXTW, LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTITEMW,
	LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMSTATE, LVNI_SELECTED, LVS_EX_CHECKBOXES,
	LVS_EX_FULLROWSELECT, PostMessageW, RECT, SM_CXVSCROLL, SW_HIDE, SW_SHOW, SW_SHOWNA,
	SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW, ShowWindow, WM_APP,
	WM_COMMAND, WM_INITDIALOG, WM_NCDESTROY, WM_NEXTDLGCTL, WPARAM, WS_VISIBLE,
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
/// The extended list view styles of a checkable list: a checkbox on each row, and a click
/// anywhere on a row selects it.
const CHECK_LIST_STYLES: i32 = LVS_EX_CHECKBOXES | LVS_EX_FULLROWSELECT;

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

	/// The dialog behind a window handle, such as a preferences page's `get_wnd`.
	pub const fn from_hwnd(hwnd: isize) -> Self {
		Self(hwnd)
	}

	/// Moves keyboard focus to control `id`, the way the dialog manager does for Tab.
	pub fn focus(&self, id: u16) {
		let control = self.control(id);
		unsafe {
			SendMessageW(self.raw(), WM_NEXTDLGCTL as u32, WPARAM(control.0 as usize), LPARAM(1))
		};
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

	/// Shows or hides a control, without activating anything. Tab skips hidden controls.
	pub fn set_visible(&self, id: u16, visible: bool) {
		unsafe {
			let _ = ShowWindow(self.control(id), if visible { SW_SHOWNA } else { SW_HIDE });
		}
	}

	/// Whether the control itself is shown, even while its dialog is hidden.
	pub fn is_visible(&self, id: u16) -> bool {
		let style = unsafe { GetWindowLongPtrW(self.control(id), GWL_STYLE) };
		style & WS_VISIBLE as isize != 0
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

	/// Sends `message` to control `id` and returns its answer.
	fn send(self, id: u16, message: i32, wparam: usize, lparam: isize) -> isize {
		unsafe {
			SendMessageW(self.control(id), message.cast_unsigned(), WPARAM(wparam), LPARAM(lparam))
		}
		.0
	}

	pub fn list_clear(&self, id: u16) {
		self.send(id, LB_RESETCONTENT, 0, 0);
	}

	pub fn list_add(&self, id: u16, text: &str) {
		let text = wide(text);
		self.send(id, LB_ADDSTRING, 0, text.as_ptr() as isize);
	}

	pub fn list_select(&self, id: u16, index: usize) {
		self.send(id, LB_SETCURSEL, index, 0);
	}

	/// The selected item, or `None` when nothing is selected.
	pub fn list_selection(&self, id: u16) -> Option<usize> {
		usize::try_from(self.send(id, LB_GETCURSEL, 0, 0)).ok()
	}

	pub fn list_count(&self, id: u16) -> usize {
		usize::try_from(self.send(id, LB_GETCOUNT, 0, 0)).unwrap_or(0)
	}

	/// The text of item `index`, or an empty string when there is no such item.
	pub fn list_text(&self, id: u16, index: usize) -> String {
		let Ok(length) = usize::try_from(self.send(id, LB_GETTEXTLEN, index, 0)) else {
			return String::new();
		};
		let mut buffer = vec![0u16; length + 1];
		let copied = self.send(id, LB_GETTEXT, index, buffer.as_mut_ptr() as isize);
		String::from_utf16_lossy(&buffer[..usize::try_from(copied).unwrap_or(0)])
	}

	/// The text of every item, in order.
	pub fn list_texts(&self, id: u16) -> Vec<String> {
		(0..self.list_count(id)).map(|index| self.list_text(id, index)).collect()
	}

	/// Makes list view `id` a list of rows with a checkbox each, in one column as wide as the
	/// list less a vertical scroll bar. Clicking anywhere on a row selects it. Call it once,
	/// before adding rows.
	pub fn check_list_init(&self, id: u16) {
		let (styles, mask) = (CHECK_LIST_STYLES as isize, CHECK_LIST_STYLES as usize);
		self.send(id, LVM_SETEXTENDEDLISTVIEWSTYLE, mask, styles);
		let mut client = RECT::default();
		unsafe {
			let _ = GetClientRect(self.control(id), &raw mut client);
		}
		// Room for the vertical scroll bar a long list gets, so it never needs a horizontal one.
		let width = client.right - unsafe { GetSystemMetrics(SM_CXVSCROLL) };
		let column = LVCOLUMNW { mask: LVCF_WIDTH as u32, cx: width, ..LVCOLUMNW::default() };
		self.send(id, LVM_INSERTCOLUMNW, 0, (&raw const column) as isize);
	}

	/// Whether list view `id` is set up as [`check_list_init`](Self::check_list_init) sets it
	/// up: a checkbox on each row, and a click anywhere on a row selects it.
	pub fn is_check_list(&self, id: u16) -> bool {
		const STYLES: isize = CHECK_LIST_STYLES as isize;
		self.send(id, LVM_GETEXTENDEDLISTVIEWSTYLE, 0, 0) & STYLES == STYLES
	}

	pub fn check_list_clear(&self, id: u16) {
		self.send(id, LVM_DELETEALLITEMS, 0, 0);
	}

	/// Adds a row after the last one, checked or not.
	pub fn check_list_add(&self, id: u16, text: &str, checked: bool) {
		let mut text = wide(text);
		let row = LVITEMW {
			mask: LVIF_TEXT as u32,
			iItem: i32::try_from(self.check_list_count(id)).unwrap_or(i32::MAX),
			pszText: PWSTR(text.as_mut_ptr()),
			..LVITEMW::default()
		};
		let added = self.send(id, LVM_INSERTITEMW, 0, (&raw const row) as isize);
		if let Ok(index) = usize::try_from(added) {
			self.set_row_checked(id, index, checked);
		}
	}

	pub fn check_list_count(&self, id: u16) -> usize {
		usize::try_from(self.send(id, LVM_GETITEMCOUNT, 0, 0)).unwrap_or(0)
	}

	/// The text of row `index`, or an empty string when there is no such row.
	pub fn check_list_text(&self, id: u16, index: usize) -> String {
		// A list view doesn't say how long a row's text is, so the buffer grows until the text
		// fits with room to spare.
		let mut buffer = vec![0u16; 256];
		loop {
			let mut row = LVITEMW {
				pszText: PWSTR(buffer.as_mut_ptr()),
				cchTextMax: i32::try_from(buffer.len()).unwrap_or(i32::MAX),
				..LVITEMW::default()
			};
			let copied = self.send(id, LVM_GETITEMTEXTW, index, (&raw mut row) as isize);
			let copied = usize::try_from(copied).unwrap_or(0);
			if copied + 1 < buffer.len() {
				return String::from_utf16_lossy(&buffer[..copied]);
			}
			buffer.resize(buffer.len() * 2, 0);
		}
	}

	/// The text of every row, in order.
	pub fn check_list_texts(&self, id: u16) -> Vec<String> {
		(0..self.check_list_count(id)).map(|index| self.check_list_text(id, index)).collect()
	}

	/// Whether row `index` is checked: its state image is the second, the checked box.
	pub fn is_row_checked(&self, id: u16, index: usize) -> bool {
		self.send(id, LVM_GETITEMSTATE, index, LVIS_STATEIMAGEMASK as isize) >> 12 == 2
	}

	pub fn set_row_checked(&self, id: u16, index: usize, checked: bool) {
		let image: u32 = if checked { 2 } else { 1 };
		self.set_row_state(id, index, image << 12, LVIS_STATEIMAGEMASK as u32);
	}

	/// The selected row, or `None` when no row is selected.
	pub fn check_list_selection(&self, id: u16) -> Option<usize> {
		// Searching from -1 starts at the first row.
		usize::try_from(self.send(id, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize)).ok()
	}

	/// Selects row `index`, makes it the row the arrow keys and Space act on, and scrolls the
	/// list until the whole row is in view.
	pub fn check_list_select(&self, id: u16, index: usize) {
		let state = (LVIS_SELECTED | LVIS_FOCUSED) as u32;
		self.set_row_state(id, index, state, state);
		// An lParam of FALSE asks for the whole row, not just part of it.
		self.send(id, LVM_ENSUREVISIBLE, index, 0);
	}

	/// Sets the bits of row `index`'s state that `mask` names.
	fn set_row_state(self, id: u16, index: usize, state: u32, mask: u32) {
		let row = LVITEMW { state, stateMask: mask, ..LVITEMW::default() };
		self.send(id, LVM_SETITEMSTATE, index, (&raw const row) as isize);
	}

	pub fn is_checked(&self, id: u16) -> bool {
		let state =
			unsafe { SendMessageW(self.control(id), BM_GETCHECK as u32, WPARAM(0), LPARAM(0)) };
		state.0 == BST_CHECKED as isize
	}

	pub fn set_checked(&self, id: u16, checked: bool) {
		let state = if checked { BST_CHECKED as usize } else { BST_UNCHECKED as usize };
		unsafe { SendMessageW(self.control(id), BM_SETCHECK as u32, WPARAM(state), LPARAM(0)) };
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
	/// Taken out while a call runs, so a message that arrives during the call is ignored (a
	/// refresh is remembered instead).
	handler: Cell<Option<Box<dyn DialogHandler>>>,
	busy: Cell<bool>,
	/// The window is gone. A running call drops the handler and the slot when it returns.
	destroyed: Cell<bool>,
	/// A refresh arrived while a call ran, for example while `ShellExecuteW` pumped messages;
	/// it runs once that call returns, so the dialog never stays out of date.
	refresh_pending: Cell<bool>,
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
		if unsafe { &*slot }.refresh_pending.take() {
			call(window, DialogHandler::refresh);
		}
	}
	result.map_err(|_| console::print("Foxide2k: a dialog handler panicked")).ok()
}

unsafe extern "system" fn dialog_proc(
	window: HWND,
	message: u32,
	wparam: WPARAM,
	lparam: LPARAM,
) -> isize {
	catch_unwind(AssertUnwindSafe(|| handle(window, message, wparam, lparam))).unwrap_or_else(
		|_| {
			console::print("Foxide2k: a dialog procedure panicked");
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
			let slot = slot_of(window);
			if !slot.is_null() && unsafe { &*slot }.busy.get() {
				unsafe { &*slot }.refresh_pending.set(true);
			} else {
				call(window, DialogHandler::refresh);
			}
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

/// Runs `create`, which may claim a slot through `WM_INITDIALOG`, and returns what it claimed. A
/// dialog that a handler's init creates meanwhile claims its own slot without touching this one.
fn claiming<R>(create: impl FnOnce() -> R) -> (R, usize) {
	let outer = CLAIMED.replace(0);
	let result = create();
	(result, CLAIMED.replace(outer))
}

/// Registers the common controls' list view class, once per process, as their documentation
/// asks before a list view is created. Current Windows would also register it on demand, when
/// the dialog asks for the class, with or without a manifest.
fn register_list_view() {
	static REGISTERED: Once = Once::new();
	REGISTERED.call_once(|| {
		#[allow(clippy::cast_possible_truncation, reason = "the structure is 8 bytes")]
		let controls = INITCOMMONCONTROLSEX {
			dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
			dwICC: ICC_LISTVIEW_CLASSES as u32,
		};
		unsafe {
			let _ = InitCommonControlsEx(&raw const controls);
		}
	});
}

fn create(
	parent: isize,
	template: &DialogTemplate,
	handler: Box<dyn DialogHandler>,
) -> Result<Dialog> {
	if template.items.iter().any(|item| item.class == ControlClass::ListView) {
		register_list_view();
	}
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
		refresh_pending: Cell::new(false),
	}));
	let parent = (parent != 0).then_some(HWND(parent as _));
	let (window, claimed) = claiming(|| unsafe {
		CreateDialogIndirectParamW(
			None,
			aligned.as_ptr().cast(),
			parent,
			Some(dialog_proc),
			LPARAM(slot as isize),
		)
	});
	if window.0.is_null() {
		let error = std::io::Error::last_os_error();
		if claimed != slot as usize {
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
		BN_CLICKED, BS_AUTOCHECKBOX, CBS_DROPDOWNLIST, DS_CONTROL, DS_MODALFRAME, ES_MULTILINE,
		ES_READONLY, GetClientRect, GetSystemMetrics, GetWindowTextW, HWND, LBS_NOTIFY, LPARAM,
		LVM_GETCOLUMNWIDTH, LVM_GETCOUNTPERPAGE, LVM_GETTOPINDEX, RECT, SM_CXVSCROLL, SendMessageW,
		WM_CLOSE, WM_COMMAND, WPARAM, WS_BORDER, WS_CAPTION, WS_CHILD, WS_POPUP, WS_SYSMENU,
		WS_TABSTOP, WS_VSCROLL,
	};
	use crate::dialog::style;
	use crate::dialog::template::{ControlClass, DialogItem, DialogTemplate, shell_font};
	use crate::testing::wants_all_keys;

	#[test]
	fn a_dialog_created_during_another_ones_creation_keeps_its_own_claim() {
		let ((), outer) = super::claiming(|| {
			// The outer dialog's WM_INITDIALOG claims its slot, then its init creates another.
			super::CLAIMED.set(1);
			let ((), inner) = super::claiming(|| super::CLAIMED.set(2));
			assert_eq!(inner, 2);
		});
		assert_eq!(outer, 1);
	}

	/// A hidden top-level window to own the dialogs under test. Also initialises COM for the
	/// live-region tests, as foobar2000's main thread has it.
	pub fn hidden_parent() -> isize {
		crate::testing::hidden_window()
	}

	/// Handles every message waiting for this thread.
	pub fn pump() {
		crate::testing::pump_messages();
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
			font: shell_font(),
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
	pub struct Record {
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
		/// Posts a refresh and pumps messages inside `command`, as `ShellExecuteW` can.
		pump_on_command: bool,
	}

	impl DialogHandler for Recorder {
		fn init(&mut self, _dialog: &Dialog) -> Option<u16> {
			self.record.borrow_mut().inits += 1;
			self.focus
		}

		fn command(&mut self, dialog: &Dialog, id: u16, notification: u16) {
			assert!(!self.panic_on_command, "a handler bug");
			self.record.borrow_mut().commands.push((id, notification));
			if self.pump_on_command {
				super::refresh_poster(dialog).post();
				pump();
			}
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
	fn a_list_box_holds_and_selects_its_items() {
		let template = DialogTemplate {
			items: vec![item(
				109,
				ControlClass::ListBox,
				"",
				WS_TABSTOP | WS_VSCROLL | WS_BORDER | LBS_NOTIFY,
			)],
			..template(MODELESS)
		};
		let dialog =
			create_modeless_dialog(hidden_parent(), &template, Box::new(Recorder::default()))
				.unwrap();
		for text in ["Liked Songs", "Church / Sunday set", "Getty favourites"] {
			dialog.list_add(109, text);
		}
		assert_eq!(dialog.list_count(109), 3);
		assert_eq!(dialog.list_text(109, 1), "Church / Sunday set");
		assert_eq!(
			dialog.list_texts(109),
			["Liked Songs", "Church / Sunday set", "Getty favourites"]
		);
		assert_eq!(dialog.list_selection(109), None);
		dialog.list_select(109, 1);
		assert_eq!(dialog.list_selection(109), Some(1));
		dialog.list_clear(109);
		assert_eq!(dialog.list_count(109), 0);
		assert!(dialog.list_texts(109).is_empty());
		assert_eq!(dialog.list_selection(109), None);
		destroy(dialog);
	}

	const LIST: u16 = 111;

	fn check_list() -> DialogItem {
		let style = style::LVS_REPORT
			| style::LVS_NOCOLUMNHEADER
			| style::LVS_SINGLESEL
			| style::LVS_SHOWSELALWAYS
			| style::WS_TABSTOP
			| style::WS_BORDER;
		DialogItem { style, cy: 60, ..item(LIST, ControlClass::ListView, "", 0) }
	}

	fn check_list_dialog(items: Vec<DialogItem>) -> Dialog {
		let template = DialogTemplate { items, ..template(MODELESS) };
		create_modeless_dialog(hidden_parent(), &template, Box::new(Recorder::default())).unwrap()
	}

	#[test]
	fn a_check_list_holds_rows_and_their_ticks() {
		let dialog = check_list_dialog(vec![check_list()]);
		dialog.check_list_init(LIST);
		dialog.check_list_add(LIST, "Liked Songs", true);
		dialog.check_list_add(LIST, "Church / Sunday set", false);
		assert_eq!(dialog.check_list_count(LIST), 2);
		assert_eq!(dialog.check_list_text(LIST, 1), "Church / Sunday set");
		assert_eq!(dialog.check_list_texts(LIST), ["Liked Songs", "Church / Sunday set"]);
		assert!(dialog.is_row_checked(LIST, 0));
		assert!(!dialog.is_row_checked(LIST, 1));
		dialog.set_row_checked(LIST, 1, true);
		dialog.set_row_checked(LIST, 0, false);
		assert!(!dialog.is_row_checked(LIST, 0) && dialog.is_row_checked(LIST, 1));
		assert_eq!(dialog.check_list_selection(LIST), None);
		dialog.check_list_select(LIST, 1);
		assert_eq!(dialog.check_list_selection(LIST), Some(1));
		dialog.check_list_clear(LIST);
		assert_eq!(dialog.check_list_count(LIST), 0);
		assert_eq!(dialog.check_list_selection(LIST), None);
		destroy(dialog);
	}

	#[test]
	fn a_list_view_is_a_check_list_once_it_is_set_up() {
		let dialog = check_list_dialog(vec![check_list()]);
		assert!(!dialog.is_check_list(LIST));
		dialog.check_list_init(LIST);
		assert!(dialog.is_check_list(LIST));
		destroy(dialog);
	}

	#[test]
	fn selecting_a_row_scrolls_it_into_view() {
		let dialog = check_list_dialog(vec![check_list()]);
		dialog.check_list_init(LIST);
		for row in 0..30 {
			dialog.check_list_add(LIST, &format!("Playlist {row}"), false);
		}
		let send = |message: i32| unsafe {
			SendMessageW(dialog.control(LIST), message.cast_unsigned(), WPARAM(0), LPARAM(0)).0
		};
		let page = send(LVM_GETCOUNTPERPAGE);
		assert!(0 < page && page < 29, "{page} rows fit");
		assert_eq!(send(LVM_GETTOPINDEX), 0);
		dialog.check_list_select(LIST, 29);
		let top = send(LVM_GETTOPINDEX);
		assert!(top <= 29 && 29 < top + page, "row 29 is out of view: {top}..{}", top + page);
		destroy(dialog);
	}

	#[test]
	fn a_check_list_reads_rows_of_any_length() {
		let dialog = check_list_dialog(vec![check_list()]);
		dialog.check_list_init(LIST);
		let long = "In Christ Alone ".repeat(40);
		dialog.check_list_add(LIST, &long, false);
		assert_eq!(dialog.check_list_text(LIST, 0), long);
		assert_eq!(dialog.check_list_text(LIST, 1), "", "there is no second row");
		destroy(dialog);
	}

	#[test]
	fn a_check_list_column_leaves_room_for_the_scroll_bar() {
		let dialog = check_list_dialog(vec![check_list()]);
		dialog.check_list_init(LIST);
		let mut client = RECT::default();
		assert!(unsafe { GetClientRect(dialog.control(LIST), &raw mut client) }.as_bool());
		let scroll_bar = unsafe { GetSystemMetrics(SM_CXVSCROLL) };
		let width = unsafe {
			SendMessageW(dialog.control(LIST), LVM_GETCOLUMNWIDTH as u32, WPARAM(0), LPARAM(0))
		};
		assert!(client.right > scroll_bar && scroll_bar > 0);
		assert_eq!(width.0, isize::try_from(client.right - scroll_bar).unwrap());
		destroy(dialog);
	}

	#[test]
	fn a_check_list_follows_its_label_in_tab_order() {
		let mut items = template(MODELESS).items;
		items.splice(2..2, [item(110, ControlClass::Static, "&Playlists:", 0), check_list()]);
		let dialog = check_list_dialog(items);
		assert_eq!(dialog.tab_order(), [102, LIST, 104, 107, 2]);
		destroy(dialog);
	}

	#[test]
	fn checkboxes_can_be_checked_and_unchecked() {
		let template = DialogTemplate {
			items: vec![item(
				108,
				ControlClass::Button,
				"Artist links add the &whole discography",
				WS_TABSTOP | BS_AUTOCHECKBOX,
			)],
			..child_template()
		};
		let dialog =
			create_child_dialog(hidden_parent(), &template, Box::new(Recorder::default())).unwrap();
		assert!(!dialog.is_checked(108));
		dialog.set_checked(108, true);
		assert!(dialog.is_checked(108));
		dialog.set_checked(108, false);
		assert!(!dialog.is_checked(108));
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
	fn hidden_controls_leave_the_tab_order_until_shown_again() {
		let (dialog, _record) = child(Recorder::default());
		assert!(dialog.is_visible(104));
		dialog.set_visible(104, false);
		assert!(!dialog.is_visible(104));
		assert_eq!(dialog.tab_order(), [102, 107, 2]);
		dialog.set_visible(104, true);
		assert!(dialog.is_visible(104));
		assert_eq!(dialog.tab_order(), [102, 104, 107, 2]);
		destroy(dialog);
	}

	#[test]
	fn only_multiline_edits_keep_escape_and_enter_from_the_dialog() {
		let mut with_multiline = child_template();
		with_multiline.items.push(item(
			105,
			ControlClass::Edit,
			"",
			WS_TABSTOP | ES_READONLY | ES_MULTILINE,
		));
		let dialog =
			create_child_dialog(hidden_parent(), &with_multiline, Box::new(Recorder::default()))
				.unwrap();
		assert!(!wants_all_keys(&dialog, 102));
		assert!(!wants_all_keys(&dialog, 104));
		assert!(wants_all_keys(&dialog, 105));
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
	fn focus_moves_to_the_named_control() {
		let (dialog, _record) = child(Recorder::default());
		dialog.focus(104);
		assert_eq!(dialog.focused(), Some(104));
		dialog.focus(102);
		assert_eq!(dialog.focused(), Some(102));
		assert_eq!(Dialog::from_hwnd(dialog.hwnd()), dialog);
		destroy(dialog);
	}

	#[test]
	fn refresh_that_arrives_during_a_call_runs_after_it() {
		let mut recorder = Recorder::default();
		recorder.pump_on_command = true;
		let (dialog, record) = child(recorder);
		click(dialog, 102);
		assert_eq!(record.borrow().refreshes, 1, "the refresh was dropped");
		pump();
		assert_eq!(record.borrow().refreshes, 1);
		destroy(dialog);
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
