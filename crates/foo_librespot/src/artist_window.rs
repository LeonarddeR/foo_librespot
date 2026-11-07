//! The shortcut-only "Open artist…": a window that lists a track's artists, as Spotify names
//! them, and opens the one picked as a playlist tab named after it, as [`open_in_tab`] opens any
//! link.
//!
//! What the window lists lives in a main-thread [`CHOICES`]: showing the window sets it, and the
//! window's handler reads it when the window opens and on each refresh.

use std::cell::RefCell;
use std::sync::OnceLock;

use foo_librespot_core::related::Opened;
use foxide2k::console;
use foxide2k::dialog::host::{Dialog, DialogHandler, destroy, refresh_poster};
use foxide2k::dialog::notify::{BN_CLICKED, IDCANCEL, IDOK, LBN_DBLCLK};
use foxide2k::dialog::style::{
	BS_DEFPUSHBUTTON, DS_CENTER, DS_MODALFRAME, LBS_NOTIFY, SS_NOPREFIX, WS_BORDER, WS_CAPTION,
	WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VSCROLL,
};
use foxide2k::dialog::template::{ControlClass, DialogTemplate, shell_font};
use foxide2k::host::host;
use foxide2k::popup::PopupIcon;

use crate::account::item;
use crate::errors::show_failure;
use crate::status_line::StatusLine;
use crate::tabs::open_in_tab;
use crate::window::SingleWindow;

pub const LABEL: u16 = 401;
pub const LIST: u16 = 402;
pub const STATUS: u16 = 403;

const TITLE: &str = "Open a Spotify artist";

pub fn window_template() -> DialogTemplate {
	DialogTemplate {
		style: WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME | DS_CENTER,
		title: TITLE.to_owned(),
		cx: 200,
		cy: 139,
		font: shell_font(),
		items: vec![
			item(LABEL, ControlClass::Static, "&Artists:", 0, (7, 7, 186, 10)),
			item(
				LIST,
				ControlClass::ListBox,
				"",
				WS_TABSTOP | WS_VSCROLL | WS_BORDER | LBS_NOTIFY,
				(7, 19, 186, 80),
			),
			item(STATUS, ControlClass::Static, "", SS_NOPREFIX, (7, 103, 186, 10)),
			item(
				IDOK,
				ControlClass::Button,
				"&Open",
				BS_DEFPUSHBUTTON | WS_TABSTOP,
				(55, 118, 66, 14),
			),
			item(IDCANCEL, ControlClass::Button, "Close", WS_TABSTOP, (127, 118, 66, 14)),
		],
	}
}

/// The artist window.
pub static WINDOW: SingleWindow<ArtistWindow> = SingleWindow::new(window_template);

thread_local! {
	/// The artists the window lists, in the track's order, or `None` while Spotify is asked for
	/// them. Only foobar2000's main thread sets and reads it.
	static CHOICES: RefCell<Option<Vec<Opened>>> = const { RefCell::new(None) };
}

/// Where the artist window appears. Called on foobar2000's main thread.
pub trait Screen: Send + Sync {
	/// The window that owns the artist window.
	fn owner(&self) -> isize;
	/// Makes `dialog` visible and brings it to the front.
	fn bring_up(&self, dialog: Dialog);
}

/// foobar2000's main window owns the artist window, which shows in front of it.
struct MainWindow;

impl Screen for MainWindow {
	fn owner(&self) -> isize {
		host().main_window()
	}

	fn bring_up(&self, dialog: Dialog) {
		dialog.show();
		dialog.activate();
	}
}

static SCREEN: OnceLock<Box<dyn Screen>> = OnceLock::new();

/// The screen in use: foobar2000's, or the one a test installed.
fn screen() -> &'static dyn Screen {
	SCREEN.get_or_init(|| Box::new(MainWindow)).as_ref()
}

