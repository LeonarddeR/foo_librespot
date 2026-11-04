//! The account controls the preferences page and the login window share: the status line, the
//! account button, the code field and "Open browser". They show the login controller's state
//! and follow it as it changes.

use oxibar::console;
use oxibar::dialog::accessibility::{announce, make_live_region};
use oxibar::dialog::host::{Dialog, refresh_poster};
use oxibar::dialog::notify::BN_CLICKED;
use oxibar::dialog::style::{
	ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, SS_NOPREFIX, WS_BORDER, WS_TABSTOP,
};
use oxibar::dialog::template::{ControlClass, DialogItem};

use crate::login::{controller, view};

/// The status line, a polite live region.
pub const STATUS: u16 = 101;
/// "Log in", "Cancel login" or "Log out".
pub const ACCOUNT: u16 = 102;
pub const CODE_LABEL: u16 = 103;
/// The code and the page to confirm it on, read-only so it can be read and copied.
pub const CODE: u16 = 104;
/// "Open browser": the only way Spotify's page opens.
pub const BROWSER: u16 = 108;

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
		item(CODE_LABEL, ControlClass::Static, "Co&de and page:", 0, (x, y + 41, width, 8)),
		item(
			CODE,
			ControlClass::Edit,
			"",
			ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | WS_BORDER | WS_TABSTOP,
			(x, y + 51, width, 22),
		),
		item(BROWSER, ControlClass::Button, "Open &browser", WS_TABSTOP, (x, y + 77, 84, 14)),
	]
}

/// The account controls of one open dialog, subscribed to the login for as long as it lives.
pub struct AccountPanel {
	subscription: u64,
	status: String,
}

impl AccountPanel {
	/// Subscribes to the login and shows where it is. Call from the dialog's `init`.
	pub fn init(dialog: Dialog) -> Self {
		let poster = refresh_poster(&dialog);
		let subscription = controller().subscribe(Box::new(move || poster.post()));
		if let Err(error) = make_live_region(&dialog, STATUS) {
			console::print(&format!("foo_librespot: {}", error.message));
		}
		let mut panel = Self { subscription, status: String::new() };
		controller().refresh();
		panel.render(dialog);
		panel
	}

	/// Handles the panel's own commands; false for any other control.
	pub fn command(id: u16, notification: u16) -> bool {
		match (id, notification) {
			(ACCOUNT, BN_CLICKED) => controller().press(),
			(BROWSER, BN_CLICKED) => controller().open_browser(),
			(STATUS | ACCOUNT | CODE_LABEL | CODE | BROWSER, _) => {}
			_ => return false,
		}
		true
	}

	/// Shows the login's current state; a new status is announced.
	pub fn refresh(&mut self, dialog: Dialog) {
		let status = self.status.clone();
		self.render(dialog);
		if self.status != status {
			announce(&dialog, STATUS);
		}
	}

	fn render(&mut self, dialog: Dialog) {
		let shown = view(&controller().state());
		// A disabled control can't keep focus; the account button replaced what was pressed.
		if !shown.browser_enabled && dialog.focused() == Some(BROWSER) {
			dialog.focus(ACCOUNT);
		}
		dialog.set_text(STATUS, &shown.status);
		dialog.set_text(ACCOUNT, shown.button);
		dialog.set_text(CODE, &shown.code);
		dialog.enable(BROWSER, shown.browser_enabled);
		self.status = shown.status;
	}
}

impl Drop for AccountPanel {
	fn drop(&mut self) {
		controller().unsubscribe(self.subscription);
	}
}
