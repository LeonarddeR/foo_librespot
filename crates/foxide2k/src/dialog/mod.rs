//! Win32 dialogs for components: templates built in Rust, and the windows made from them.

pub mod accessibility;
pub mod host;
pub mod template;

/// Window and control styles for dialog templates, as `u32`.
pub mod style {
	use crate::bindings::Windows::Win32 as win;

	pub const WS_CHILD: u32 = win::WS_CHILD.cast_unsigned();
	pub const WS_POPUP: u32 = win::WS_POPUP;
	pub const WS_CAPTION: u32 = win::WS_CAPTION.cast_unsigned();
	pub const WS_SYSMENU: u32 = win::WS_SYSMENU.cast_unsigned();
	pub const WS_TABSTOP: u32 = win::WS_TABSTOP.cast_unsigned();
	pub const WS_BORDER: u32 = win::WS_BORDER.cast_unsigned();
	pub const WS_VSCROLL: u32 = win::WS_VSCROLL.cast_unsigned();
	pub const DS_CONTROL: u32 = win::DS_CONTROL.cast_unsigned();
	pub const DS_MODALFRAME: u32 = win::DS_MODALFRAME.cast_unsigned();
	pub const DS_CENTER: u32 = win::DS_CENTER.cast_unsigned();
	pub const BS_PUSHBUTTON: u32 = win::BS_PUSHBUTTON.cast_unsigned();
	pub const BS_GROUPBOX: u32 = win::BS_GROUPBOX.cast_unsigned();
	/// A checkbox that checks and unchecks itself when clicked.
	pub const BS_AUTOCHECKBOX: u32 = win::BS_AUTOCHECKBOX.cast_unsigned();
	/// Makes a button tell its dialog when it gains and loses focus.
	pub const BS_NOTIFY: u32 = win::BS_NOTIFY.cast_unsigned();
	pub const ES_MULTILINE: u32 = win::ES_MULTILINE.cast_unsigned();
	pub const ES_READONLY: u32 = win::ES_READONLY.cast_unsigned();
	pub const ES_AUTOVSCROLL: u32 = win::ES_AUTOVSCROLL.cast_unsigned();
	pub const ES_AUTOHSCROLL: u32 = win::ES_AUTOHSCROLL.cast_unsigned();
	pub const SS_NOPREFIX: u32 = win::SS_NOPREFIX.cast_unsigned();
	pub const CBS_DROPDOWNLIST: u32 = win::CBS_DROPDOWNLIST.cast_unsigned();
	/// Makes a list box tell its dialog about clicks and double clicks.
	pub const LBS_NOTIFY: u32 = win::LBS_NOTIFY.cast_unsigned();
	/// A push button that Enter presses while focus is on a control that doesn't use Enter.
	pub const BS_DEFPUSHBUTTON: u32 = win::BS_DEFPUSHBUTTON.cast_unsigned();
	/// A list view that shows its rows one below the other, in columns.
	pub const LVS_REPORT: u32 = win::LVS_REPORT.cast_unsigned();
	pub const LVS_NOCOLUMNHEADER: u32 = win::LVS_NOCOLUMNHEADER.cast_unsigned();
	pub const LVS_SINGLESEL: u32 = win::LVS_SINGLESEL.cast_unsigned();
	/// Keeps a list view's selection shown while another control has focus.
	pub const LVS_SHOWSELALWAYS: u32 = win::LVS_SHOWSELALWAYS.cast_unsigned();
	/// A list view for [`Dialog::check_list_init`](super::host::Dialog::check_list_init): one
	/// column of rows without a header, one row selected at a time and shown while another
	/// control has focus, with a border, and a tab stop.
	pub const CHECK_LIST: u32 = LVS_REPORT
		| LVS_NOCOLUMNHEADER
		| LVS_SINGLESEL
		| LVS_SHOWSELALWAYS
		| WS_TABSTOP
		| WS_BORDER;
}

/// `WM_COMMAND` notification codes and the ids Windows gives meaning to.
#[allow(clippy::cast_possible_truncation, reason = "the values are 16-bit by definition")]
pub mod notify {
	use crate::bindings::Windows::Win32 as win;

	pub const BN_CLICKED: u16 = win::BN_CLICKED as u16;
	/// Sent only by buttons with `BS_NOTIFY`.
	pub const BN_SETFOCUS: u16 = win::BN_SETFOCUS as u16;
	pub const EN_SETFOCUS: u16 = win::EN_SETFOCUS as u16;
	pub const CBN_SELCHANGE: u16 = win::CBN_SELCHANGE as u16;
	/// The id of a dialog's Cancel or Close button, which Escape and the close box press.
	pub const IDCANCEL: u16 = win::IDCANCEL as u16;
	/// The id of a dialog's default button, which Enter presses.
	pub const IDOK: u16 = win::IDOK as u16;
	/// Sent by a list box with `LBS_NOTIFY` when an item is double clicked.
	pub const LBN_DBLCLK: u16 = win::LBN_DBLCLK as u16;
}

#[cfg(test)]
mod tests {
	use super::{notify, style};

	#[test]
	fn styles_and_notifications_have_their_win32_values() {
		assert_eq!(style::WS_TABSTOP, 0x0001_0000);
		assert_eq!(style::WS_POPUP, 0x8000_0000);
		assert_eq!(style::BS_GROUPBOX, 7);
		assert_eq!(style::BS_AUTOCHECKBOX, 3);
		assert_eq!(style::ES_READONLY, 0x0800);
		assert_eq!(style::ES_AUTOHSCROLL, 0x0080);
		assert_eq!(style::CBS_DROPDOWNLIST, 3);
		assert_eq!(style::DS_CENTER, 0x0800);
		assert_eq!((notify::BN_CLICKED, notify::CBN_SELCHANGE, notify::IDCANCEL), (0, 1, 2));
		assert_eq!(style::BS_NOTIFY, 0x4000);
		assert_eq!((notify::BN_SETFOCUS, notify::EN_SETFOCUS), (6, 0x0100));
		assert_eq!((style::LBS_NOTIFY, style::BS_DEFPUSHBUTTON), (1, 1));
		assert_eq!((notify::LBN_DBLCLK, notify::IDOK), (2, 1));
		assert_eq!(
			(
				style::LVS_REPORT,
				style::LVS_NOCOLUMNHEADER,
				style::LVS_SINGLESEL,
				style::LVS_SHOWSELALWAYS
			),
			(1, 0x4000, 4, 8)
		);
		assert_eq!(style::CHECK_LIST, 0x0001_0000 | 0x0080_0000 | 0x4000 | 4 | 8 | 1);
	}
}