/// Makes `screen()` return `screen` in this test process.
#[cfg(test)]
pub fn install_screen(screen: Box<dyn Screen>) {
	assert!(SCREEN.set(screen).is_ok(), "a screen was already in use");
}

/// Shows the window saying "Loading…", opening it if it is closed. Main thread only.
#[cfg_attr(
	not(test),
	expect(
		dead_code,
		reason = "the shortcut-only Open artist… will show it while Spotify is asked"
	)
)]
pub fn show_loading() {
	show(None);
}

/// Shows the window listing `artists`, opening it if it is closed. Main thread only.
pub fn show_choices(artists: Vec<Opened>) {
	show(Some(artists));
}

/// Makes the window list `choices`, opening it if it is closed, and brings it up. Says in the
/// console and a popup when it can't open.
fn show(choices: Option<Vec<Opened>>) {
	CHOICES.set(choices);
	let screen = screen();
	let dialog = if let Some(dialog) = WINDOW.current() {
		refresh_poster(&dialog).post();
		dialog
	} else {
		match WINDOW.open(screen.owner()) {
			Ok(dialog) => dialog,
			Err(error) => return cant_show(&error.message),
		}
	};
	screen.bring_up(dialog);
}

/// Says in the console and a popup that the window can't open, and why.
fn cant_show(why: &str) {
	console::print(&format!("foo_librespot: can't open the artist window: {why}"));
	show_failure("Can't open this artist", PopupIcon::Error, TITLE, &why);
}

/// Says in the console and a popup that the artist `name` couldn't be opened, and why.
fn cant_open(name: &str, why: &str) {
	console::print(&format!("foo_librespot: can't open {name}: {why}"));
	show_failure("Can't open this artist", PopupIcon::Error, name, &why);
}

#[derive(Default)]
pub struct ArtistWindow {
	/// The artists the list shows, row by row.
	artists: Vec<Opened>,
	status: Option<StatusLine>,
}

impl ArtistWindow {
	/// Shows [`CHOICES`] in the list and the status line; a new status is announced.
	fn render(&mut self, dialog: Dialog) {
		let (artists, status) = match CHOICES.with_borrow(Clone::clone) {
			Some(artists) => (artists, ""),
			None => (Vec::new(), "Loading…"),
		};
		self.fill(dialog, artists);
		if let Some(line) = &mut self.status {
			line.show(dialog, status.to_owned());
		}
	}

	/// Shows `artists` in the list with the first selected. An unchanged list is left alone.
	fn fill(&mut self, dialog: Dialog, artists: Vec<Opened>) {
		if artists == self.artists {
			return;
		}
		dialog.list_clear(LIST);
		for artist in &artists {
			dialog.list_add(LIST, &artist.name);
		}
		self.artists = artists;
		if !self.artists.is_empty() {
			dialog.list_select(LIST, 0);
		}
	}

	/// Opens the selected artist and closes the window; does nothing when none is selected.
	fn pick(&self, dialog: Dialog) {
		let Some(artist) = dialog.list_selection(LIST).and_then(|row| self.artists.get(row)) else {
			return;
		};
		if let Err(error) = open_in_tab(&artist.name, &artist.link) {
			cant_open(&artist.name, &error.message);
		}
		destroy(dialog);
	}
}

impl DialogHandler for ArtistWindow {
	fn init(&mut self, dialog: &Dialog) -> Option<u16> {
		self.status = Some(StatusLine::new(*dialog, STATUS, String::new()));
		self.render(*dialog);
		Some(LIST)
	}

	fn command(&mut self, dialog: &Dialog, id: u16, notification: u16) {
		match (id, notification) {
			(IDOK, BN_CLICKED) | (LIST, LBN_DBLCLK) => self.pick(*dialog),
			(IDCANCEL, _) => destroy(*dialog),
			_ => {}
		}
	}

