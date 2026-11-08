//! The window "Open artist…" shows for a track with several artists: it lists them, as Spotify
//! names them, and opens the one picked as a playlist tab named after it, as [`open_in_tab`]
//! opens any link.
//!
//! What the window lists lives in a main-thread [`CHOICES`]: showing the window sets it, and the
//! window's handler reads it when the window opens and on each refresh. A window that says
//! "Loading…" waits for one lookup, the one [`show_loading`] gave its [`Ticket`]: [`settle`]
//! fills the window with that lookup's artists, or closes it after any other result. Every
//! lookup of every artist holds a ticket, with or without the window, and the newest one's list
//! wins.
//!
//! [`open_in_tab`]: crate::tabs::open_in_tab

use std::cell::{Cell, RefCell};

use foo_librespot_core::related::Opened;
use foxide2k::dialog::host::{Dialog, DialogHandler, destroy, refresh_poster};
use foxide2k::dialog::notify::IDCANCEL;
use foxide2k::dialog::template::DialogTemplate;
use foxide2k::host::host;

use crate::errors::report_failure;
use crate::link_list::{self, Ids, LinkList, is_pick};
use crate::status_line::StatusLine;
use crate::window::SingleWindow;

pub const LABEL: u16 = 401;
pub const LIST: u16 = 402;
pub const STATUS: u16 = 403;

const TITLE: &str = "Open a Spotify artist";

/// The title of the popup that says the window or an artist can't be opened.
const CANT_OPEN: &str = "Can't open this artist";

pub fn window_template() -> DialogTemplate {
	let ids = Ids { label: LABEL, list: LIST, status: STATUS };
	link_list::template(TITLE, "&Artists:", ids, (200, 139), 80)
}

/// The artist window.
pub static WINDOW: SingleWindow<ArtistWindow> = SingleWindow::new(window_template);

thread_local! {
	/// The artists the window lists, in the track's order, or `None` while Spotify is asked for
	/// them. Only foobar2000's main thread sets and reads it.
	static CHOICES: RefCell<Option<Vec<Opened>>> = const { RefCell::new(None) };
	/// The ticket of the lookup the window waits for while it says "Loading…"; `None` once it
	/// lists artists or the user closed it. Only foobar2000's main thread sets and reads it.
	static WAITING: Cell<Option<u64>> = const { Cell::new(None) };
	/// The newest ticket whose lookup the window has listed or waited for. Only foobar2000's
	/// main thread sets and reads it.
	static NEWEST: Cell<u64> = const { Cell::new(0) };
	/// The last ticket given out.
	static LAST_TICKET: Cell<u64> = const { Cell::new(0) };
}

/// The place in line of a lookup of every artist: tickets are given out in the order the
/// lookups start, so the newest lookup's list wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
	number: u64,
	/// Whether the window said "Loading…" for the lookup when it started.
	loading: bool,
}

impl Ticket {
	fn next(loading: bool) -> Self {
		let number = LAST_TICKET.get() + 1;
		LAST_TICKET.set(number);
		Self { number, loading }
	}

	/// Whether the window said "Loading…" for the lookup when it started.
	pub const fn shows_loading(self) -> bool {
		self.loading
	}
}

/// The ticket of a lookup of every artist that starts without the window. Main thread only.
pub fn take_ticket() -> Ticket {
	Ticket::next(false)
}

/// Shows the window saying "Loading…", opening it if it is closed, for the lookup of every
/// artist that starts with the returned ticket. Main thread only.
pub fn show_loading() -> Ticket {
	let ticket = Ticket::next(true);
	WAITING.set(Some(ticket.number));
	NEWEST.set(ticket.number);
	show(None);
	ticket
}

/// Settles the window for the lookup that held `ticket` and ended. When it found several
/// `artists`, the window lists them, opening if it is closed, if it still says "Loading…" for
/// this lookup or the lookup started after every lookup the window has listed or waited for:
/// the newest lookup's list wins, and a lookup whose "Loading…" window the user closed opens
/// nothing. After any other result, the window closes when it still says "Loading…" for this
/// lookup. Main thread only.
pub fn settle(ticket: Ticket, artists: Option<Vec<Opened>>) {
	let waited_for = WAITING.get() == Some(ticket.number);
	match artists {
		Some(artists) if waited_for || ticket.number > NEWEST.get() => {
			WAITING.set(None);
			NEWEST.set(ticket.number);
			show(Some(artists));
		}
		None if waited_for => {
			WAITING.set(None);
			if let Some(dialog) = WINDOW.current() {
				destroy(dialog);
			}
		}
		_ => {}
	}
}

