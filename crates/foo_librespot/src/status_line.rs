//! A dialog's status line: a polite live region, so a screen reader reads each new status.

#[cfg(test)]
use std::cell::RefCell;

use foxide2k::console;
use foxide2k_ui::accessibility::{announce, make_live_region};
use foxide2k_ui::host::Dialog;

/// The status line of one open dialog, and the text it shows.
pub struct StatusLine {
	id: u16,
	text: String,
}

impl StatusLine {
	/// Makes control `id` of `dialog` a live region showing `text`, without announcing it. Says
	/// in the console when it can't be one.
	pub fn new(dialog: Dialog, id: u16, text: String) -> Self {
		if let Err(error) = make_live_region(&dialog, id) {
			console::print(&format!("foo_librespot: {}", error.message));
		}
		dialog.set_text(id, &text);
		Self { id, text }
	}

	/// Shows `text`, and announces it when it isn't the text shown before.
	pub fn show(&mut self, dialog: Dialog, text: String) {
		let changed = text != self.text;
		self.set(dialog, text);
		if changed {
			self.announce(dialog);
		}
	}

	/// Shows `text` and announces it, also when it is the text shown before: for the result of
	/// something the user just did, which is news even when it is the same as last time.
	pub fn tell(&mut self, dialog: Dialog, text: String) {
		self.set(dialog, text);
		self.announce(dialog);
	}

	fn set(&mut self, dialog: Dialog, text: String) {
		dialog.set_text(self.id, &text);
		self.text = text;
	}

	/// Announces the text shown.
	fn announce(&self, dialog: Dialog) {
		announce(&dialog, self.id);
		#[cfg(test)]
		ANNOUNCED.with_borrow_mut(|announced| announced.push(self.text.clone()));
	}
}

#[cfg(test)]
thread_local! {
	/// Each text a status line announced on this thread, in order.
	static ANNOUNCED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Each text a status line announced on this thread, in order.
#[cfg(test)]
pub fn announced() -> Vec<String> {
	ANNOUNCED.with_borrow(Clone::clone)
}

#[cfg(test)]
mod tests {
	use foxide2k::testing::FakeApi;
	use foxide2k_ui::host::{Dialog, DialogHandler, create_modeless_dialog, destroy};
	use foxide2k_ui::style::{SS_NOPREFIX, WS_CAPTION, WS_POPUP};
	use foxide2k_ui::template::{ControlClass, DialogItem, DialogTemplate, shell_font};
	use foxide2k_ui::testing::hidden_window;

	use super::{StatusLine, announced};

	const STATUS: u16 = 101;

	struct Empty;

	impl DialogHandler for Empty {
		fn init(&mut self, _dialog: &Dialog) -> Option<u16> {
			None
		}

		fn command(&mut self, _dialog: &Dialog, _id: u16, _notification: u16) {}

		fn refresh(&mut self, _dialog: &Dialog) {}
	}

	fn dialog() -> Dialog {
		let template = DialogTemplate {
			style: WS_POPUP | WS_CAPTION,
			title: "Status".to_owned(),
			cx: 100,
			cy: 30,
			font: shell_font(),
			items: vec![DialogItem {
				id: STATUS,
				class: ControlClass::Static,
				text: String::new(),
				style: SS_NOPREFIX,
				x: 7,
				y: 7,
				cx: 86,
				cy: 10,
			}],
		};
		create_modeless_dialog(hidden_window(), &template, Box::new(Empty)).unwrap()
	}

	#[test]
	fn the_status_shows_each_text_and_says_nothing_when_it_works() {
		let api = FakeApi::new();
		api.install();
		let dialog = dialog();
		let mut status = StatusLine::new(dialog, STATUS, "Loading…".to_owned());
		assert_eq!(dialog.text(STATUS), "Loading…");
		status.show(dialog, "Loading…".to_owned());
		status.show(dialog, "Couldn't load your playlists.".to_owned());
		assert_eq!(dialog.text(STATUS), "Couldn't load your playlists.");
		assert!(api.console_lines().is_empty(), "{:?}", api.console_lines());
		destroy(dialog);
	}

	#[test]
	fn show_announces_only_a_new_text_and_tell_announces_every_time() {
		let api = FakeApi::new();
		api.install();
		let dialog = dialog();
		let mut status = StatusLine::new(dialog, STATUS, "Found 2 tracks.".to_owned());
		status.show(dialog, "Found 2 tracks.".to_owned());
		assert!(announced().is_empty());
		status.tell(dialog, "Added 1 track to Default.".to_owned());
		status.tell(dialog, "Added 1 track to Default.".to_owned());
		assert_eq!(dialog.text(STATUS), "Added 1 track to Default.");
		status.show(dialog, "Added 1 track to Default.".to_owned());
		status.show(dialog, "Searching…".to_owned());
		assert_eq!(
			announced(),
			["Added 1 track to Default.", "Added 1 track to Default.", "Searching…"]
		);
		destroy(dialog);
	}
}
