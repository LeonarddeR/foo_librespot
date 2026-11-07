//! A dialog's status line: a polite live region, so a screen reader reads each new status.

use foxide2k::console;
use foxide2k::dialog::accessibility::{announce, make_live_region};
use foxide2k::dialog::host::Dialog;

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
		dialog.set_text(self.id, &text);
		if text != self.text {
			announce(&dialog, self.id);
		}
		self.text = text;
	}
}

#[cfg(test)]
mod tests {
	use foxide2k::dialog::host::{Dialog, DialogHandler, create_modeless_dialog, destroy};
	use foxide2k::dialog::style::{SS_NOPREFIX, WS_CAPTION, WS_POPUP};
	use foxide2k::dialog::template::{ControlClass, DialogItem, DialogTemplate, shell_font};
	use foxide2k::testing::{FakeApi, hidden_window};

	use super::StatusLine;

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
}
