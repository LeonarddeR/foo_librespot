//! The "Log in to Spotify" window, which opens when a link needs a login the user hasn't done
//! yet. It shows the same account controls as the preferences page.
//!
//! It opens with the login already started and focus on the code field, so the account button
//! (which then reads "Cancel login") can't be pressed by accident. It has no default button:
//! Enter in the code field does nothing. Close and Escape cancel a login still waiting.

use std::cell::Cell;

use oxibar::dialog::host::{Dialog, DialogHandler, create_modeless_dialog, destroy};
use oxibar::dialog::notify::IDCANCEL;
use oxibar::dialog::style::{
	DS_CENTER, DS_MODALFRAME, WS_CAPTION, WS_POPUP, WS_SYSMENU, WS_TABSTOP,
};
use oxibar::dialog::template::{ControlClass, DialogTemplate};
use oxibar::error::Result;
use oxibar::main_thread::{add_modeless, remove_modeless};

use crate::account::{self, AccountPanel, CODE, item};
use crate::login::controller;

pub fn window_template() -> DialogTemplate {
	let mut items = account::items(7, 7, 246);
	items.push(item(IDCANCEL, ControlClass::Button, "Close", WS_TABSTOP, (169, 110, 84, 14)));
	DialogTemplate {
		style: WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME | DS_CENTER,
		title: "Log in to Spotify".to_owned(),
		cx: 260,
		cy: 130,
		font: (8, "MS Shell Dlg".to_owned()),
		items,
	}
}

thread_local! {
	/// The open window. It lives on foobar2000's main thread, so only that thread sees it.
	static WINDOW: Cell<Option<Dialog>> = const { Cell::new(None) };
}

/// The open login window, if any.
pub fn current() -> Option<Dialog> {
	WINDOW.get().filter(Dialog::is_alive)
}

/// Creates the window, hidden, unless it is open already. Main thread only.
pub fn open(owner: isize) -> Result<Dialog> {
	if let Some(dialog) = current() {
		return Ok(dialog);
	}
	let dialog =
		create_modeless_dialog(owner, &window_template(), Box::new(LoginWindow::default()))?;
	WINDOW.set(Some(dialog));
	add_modeless(dialog.hwnd());
	Ok(dialog)
}

/// Shows the login window, or brings it to the front when it is open. Main thread only.
pub fn show(owner: isize) -> Result<()> {
	let dialog = open(owner)?;
	dialog.show();
	dialog.activate();
	Ok(())
}

#[derive(Default)]
struct LoginWindow {
	dialog: Option<Dialog>,
	panel: Option<AccountPanel>,
}

impl DialogHandler for LoginWindow {
	fn init(&mut self, dialog: &Dialog) -> Option<u16> {
		self.dialog = Some(*dialog);
		self.panel = Some(AccountPanel::init(*dialog));
		Some(CODE)
	}

	fn command(&mut self, dialog: &Dialog, id: u16, notification: u16) {
		if AccountPanel::command(id, notification) {
			return;
		}
		if id == IDCANCEL {
			// Does nothing once the login has ended.
			controller().cancel();
			destroy(*dialog);
		}
	}

	fn refresh(&mut self, dialog: &Dialog) {
		if let Some(panel) = &mut self.panel {
			panel.refresh(*dialog);
		}
	}
}

impl Drop for LoginWindow {
	/// Runs when the window is destroyed, also when foobar2000 quits with it open.
	fn drop(&mut self) {
		if let Some(dialog) = self.dialog {
			remove_modeless(dialog.hwnd());
			if WINDOW.get() == Some(dialog) {
				WINDOW.set(None);
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use oxibar::dialog::host::destroy;
	use oxibar::dialog::notify::IDCANCEL;
	use oxibar::testing::{hidden_window, pump_messages, send_close};

	use super::{current, open};
	use crate::account::{ACCOUNT, BROWSER, CODE};
	use crate::backend;
	use crate::login::{LoginState, controller};
	use crate::testing::{FakeBackend, device_code};

	fn installed() -> FakeBackend {
		let fake = FakeBackend::default();
		backend::install(Box::new(fake.clone()));
		fake
	}

	#[test]
	fn login_window_tab_order_and_first_focus() {
		let fake = installed();
		controller().start();
		fake.send_code(device_code());
		let dialog = open(hidden_window()).unwrap();
		assert_eq!(dialog.tab_order(), [ACCOUNT, CODE, BROWSER, IDCANCEL]);
		assert_eq!(dialog.focused(), Some(CODE));
		assert_eq!(dialog.text(IDCANCEL), "Close");
		destroy(dialog);
	}

	#[test]
	fn closing_the_login_window_cancels_a_waiting_login() {
		let fake = installed();
		controller().start();
		let dialog = open(hidden_window()).unwrap();
		send_close(&dialog);
		pump_messages();
		assert_eq!(controller().state(), LoginState::LoggedOut);
		assert!(fake.last_login_cancelled());
		assert!(!dialog.is_alive());
		assert!(current().is_none());
	}

	#[test]
	fn closing_after_login_keeps_the_login() {
		let fake = installed();
		controller().start();
		let dialog = open(hidden_window()).unwrap();
		fake.send_code(device_code());
		fake.finish_login(Ok("Keith Getty".to_owned()));
		pump_messages();
		assert_eq!(dialog.text(ACCOUNT), "Log &out");
		send_close(&dialog);
		pump_messages();
		assert_eq!(controller().state(), LoginState::LoggedIn(Some("Keith Getty".to_owned())));
		assert!(!dialog.is_alive());
	}

	#[test]
	fn showing_twice_keeps_one_window() {
		let _fake = installed();
		let owner = hidden_window();
		let first = open(owner).unwrap();
		let second = open(owner).unwrap();
		assert_eq!(first, second);
		assert_eq!(current(), Some(first));
		destroy(first);
		assert!(current().is_none());
	}
}
