//! The account controls the preferences page and the login window share: the status line, the
//! account button, the code and page fields and "Open browser". They show the login
//! controller's state and follow it as it changes.

use foxide2k::dialog::host::{Dialog, refresh_poster};
use foxide2k::dialog::notify::{BN_CLICKED, BN_SETFOCUS, EN_SETFOCUS};
use foxide2k::dialog::style::{
	BS_NOTIFY, ES_AUTOHSCROLL, ES_READONLY, SS_NOPREFIX, WS_BORDER, WS_TABSTOP,
};
use foxide2k::dialog::template::{ControlClass, DialogItem};

use crate::login::{controller, view};
use crate::status_line::StatusLine;

/// The status line, a polite live region.
pub const STATUS: u16 = 101;
/// "Log in", "Cancel login" or "Log out".
pub const ACCOUNT: u16 = 102;
pub const CODE_LABEL: u16 = 103;
/// The code to confirm, read-only so it can be read and copied.
pub const CODE: u16 = 104;
/// "Open browser": the only way Spotify's page opens. Shown only while a code waits, and never
/// disabled: Windows can give focus back to a hidden control, but not to a disabled one.
pub const BROWSER: u16 = 108;
pub const PAGE_LABEL: u16 = 109;
/// The page to confirm the code on, read-only like the code.
pub const PAGE: u16 = 110;

/// The controls that come and go with a login: the code and the page with their labels, and
/// "Open browser".
const LOGIN_CONTROLS: [u16; 5] = [CODE_LABEL, CODE, PAGE_LABEL, PAGE, BROWSER];

/// A read-only field on one line, which leaves Escape and Enter to the dialog it is in.
const FIELD: u32 = ES_READONLY | ES_AUTOHSCROLL | WS_BORDER | WS_TABSTOP;

/// When the code and the page are shown. "Open browser" shows only while a code waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginControls {
	/// Only while a code waits for the user, as on the preferences page.
	WhileConfirming,
	/// Always, as in the login window, which is open only to log in.
	Always,
}

/// One control of a dialog template, placed at (x, y) with size (cx, cy) in dialog units.
pub fn item(
	id: u16,
	class: ControlClass,
	text: &str,
	style: u32,
	place: (i16, i16, i16, i16),
) -> DialogItem {
	let (x, y, cx, cy) = place;
	DialogItem { id, class, text: text.to_owned(), style, x, y, cx, cy }
}

/// The controls, in tab order, at (`x`, `y`) and `width` dialog units wide. Each label comes
/// right before its control, so screen readers name the control after it.
pub fn items(x: i16, y: i16, width: i16) -> Vec<DialogItem> {
	vec![
		item(STATUS, ControlClass::Static, "", SS_NOPREFIX, (x, y, width, 18)),
		item(ACCOUNT, ControlClass::Button, "&Log in", WS_TABSTOP, (x, y + 21, 84, 14)),
		item(CODE_LABEL, ControlClass::Static, "Co&de:", 0, (x, y + 43, 30, 8)),
		item(CODE, ControlClass::Edit, "", FIELD, (x + 32, y + 41, width - 32, 12)),
		item(PAGE_LABEL, ControlClass::Static, "Pa&ge:", 0, (x, y + 59, 30, 8)),
		item(PAGE, ControlClass::Edit, "", FIELD, (x + 32, y + 57, width - 32, 12)),
		item(
			BROWSER,
			ControlClass::Button,
			"Open &browser",
			WS_TABSTOP | BS_NOTIFY,
			(x, y + 75, 84, 14),
		),
	]
}

/// The account controls of one open dialog, subscribed to the login for as long as it lives.
pub struct AccountPanel {
	subscription: u64,
	status: StatusLine,
	controls: LoginControls,
}

impl AccountPanel {
	/// Subscribes to the login and shows where it is. Call from the dialog's `init`.
	pub fn init(dialog: Dialog, controls: LoginControls) -> Self {
		let poster = refresh_poster(&dialog);
		let subscription = controller().subscribe(Box::new(move || poster.post()));
		controller().refresh();
		let status = StatusLine::new(dialog, STATUS, view(&controller().state()).status);
		let mut panel = Self { subscription, status, controls };
		panel.refresh(dialog);
		panel
	}

	/// Handles the panel's own commands; false for any other control.
	pub fn command(dialog: Dialog, id: u16, notification: u16) -> bool {
		match (id, notification) {
			(ACCOUNT, BN_CLICKED) => controller().press(),
			(BROWSER, BN_CLICKED) => controller().open_browser(),
			// Coming back to foobar2000, Windows gives focus to the control that had it when
			// foobar2000 was left, which may have been hidden since; the refresh moves it on.
			(CODE | PAGE, EN_SETFOCUS) | (BROWSER, BN_SETFOCUS) if !dialog.is_visible(id) => {
				refresh_poster(&dialog).post();
			}
			(STATUS | ACCOUNT | CODE_LABEL | CODE | PAGE_LABEL | PAGE | BROWSER, _) => {}
			_ => return false,
		}
		true
	}

	/// Shows the login's current state; a new status is announced.
	pub fn refresh(&mut self, dialog: Dialog) {
		let shown = view(&controller().state());
		let fields = self.controls == LoginControls::Always || shown.confirming;
		let visible = |id| match id {
			CODE_LABEL | CODE | PAGE_LABEL | PAGE => fields,
			BROWSER => shown.confirming,
			_ => true,
		};
		// Focus on a control that hides moves to the account button, which replaced it.
		if dialog.focused().is_some_and(|id| !visible(id)) {
			dialog.focus(ACCOUNT);
		}
		dialog.set_text(ACCOUNT, shown.button);
		dialog.set_text(CODE, &shown.code);
		dialog.set_text(PAGE, &shown.page);
		for id in LOGIN_CONTROLS {
			dialog.set_visible(id, visible(id));
		}
		self.status.show(dialog, shown.status);
	}
}

impl Drop for AccountPanel {
	fn drop(&mut self) {
		controller().unsubscribe(self.subscription);
	}
}
