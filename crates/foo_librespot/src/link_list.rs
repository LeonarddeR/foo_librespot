//! What "Open playlist…" and "Open artist…" share: a window that lists Spotify links under a
//! label, with a status line, Open and Close. Open, Enter or a double click opens the link picked
//! in a playlist tab named after it, as [`open_in_tab`] opens any link, and closes the window.

use foo_librespot_core::link::Link;
use foo_librespot_core::related::Opened;
use foxide2k::dialog::host::{Dialog, destroy};
use foxide2k::dialog::notify::{BN_CLICKED, IDCANCEL, IDOK, LBN_DBLCLK};
use foxide2k::dialog::style::{
	BS_DEFPUSHBUTTON, DS_CENTER, DS_MODALFRAME, LBS_NOTIFY, SS_NOPREFIX, WS_BORDER, WS_CAPTION,
	WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VSCROLL,
};
use foxide2k::dialog::template::{ControlClass, DialogTemplate, shell_font};

use crate::account::item;
use crate::errors::report_failure;
use crate::tabs::open_in_tab;

/// The ids of a window's label, list and status line; Open and Close are `IDOK` and `IDCANCEL`.
#[derive(Clone, Copy)]
pub struct Ids {
	pub label: u16,
	pub list: u16,
	pub status: u16,
}

/// A window titled `title`, `cx` by `cy` dialog units, whose list is `list_height` high and
/// labelled `label`. Each control is made in tab order, the label right before its list.
pub fn template(
	title: &str,
	label: &str,
	ids: Ids,
	(cx, cy): (i16, i16),
	list_height: i16,
) -> DialogTemplate {
	let width = cx - 14;
	let status_y = 23 + list_height;
	let buttons_y = status_y + 15;
	DialogTemplate {
		style: WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME | DS_CENTER,
		title: title.to_owned(),
		cx,
		cy,
		font: shell_font(),
		items: vec![
			item(ids.label, ControlClass::Static, label, 0, (7, 7, width, 10)),
			item(
				ids.list,
				ControlClass::ListBox,
				"",
				WS_TABSTOP | WS_VSCROLL | WS_BORDER | LBS_NOTIFY,
				(7, 19, width, list_height),
			),
			item(ids.status, ControlClass::Static, "", SS_NOPREFIX, (7, status_y, width, 10)),
			item(
				IDOK,
				ControlClass::Button,
				"&Open",
				BS_DEFPUSHBUTTON | WS_TABSTOP,
				(cx - 145, buttons_y, 66, 14),
			),
			item(IDCANCEL, ControlClass::Button, "Close", WS_TABSTOP, (cx - 73, buttons_y, 66, 14)),
		],
	}
}

/// Whether control `id` sent `notification` to open the link picked in `list`: Open was pressed
/// or the list was double-clicked.
pub fn is_pick(list: u16, id: u16, notification: u16) -> bool {
	(id, notification) == (IDOK, BN_CLICKED) || (id, notification) == (list, LBN_DBLCLK)
}

/// One line of a list: the text it shows, the name of the tab it opens and where its tracks come
/// from.
#[derive(PartialEq, Eq)]
pub struct Entry {
	pub text: String,
	pub name: String,
	pub link: Link,
}

impl From<Opened> for Entry {
	/// The line of a tab Spotify named: it shows the tab's name.
	fn from(opened: Opened) -> Self {
		Self { text: opened.name.clone(), name: opened.name, link: opened.link }
	}
}

/// The entries list `id` shows, row by row.
#[derive(Default)]
pub struct LinkList {
	entries: Vec<Entry>,
}

impl LinkList {
	/// Shows `entries` in list `id`. An unchanged list is left alone. A changed one is refilled
	/// with its first entry selected, or with the entry of the link that was selected when
	/// `keep_pick` and that link is still listed.
	pub fn fill(&mut self, dialog: Dialog, id: u16, entries: Vec<Entry>, keep_pick: bool) {
		if entries == self.entries {
			return;
		}
		let picked = keep_pick.then(|| self.selected(dialog, id)).flatten().map(|e| e.link.clone());
		dialog.list_clear(id);
		for entry in &entries {
			dialog.list_add(id, &entry.text);
		}
		let row = entries.iter().position(|entry| Some(&entry.link) == picked.as_ref());
		self.entries = entries;
		if !self.entries.is_empty() {
			dialog.list_select(id, row.unwrap_or(0));
		}
	}

	/// Opens the entry selected in list `id` and closes the window; does nothing when none is
	/// selected. When it can't be opened, the console and an error popup titled `title` say why.
	pub fn pick(&self, dialog: Dialog, id: u16, title: &str) {
		let Some(entry) = self.selected(dialog, id) else {
			return;
		};
		if let Err(error) = open_in_tab(&entry.name, &entry.link) {
			let name = &entry.name;
			report_failure(&format!("can't open {name}"), title, name, &error.message);
		}
		destroy(dialog);
	}

	fn selected(&self, dialog: Dialog, id: u16) -> Option<&Entry> {
		dialog.list_selection(id).and_then(|row| self.entries.get(row))
	}
}

#[cfg(test)]
mod tests {
	use foxide2k::dialog::notify::{IDCANCEL, IDOK};

	use super::{Ids, template};

	const IDS: Ids = Ids { label: 1, list: 2, status: 3 };

	/// A control's id, and its place: x, y, width and height.
	type Place = (u16, i16, i16, i16, i16);

	/// The places of the controls of a window `cx` by `cy` with a list `list_height` high.
	fn places(cx: i16, cy: i16, list_height: i16) -> Vec<Place> {
		let made = template("Title", "&Label:", IDS, (cx, cy), list_height);
		assert_eq!((made.cx, made.cy), (cx, cy));
		made.items.iter().map(|item| (item.id, item.x, item.y, item.cx, item.cy)).collect()
	}

	#[test]
	fn the_status_and_buttons_sit_below_the_list_and_the_buttons_at_the_right() {
		assert_eq!(
			places(200, 139, 80),
			[
				(1, 7, 7, 186, 10),
				(2, 7, 19, 186, 80),
				(3, 7, 103, 186, 10),
				(IDOK, 55, 118, 66, 14),
				(IDCANCEL, 127, 118, 66, 14),
			]
		);
		assert_eq!(
			places(260, 200, 140),
			[
				(1, 7, 7, 246, 10),
				(2, 7, 19, 246, 140),
				(3, 7, 163, 246, 10),
				(IDOK, 115, 178, 66, 14),
				(IDCANCEL, 187, 178, 66, 14),
			]
		);
	}
}