	fn refresh(&mut self, dialog: &Dialog) {
		self.render(*dialog);
	}
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::SpotifyId;
	use foo_librespot_core::link::{Link, LinkKind};
	use foo_librespot_core::related::Opened;
	use foxide2k::abi::playlist;
	use foxide2k::dialog::host::{Dialog, destroy};
	use foxide2k::dialog::notify::{BN_CLICKED, IDCANCEL, IDOK, LBN_DBLCLK};
	use foxide2k::dialog::style::BS_DEFPUSHBUTTON;
	use foxide2k::dialog::template::ControlClass;
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::{FakeApi, pump_messages, send_close, send_command};

	use super::{LABEL, LIST, STATUS, WINDOW, show_choices, show_loading, window_template};
	use crate::testing::{FakeScreen, installed_screen};

	const GETTY_TRACK: &str = "sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO";
	const KRISTYN: &str = "2kxP07DLgs4xlWz8YHlvfh";

	fn artist(name: &str, id: SpotifyId) -> Opened {
		Opened { name: name.to_owned(), link: Link::Id { kind: LinkKind::Artist, id } }
	}

	/// The artists of "In Christ Alone", in the track's order.
	fn getty_artists() -> Vec<Opened> {
		vec![
			artist("Keith Getty", SpotifyId::from_base62("0I4Bk2s2BUJyykCwtxx8Xx").unwrap()),
			artist("Kristyn Getty", SpotifyId::from_base62(KRISTYN).unwrap()),
		]
	}

	fn installed() -> (&'static FakeApi, FakeScreen) {
		let api = FakeApi::new();
		api.install();
		(api, installed_screen())
	}

	fn texts(dialog: Dialog) -> Vec<String> {
		(0..dialog.list_count(LIST)).map(|row| dialog.list_text(LIST, row)).collect()
	}

	#[test]
	fn the_template_puts_the_label_right_before_the_list() {
		let template = window_template();
		assert_eq!(template.title, "Open a Spotify artist");
		let ids: Vec<u16> = template.items.iter().map(|item| item.id).collect();
		assert_eq!(ids, [LABEL, LIST, STATUS, IDOK, IDCANCEL]);
		assert_eq!(template.items[0].text, "&Artists:");
		assert_eq!(template.items[1].class, ControlClass::ListBox);
		assert_eq!(template.items[3].text, "&Open");
		assert_ne!(template.items[3].style & BS_DEFPUSHBUTTON, 0);
		assert_eq!(template.items[4].text, "Close");
	}