/// Makes the window list `choices`, opening it if it is closed, and brings it up. Says in the
/// console and a popup when it can't open.
fn show(choices: Option<Vec<Opened>>) {
	CHOICES.set(choices);
	if let Some(dialog) = WINDOW.current() {
		refresh_poster(&dialog).post();
	}
	if let Err(error) = WINDOW.show(host().main_window()) {
		report_failure("can't open the artist window", CANT_OPEN, TITLE, &error.message);
	}
}

#[derive(Default)]
pub struct ArtistWindow {
	artists: LinkList,
	status: Option<StatusLine>,
}

impl ArtistWindow {
	/// Shows [`CHOICES`] in the list, the first selected, and the status line; a new status is
	/// announced.
	fn render(&mut self, dialog: Dialog) {
		let (artists, status) = match CHOICES.with_borrow(Clone::clone) {
			Some(artists) => (artists, ""),
			None => (Vec::new(), "Loading…"),
		};
		self.artists.fill(dialog, LIST, artists.into_iter().map(Into::into).collect(), false);
		if let Some(line) = &mut self.status {
			line.show(dialog, status.to_owned());
		}
	}
}

impl DialogHandler for ArtistWindow {
	fn init(&mut self, dialog: &Dialog) -> Option<u16> {
		self.status = Some(StatusLine::new(*dialog, STATUS, String::new()));
		self.render(*dialog);
		Some(LIST)
	}

	fn command(&mut self, dialog: &Dialog, id: u16, notification: u16) {
		if is_pick(LIST, id, notification) {
			self.artists.pick(*dialog, LIST, CANT_OPEN);
		} else if id == IDCANCEL {
			WAITING.set(None);
			destroy(*dialog);
		}
	}

	fn refresh(&mut self, dialog: &Dialog) {
		self.render(*dialog);
	}
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::SpotifyId;
	use foo_librespot_core::link::LinkKind;
	use foo_librespot_core::related::Opened;
	use foxide2k::abi::playlist;
	use foxide2k::dialog::host::{Dialog, destroy};
	use foxide2k::dialog::notify::{BN_CLICKED, IDCANCEL, IDOK, LBN_DBLCLK};
	use foxide2k::dialog::style::BS_DEFPUSHBUTTON;
	use foxide2k::dialog::template::ControlClass;
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::{hidden_window, pump_messages, send_close, send_command};

	use super::{LABEL, LIST, STATUS, WINDOW, settle, show_loading, take_ticket, window_template};
	use crate::testing::{GETTY_TRACK, KEITH_ID, KRISTYN_ID, getty_artists, installed_api};
	use crate::window::brought_up;

	/// Settles the window as a lookup that just started without the window and found the Getty
	/// artists.
	fn list_getty_artists() {
		settle(take_ticket(), Some(getty_artists()));
	}