	#[test]
	fn the_window_lists_the_artists_in_order() {
		let (_, screen) = installed();
		show_choices(getty_artists());
		let dialog = WINDOW.current().unwrap();
		assert_eq!(texts(dialog), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(dialog.tab_order(), [LIST, IDOK, IDCANCEL]);
		assert_eq!(dialog.focused(), Some(LIST));
		assert_eq!(dialog.list_selection(LIST), Some(0));
		assert_eq!(dialog.text(STATUS), "");
		assert_eq!(screen.brought_up(), 1);
		destroy(dialog);
	}

	#[test]
	fn loading_says_so_until_the_artists_arrive() {
		let (_, screen) = installed();
		show_loading();
		let dialog = WINDOW.current().unwrap();
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert_eq!(dialog.list_count(LIST), 0);
		assert_eq!(screen.brought_up(), 1);
		show_choices(getty_artists());
		pump_messages();
		assert_eq!(WINDOW.current(), Some(dialog));
		assert_eq!(texts(dialog), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(dialog.list_selection(LIST), Some(0));
		assert_eq!(dialog.text(STATUS), "");
		assert_eq!(screen.brought_up(), 2);
		destroy(dialog);
	}

	#[test]
	fn open_while_loading_does_nothing() {
		let (api, _) = installed();
		show_loading();
		let dialog = WINDOW.current().unwrap();
		send_command(&dialog, IDOK, BN_CLICKED);
		assert!(dialog.is_alive());
		assert!(api.location_requests().is_empty());
		destroy(dialog);
	}

	#[test]
	fn showing_again_replaces_the_list() {
		let (_, screen) = installed();
		show_choices(getty_artists());
		let dialog = WINDOW.current().unwrap();
		dialog.list_select(LIST, 1);
		let townend = artist("Stuart Townend", SpotifyId::from_raw(&[0x53; 16]).unwrap());
		show_choices(vec![townend, getty_artists().remove(0)]);
		pump_messages();
		assert_eq!(WINDOW.current(), Some(dialog));
		assert_eq!(texts(dialog), ["Stuart Townend", "Keith Getty"]);
		assert_eq!(dialog.list_selection(LIST), Some(0));
		assert_eq!(screen.brought_up(), 2);
		destroy(dialog);
	}

	#[test]
	fn picking_an_artist_opens_it_and_closes_the_window() {
		let (api, _) = installed();
		show_choices(getty_artists());
		let dialog = WINDOW.current().unwrap();
		dialog.list_select(LIST, 1);
		send_command(&dialog, IDOK, BN_CLICKED);
		assert!(!dialog.is_alive());
		assert!(WINDOW.current().is_none());
		assert_eq!(api.location_requests(), [[format!("sptf://spotify:artist:{KRISTYN}")]]);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		assert_eq!(api.tabs().last().unwrap().name, "Kristyn Getty");
	}

	#[test]
	fn double_clicking_an_artist_opens_it() {
		let (api, _) = installed();
		show_choices(getty_artists());
		let dialog = WINDOW.current().unwrap();
		send_command(&dialog, LIST, LBN_DBLCLK);
		assert!(!dialog.is_alive());
		assert_eq!(
			api.location_requests(),
			[["sptf://spotify:artist:0I4Bk2s2BUJyykCwtxx8Xx".to_owned()]]
		);
	}

	#[test]
	fn a_pick_that_cant_be_opened_shows_a_popup_saying_why() {
		let (api, _) = installed();
		api.remove_services(playlist::MANAGER_IID);
		show_choices(getty_artists());
		let dialog = WINDOW.current().unwrap();
		dialog.list_select(LIST, 1);
		send_command(&dialog, IDOK, BN_CLICKED);
		assert!(!dialog.is_alive());
		assert_eq!(
			api.popups(),
			[(
				"Can't open this artist".to_owned(),
				"Kristyn Getty\n\nfoobar2000 has no playlist_manager.".to_owned(),
				PopupIcon::Error
			)]
		);
		assert!(
			api.console_lines().contains(
				&"foo_librespot: can't open Kristyn Getty: foobar2000 has no playlist_manager"
					.to_owned()
			)
		);
	}

	#[test]
	fn closing_the_window_destroys_it() {
		let (api, _) = installed();
		show_choices(getty_artists());
		let dialog = WINDOW.current().unwrap();
		send_command(&dialog, IDCANCEL, BN_CLICKED);
		assert!(!dialog.is_alive());
		assert!(WINDOW.current().is_none());
		show_loading();
		let again = WINDOW.current().unwrap();
		send_close(&again);
		pump_messages();
		assert!(!again.is_alive());
		assert!(api.location_requests().is_empty());
	}

	#[test]
	fn a_window_that_cant_open_says_why() {
		let (api, screen) = installed();
		screen.lose_owner();
		show_choices(getty_artists());
		assert!(WINDOW.current().is_none());
		assert_eq!(screen.brought_up(), 0);
		let popups = api.popups();
		assert_eq!(popups.len(), 1, "{popups:?}");
		let (title, message, icon) = &popups[0];
		assert_eq!(title, "Can't open this artist");
		assert!(
			message.starts_with("Open a Spotify artist\n\ncan't create a dialog: "),
			"{message}"
		);
		assert_eq!(*icon, PopupIcon::Error);
		assert!(
			api.console_lines().iter().any(|line| line.starts_with(
				"foo_librespot: can't open the artist window: can't create a dialog: "
			)),
			"{:?}",
			api.console_lines()
		);
	}
}