	/// The artists of another track: Stuart Townend and Keith Getty.
	fn townend_and_keith() -> Vec<Opened> {
		let townend = SpotifyId::from_raw(&[0x53; 16]).unwrap();
		vec![Opened::new("Stuart Townend", LinkKind::Artist, townend), getty_artists().remove(0)]
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
		installed_api();
		list_getty_artists();
		let dialog = WINDOW.current().unwrap();
		assert_eq!(dialog.list_texts(LIST), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(dialog.tab_order(), [LIST, IDOK, IDCANCEL]);
		assert_eq!(dialog.focused(), Some(LIST));
		assert_eq!(dialog.list_selection(LIST), Some(0));
		assert_eq!(dialog.text(STATUS), "");
		assert_eq!(brought_up(), 1);
		destroy(dialog);
	}

	#[test]
	fn loading_says_so_until_the_artists_arrive() {
		installed_api();
		let wait = show_loading();
		let dialog = WINDOW.current().unwrap();
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert_eq!(dialog.list_count(LIST), 0);
		assert_eq!(brought_up(), 1);
		settle(wait, Some(getty_artists()));
		pump_messages();
		assert_eq!(WINDOW.current(), Some(dialog));
		assert_eq!(dialog.list_texts(LIST), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(dialog.list_selection(LIST), Some(0));
		assert_eq!(dialog.text(STATUS), "");
		assert_eq!(brought_up(), 2);
		destroy(dialog);
	}

	#[test]
	fn open_while_loading_does_nothing() {
		let api = installed_api();
		show_loading();
		let dialog = WINDOW.current().unwrap();
		send_command(&dialog, IDOK, BN_CLICKED);
		assert!(dialog.is_alive());
		assert!(api.location_requests().is_empty());
		destroy(dialog);
	}

	#[test]
	fn showing_again_replaces_the_list() {
		installed_api();
		list_getty_artists();
		let dialog = WINDOW.current().unwrap();
		dialog.list_select(LIST, 1);
		settle(take_ticket(), Some(townend_and_keith()));
		pump_messages();
		assert_eq!(WINDOW.current(), Some(dialog));
		assert_eq!(dialog.list_texts(LIST), ["Stuart Townend", "Keith Getty"]);
		assert_eq!(dialog.list_selection(LIST), Some(0));
		assert_eq!(brought_up(), 2);
		destroy(dialog);
	}

	#[test]
	fn picking_an_artist_opens_it_and_closes_the_window() {
		let api = installed_api();
		list_getty_artists();
		let dialog = WINDOW.current().unwrap();
		dialog.list_select(LIST, 1);
		send_command(&dialog, IDOK, BN_CLICKED);
		assert!(!dialog.is_alive());
		assert!(WINDOW.current().is_none());
		assert_eq!(api.location_requests(), [[format!("sptf://spotify:artist:{KRISTYN_ID}")]]);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		assert_eq!(api.tabs().last().unwrap().name, "Kristyn Getty");
	}

	#[test]
	fn double_clicking_an_artist_opens_it() {
		let api = installed_api();
		list_getty_artists();
		let dialog = WINDOW.current().unwrap();
		send_command(&dialog, LIST, LBN_DBLCLK);
		assert!(!dialog.is_alive());
		assert_eq!(api.location_requests(), [[format!("sptf://spotify:artist:{KEITH_ID}")]]);
	}

	#[test]
	fn a_pick_that_cant_be_opened_shows_a_popup_saying_why() {
		let api = installed_api();
		api.remove_services(playlist::MANAGER_IID);
		list_getty_artists();
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
		let api = installed_api();
		list_getty_artists();
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
	fn closing_while_loading_keeps_that_lookups_artists_from_opening_it() {
		installed_api();
		let wait = show_loading();
		send_command(&WINDOW.current().unwrap(), IDCANCEL, BN_CLICKED);
		settle(wait, Some(getty_artists()));
		assert!(WINDOW.current().is_none());
		assert_eq!(brought_up(), 1);
		list_getty_artists();
		let dialog = WINDOW.current().expect("the next artists open the window");
		assert_eq!(dialog.list_texts(LIST), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(brought_up(), 2);
		destroy(dialog);
	}

	#[test]
	fn artists_open_the_window_when_no_loading_window_was_closed() {
		installed_api();
		list_getty_artists();
		send_command(&WINDOW.current().unwrap(), IDCANCEL, BN_CLICKED);
		assert_eq!(brought_up(), 1);
		list_getty_artists();
		let dialog = WINDOW.current().expect("closing a list doesn't stop the next one");
		assert_eq!(dialog.list_texts(LIST), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(brought_up(), 2);
		destroy(dialog);
	}

	#[test]
	fn loading_again_after_closing_waits_for_the_new_lookup() {
		installed_api();
		let closed = show_loading();
		send_command(&WINDOW.current().unwrap(), IDCANCEL, BN_CLICKED);
		let wait = show_loading();
		let dialog = WINDOW.current().unwrap();
		settle(closed, Some(getty_artists()));
		assert_eq!(dialog.list_count(LIST), 0);
		settle(wait, Some(getty_artists()));
		pump_messages();
		assert_eq!(WINDOW.current(), Some(dialog));
		assert_eq!(dialog.list_texts(LIST), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(brought_up(), 3);
		destroy(dialog);
	}

	#[test]
	fn a_later_loading_takes_the_window_over_from_the_earlier_lookup() {
		installed_api();
		let earlier = show_loading();
		let dialog = WINDOW.current().unwrap();
		let later = show_loading();
		settle(earlier, None);
		settle(earlier, Some(getty_artists()));
		pump_messages();
		assert_eq!(WINDOW.current(), Some(dialog));
		assert_eq!(dialog.text(STATUS), "Loading…");
		settle(later, None);
		assert!(!dialog.is_alive());
	}

	#[test]
	fn another_result_closes_only_the_loading_window_it_waits_for() {
		installed_api();
		let wait = show_loading();
		let loading = WINDOW.current().unwrap();
		settle(wait, None);
		assert!(!loading.is_alive());
		assert!(WINDOW.current().is_none());
		list_getty_artists();
		let listing = WINDOW.current().expect("a window closed by a result opens again");
		settle(wait, None);
		assert!(listing.is_alive());
		assert_eq!(listing.list_texts(LIST), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(brought_up(), 2);
		destroy(listing);
	}

	#[test]
	fn a_result_without_a_wait_leaves_a_loading_window_alone() {
		installed_api();
		let wait = show_loading();
		let dialog = WINDOW.current().unwrap();
		settle(take_ticket(), None);
		assert_eq!(WINDOW.current(), Some(dialog));
		assert_eq!(dialog.text(STATUS), "Loading…");
		settle(wait, Some(getty_artists()));
		pump_messages();
		assert_eq!(dialog.list_texts(LIST), ["Keith Getty", "Kristyn Getty"]);
		destroy(dialog);
	}

	#[test]
	fn an_older_lookups_list_leaves_a_newer_lookups_loading_window_waiting() {
		installed_api();
		let older = take_ticket();
		let wait = show_loading();
		let dialog = WINDOW.current().unwrap();
		settle(older, Some(townend_and_keith()));
		pump_messages();
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert_eq!(dialog.list_count(LIST), 0);
		settle(wait, Some(getty_artists()));
		pump_messages();
		assert_eq!(dialog.list_texts(LIST), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(brought_up(), 2);
		destroy(dialog);
	}

	#[test]
	fn an_older_lookups_list_doesnt_replace_a_newer_lookups_list() {
		installed_api();
		let older = take_ticket();
		settle(take_ticket(), Some(getty_artists()));
		let dialog = WINDOW.current().unwrap();
		settle(older, Some(townend_and_keith()));
		pump_messages();
		assert_eq!(dialog.list_texts(LIST), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(brought_up(), 1);
		destroy(dialog);
	}

	#[test]
	fn a_newer_lookups_list_takes_the_window_over_from_a_loading_lookup() {
		installed_api();
		let wait = show_loading();
		let dialog = WINDOW.current().unwrap();
		settle(take_ticket(), Some(townend_and_keith()));
		settle(wait, Some(getty_artists()));
		settle(wait, None);
		pump_messages();
		assert!(dialog.is_alive());
		assert_eq!(dialog.list_texts(LIST), ["Stuart Townend", "Keith Getty"]);
		destroy(dialog);
	}

	#[test]
	fn a_result_for_a_closed_loading_window_changes_nothing() {
		installed_api();
		let wait = show_loading();
		send_command(&WINDOW.current().unwrap(), IDCANCEL, BN_CLICKED);
		settle(wait, None);
		list_getty_artists();
		let dialog = WINDOW.current().expect("a later lookup's artists open the window");
		assert_eq!(brought_up(), 2);
		destroy(dialog);
	}

	#[test]
	fn a_window_that_cant_open_says_why() {
		let api = installed_api();
		let gone = hidden_window();
		destroy(Dialog::from_hwnd(gone));
		api.set_main_window(gone);
		list_getty_artists();
		assert!(WINDOW.current().is_none());
		assert_eq!(brought_up(), 0);
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
