//! File > Spotify > "Open playlist…": a window that lists Liked Songs and the playlists of the
//! Spotify library, and opens the one picked as a playlist tab named after it, as
//! [`open_in_tab`] opens any link.
//!
//! [`open_in_tab`]: crate::tabs::open_in_tab

use foo_librespot_core::library::{LIKED_SONGS, Library, LibraryState};
use foo_librespot_core::link::Link;
use foxide2k::abi::mainmenu::{GROUP_FILE, SORT_PRIORITY_BASE};
use foxide2k::error::Result;
use foxide2k::guid::Guid;
use foxide2k::mainmenu::{MainMenuCommand, MainMenuPopup};
use foxide2k_ui::host::{Dialog, DialogHandler, destroy};
use foxide2k_ui::notify::IDCANCEL;
use foxide2k_ui::template::DialogTemplate;

use crate::library_watch::{self, LibraryWatch};
use crate::link_list::{self, Entry, Ids, LinkList, is_pick};
use crate::status_line::StatusLine;
use crate::window::SingleWindow;

pub const LABEL: u16 = 301;
pub const LIST: u16 = 302;
pub const STATUS: u16 = 303;

/// The File > Spotify submenu.
pub struct SpotifyMenu;

impl MainMenuPopup for SpotifyMenu {
	const GUID: Guid = Guid::from_u128(0x2d703516_99e0_4545_aeb0_534a4930ff20);
	const PARENT: Guid = GROUP_FILE;
	const SORT_PRIORITY: u32 = SORT_PRIORITY_BASE;
	const NAME: &'static str = "Spotify";
}

/// The command that opens the window.
pub struct OpenPlaylist;

impl MainMenuCommand for OpenPlaylist {
	const GUID: Guid = Guid::from_u128(0x459200a7_f04a_465d_9c07_d6ada1d2eb5d);
	const PARENT: Guid = SpotifyMenu::GUID;
	const NAME: &'static str = "Open playlist…";
	const DESCRIPTION: &'static str =
		"Opens your Liked Songs or one of your Spotify playlists as a playlist tab.";

	/// Shows the window, or refreshes it and brings it up when it is open. When it can't open,
	/// says why in the console and a popup, and still succeeds: foobar2000 gets no error.
	fn execute() -> Result<()> {
		WINDOW
			.show_fresh("can't open the Open playlist window", "Can't open your Spotify playlists");
		Ok(())
	}
}

pub fn window_template() -> DialogTemplate {
	let ids = Ids { label: LABEL, list: LIST, status: STATUS };
	link_list::template("Open a Spotify playlist", "&Playlists:", ids, (260, 200), 140)
}

/// The "Open playlist…" window.
pub static WINDOW: SingleWindow<OpenPlaylistWindow> = SingleWindow::new(window_template);

/// Liked Songs, then every playlist of `library`.
fn entries_of(library: &Library) -> Vec<Entry> {
	let liked_songs = Entry {
		text: LIKED_SONGS.to_owned(),
		name: LIKED_SONGS.to_owned(),
		link: Link::LikedSongs { account: None },
	};
	std::iter::once(liked_songs)
		.chain(library.playlists.iter().map(|playlist| Entry {
			text: playlist.display_name(),
			name: playlist.name.clone(),
			link: playlist.link(),
		}))
		.collect()
}

#[derive(Default)]
pub struct OpenPlaylistWindow {
	entries: LinkList,
	status: Option<StatusLine>,
	library: LibraryWatch,
}

impl OpenPlaylistWindow {
	/// Shows `state` in the list, keeping the entry picked selected, and in the status line; a
	/// new status is announced.
	fn render(&mut self, dialog: Dialog, state: &LibraryState) {
		let entries = match state {
			LibraryState::Ready(library) => entries_of(library),
			LibraryState::Loading | LibraryState::Failed(_) | LibraryState::LoggedOut => Vec::new(),
		};
		self.entries.fill(dialog, LIST, entries, true);
		if let Some(line) = &mut self.status {
			line.show(dialog, library_watch::status(state));
		}
	}
}

impl DialogHandler for OpenPlaylistWindow {
	fn init(&mut self, dialog: &Dialog) -> Option<u16> {
		self.status = Some(StatusLine::new(*dialog, STATUS, String::new()));
		let state = self.library.start(*dialog);
		self.render(*dialog, &state);
		Some(LIST)
	}

	fn command(&mut self, dialog: &Dialog, id: u16, notification: u16) {
		if is_pick(LIST, id, notification) {
			self.entries.pick(*dialog, LIST, "Can't open this Spotify playlist");
		} else if id == IDCANCEL {
			destroy(*dialog);
		}
	}

	fn refresh(&mut self, dialog: &Dialog) {
		let state = self.library.refreshed();
		self.render(*dialog, &state);
	}
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::library::LibraryState;
	use foxide2k::abi::mainmenu::{COMMANDS_IID, GROUP_FILE, GROUP_IID, SORT_PRIORITY_BASE};
	use foxide2k::abi::playlist;
	use foxide2k::guid::Guid;
	use foxide2k::mainmenu::{
		MainMenuCommand, MainMenuPopup, mainmenu_command_service, mainmenu_popup_service,
	};
	use foxide2k::testing::{FakeApi, MainMenuHandle};
	use foxide2k_ui::host::{Dialog, destroy};
	use foxide2k_ui::notify::{BN_CLICKED, IDCANCEL, IDOK, LBN_DBLCLK};
	use foxide2k_ui::template::ControlClass;
	use foxide2k_ui::testing::{hidden_window, pump_messages, send_close, send_command};

	use super::{LABEL, LIST, OpenPlaylist, STATUS, SpotifyMenu, WINDOW, window_template};
	use crate::login::controller;
	use crate::tabs::TAB_PROPERTY;
	use crate::testing::{
		GETTY_TRACK, confirm_login_when_started, getty_favourites, installed_api,
		installed_backend, installed_prompt, library_with, refresh, sunday_set,
	};
	use crate::window::brought_up;
	use foxide2k::popup::PopupIcon;

	fn ready() -> LibraryState {
		LibraryState::Ready(library_with(vec![sunday_set(), getty_favourites()]))
	}

	#[test]
	fn the_file_menu_has_a_spotify_popup_with_open_playlist() {
		let popup = MainMenuHandle::new(mainmenu_popup_service::<SpotifyMenu>());
		assert!(popup.answers(&GROUP_IID));
		assert_eq!(popup.guid(), SpotifyMenu::GUID);
		assert_eq!(popup.parent(), GROUP_FILE);
		assert_eq!(popup.sort_priority(), SORT_PRIORITY_BASE);
		assert_eq!(popup.name(), "Spotify");
		let command = MainMenuHandle::new(mainmenu_command_service::<OpenPlaylist>());
		assert!(command.answers(&COMMANDS_IID));
		assert_eq!(command.command(0), OpenPlaylist::GUID);
		assert_eq!(command.parent(), SpotifyMenu::GUID);
		assert_eq!(command.command_name(0), "Open playlist…");
		assert_eq!(
			command.description(0).as_deref(),
			Some("Opens your Liked Songs or one of your Spotify playlists as a playlist tab.")
		);
		assert_eq!(SpotifyMenu::GUID, Guid::from_u128(0x2d703516_99e0_4545_aeb0_534a4930ff20));
		assert_eq!(OpenPlaylist::GUID, Guid::from_u128(0x459200a7_f04a_465d_9c07_d6ada1d2eb5d));
	}

	#[test]
	fn the_command_shows_the_window_and_brings_it_up_when_open() {
		installed_api();
		installed_backend().set_library(ready());
		let command = MainMenuHandle::new(mainmenu_command_service::<OpenPlaylist>());
		command.execute(0, None).unwrap();
		let dialog = WINDOW.current().expect("the command opens the window");
		assert_eq!(dialog.list_count(LIST), 3);
		assert_eq!(brought_up(), 1);
		command.execute(0, None).unwrap();
		assert_eq!(WINDOW.current(), Some(dialog));
		assert_eq!(brought_up(), 2);
		destroy(dialog);
	}

	#[test]
	fn a_window_that_cant_open_is_reported_and_the_command_succeeds() {
		let api = installed_api();
		let gone = hidden_window();
		destroy(Dialog::from_hwnd(gone));
		api.set_main_window(gone);
		let command = MainMenuHandle::new(mainmenu_command_service::<OpenPlaylist>());
		command.execute(0, None).expect("the command reports the failure itself");
		assert!(WINDOW.current().is_none());
		let popups = api.popups();
		assert_eq!(popups.len(), 1, "{popups:?}");
		let (title, message, icon) = &popups[0];
		assert_eq!(title, "Can't open your Spotify playlists");
		assert!(
			message.starts_with("Open a Spotify playlist\n\ncan't create a dialog: "),
			"{message}"
		);
		assert_eq!(*icon, PopupIcon::Error);
		let console = api.console_lines();
		assert!(
			console.iter().any(|line| line.starts_with(
				"foo_librespot: can't open the Open playlist window: can't create a dialog: "
			)),
			"{console:?}"
		);
	}

	#[test]
	fn the_template_puts_the_label_right_before_the_list() {
		let template = window_template();
		let ids: Vec<u16> = template.items.iter().map(|item| item.id).collect();
		assert_eq!(ids, [LABEL, LIST, STATUS, IDOK, IDCANCEL]);
		assert_eq!(template.items[1].class, ControlClass::ListBox);
	}

	#[test]
	fn the_window_lists_liked_songs_then_every_playlist_with_its_folders() {
		installed_api();
		let fake = installed_backend();
		fake.set_library(ready());
		let dialog = WINDOW.open(hidden_window()).unwrap();
		assert_eq!(
			dialog.list_texts(LIST),
			["Liked Songs", "Church / Sunday set", "Getty favourites"]
		);
		assert_eq!(dialog.tab_order(), [LIST, IDOK, IDCANCEL]);
		assert_eq!(dialog.focused(), Some(LIST));
		assert_eq!(dialog.list_selection(LIST), Some(0));
		assert_eq!(dialog.text(STATUS), "");
		destroy(dialog);
	}

	#[test]
	fn the_window_says_loading_until_the_list_arrives() {
		installed_api();
		let fake = installed_backend();
		fake.set_library(LibraryState::Loading);
		let dialog = WINDOW.open(hidden_window()).unwrap();
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert_eq!(dialog.list_count(LIST), 0);
		fake.publish_library(ready());
		pump_messages();
		assert_eq!(dialog.list_count(LIST), 3);
		assert_eq!(dialog.text(STATUS), "");
		destroy(dialog);
	}

	#[test]
	fn a_failed_fetch_shows_why_and_is_not_retried_while_open() {
		installed_api();
		let fake = installed_backend();
		let failed = || LibraryState::Failed("Spotify is unreachable".to_owned());
		fake.set_library(failed());
		let dialog = WINDOW.open(hidden_window()).unwrap();
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert_eq!(dialog.list_count(LIST), 0);
		fake.publish_library(failed());
		pump_messages();
		assert_eq!(dialog.text(STATUS), "Couldn't load your playlists: Spotify is unreachable.");
		refresh(dialog);
		assert_eq!(fake.library_calls(), 1);
		destroy(dialog);
	}

	#[test]
	fn logged_out_the_window_asks_for_a_login_and_loads_after_it() {
		installed_api();
		let fake = installed_backend();
		let prompt = installed_prompt();
		let dialog = WINDOW.open(hidden_window()).unwrap();
		assert_eq!(dialog.text(STATUS), "Log in to Spotify to see your playlists.");
		assert_eq!(prompt.shown(), 1);
		confirm_login_when_started(&fake, 1).join().unwrap();
		let calls = fake.library_calls();
		fake.set_library(LibraryState::Loading);
		pump_messages();
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert!(fake.library_calls() > calls);
		fake.publish_library(ready());
		pump_messages();
		assert_eq!(dialog.list_count(LIST), 3);
		assert_eq!(prompt.shown(), 1);
		destroy(dialog);
	}

	#[test]
	fn logged_out_found_while_loading_asks_for_a_login_once() {
		installed_api();
		let fake = installed_backend();
		let prompt = installed_prompt();
		fake.set_library(LibraryState::Loading);
		let dialog = WINDOW.open(hidden_window()).unwrap();
		assert_eq!(prompt.shown(), 0);
		fake.publish_library(LibraryState::LoggedOut);
		pump_messages();
		assert_eq!(dialog.text(STATUS), "Log in to Spotify to see your playlists.");
		assert_eq!(prompt.shown(), 1);
		assert_eq!(fake.logins_started(), 1);
		refresh(dialog);
		assert_eq!(prompt.shown(), 1);
		destroy(dialog);
	}

	#[test]
	fn a_logout_while_the_window_loads_asks_for_no_login() {
		installed_api();
		let fake = installed_backend();
		let prompt = installed_prompt();
		fake.set_username(Some("Keith Getty"));
		fake.set_library(LibraryState::Loading);
		let dialog = WINDOW.open(hidden_window()).unwrap();
		controller().refresh();
		controller().press();
		assert_eq!(fake.logouts(), 1);
		fake.publish_library(LibraryState::LoggedOut);
		pump_messages();
		assert_eq!(dialog.text(STATUS), "Log in to Spotify to see your playlists.");
		assert_eq!(prompt.shown(), 0);
		assert_eq!(fake.logins_started(), 0);
		destroy(dialog);
	}

	#[test]
	fn a_logout_empties_the_list_without_asking_for_a_login() {
		installed_api();
		let fake = installed_backend();
		let prompt = installed_prompt();
		fake.set_library(ready());
		let dialog = WINDOW.open(hidden_window()).unwrap();
		assert_eq!(dialog.list_count(LIST), 3);
		fake.set_library(LibraryState::LoggedOut);
		refresh(dialog);
		assert_eq!(dialog.list_count(LIST), 0);
		assert_eq!(dialog.text(STATUS), "Log in to Spotify to see your playlists.");
		assert_eq!(fake.logins_started(), 0);
		assert_eq!(prompt.shown(), 0);
		destroy(dialog);
	}

	#[test]
	fn a_refresh_with_the_same_playlists_keeps_the_selection() {
		installed_api();
		let fake = installed_backend();
		fake.set_library(ready());
		let dialog = WINDOW.open(hidden_window()).unwrap();
		dialog.list_select(LIST, 2);
		fake.publish_library(ready());
		refresh(dialog);
		assert_eq!(dialog.list_selection(LIST), Some(2));
		destroy(dialog);
	}

	#[test]
	fn a_refresh_with_the_same_playlists_leaves_the_list_alone() {
		installed_api();
		let fake = installed_backend();
		fake.set_library(ready());
		let dialog = WINDOW.open(hidden_window()).unwrap();
		// A refill would clear this row.
		dialog.list_add(LIST, "Marker");
		fake.publish_library(ready());
		refresh(dialog);
		assert_eq!(dialog.list_count(LIST), 4);
		assert_eq!(dialog.list_text(LIST, 3), "Marker");
		destroy(dialog);
	}

	#[test]
	fn a_refresh_that_moves_the_picked_playlist_selects_it_again() {
		installed_api();
		let fake = installed_backend();
		fake.set_library(ready());
		let dialog = WINDOW.open(hidden_window()).unwrap();
		dialog.list_select(LIST, 2);
		fake.set_library(LibraryState::Ready(library_with(vec![getty_favourites(), sunday_set()])));
		refresh(dialog);
		assert_eq!(dialog.list_text(LIST, 1), "Getty favourites");
		assert_eq!(dialog.list_selection(LIST), Some(1));
		destroy(dialog);
	}

	/// Opens the window with the library ready, picks row `row` and presses Open.
	fn pick_row(row: usize) {
		let dialog = WINDOW.open(hidden_window()).unwrap();
		dialog.list_select(LIST, row);
		send_command(&dialog, IDOK, BN_CLICKED);
		assert!(!dialog.is_alive());
		assert!(WINDOW.current().is_none());
	}

	fn ready_api() -> &'static FakeApi {
		let api = installed_api();
		installed_backend().set_library(ready());
		api
	}

	#[test]
	fn a_pick_that_cant_be_opened_shows_a_popup_saying_why() {
		let api = ready_api();
		api.remove_services(playlist::MANAGER_IID);
		pick_row(1);
		assert_eq!(
			api.popups(),
			[(
				"Can't open this Spotify playlist".to_owned(),
				"Sunday set\n\nfoobar2000 has no playlist_manager.".to_owned(),
				PopupIcon::Error
			)]
		);
		assert!(api.console_lines().contains(
			&"foo_librespot: can't open Sunday set: foobar2000 has no playlist_manager".to_owned()
		));
		assert!(api.location_requests().is_empty());
	}

	#[test]
	fn opening_liked_songs_makes_a_liked_songs_tab() {
		let api = ready_api();
		pick_row(0);
		assert_eq!(api.location_requests(), [["sptf://spotify:collection:tracks"]]);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		let tabs = api.tabs();
		assert_eq!(tabs.last().unwrap().name, "Liked Songs");
		assert_eq!(
			tabs.last().unwrap().properties,
			[(TAB_PROPERTY, b"spotify:collection:tracks".to_vec())]
		);
	}

	#[test]
	fn double_clicking_a_playlist_opens_it() {
		let api = ready_api();
		let dialog = WINDOW.open(hidden_window()).unwrap();
		dialog.list_select(LIST, 2);
		send_command(&dialog, LIST, LBN_DBLCLK);
		assert!(!dialog.is_alive());
		let getty_uri = getty_favourites().link().uri();
		assert_eq!(api.location_requests(), [[format!("sptf://{getty_uri}")]]);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		assert_eq!(api.tabs().last().unwrap().name, "Getty favourites");
	}

	#[test]
	fn closing_the_window_destroys_it() {
		installed_api();
		let fake = installed_backend();
		fake.set_library(ready());
		let dialog = WINDOW.open(hidden_window()).unwrap();
		send_command(&dialog, IDCANCEL, BN_CLICKED);
		assert!(!dialog.is_alive());
		assert!(WINDOW.current().is_none());
		let again = WINDOW.open(hidden_window()).unwrap();
		send_close(&again);
		pump_messages();
		assert!(!again.is_alive());
	}

	#[test]
	fn the_window_listens_to_the_library_while_it_is_open() {
		installed_api();
		let fake = installed_backend();
		fake.set_library(LibraryState::Loading);
		let dialog = WINDOW.open(hidden_window()).unwrap();
		assert_eq!(fake.library_subscribers(), 1);
		refresh(dialog);
		assert_eq!(fake.library_calls(), 2);
		assert_eq!(fake.library_subscribers(), 1);
		destroy(dialog);
		assert_eq!(fake.library_subscribers(), 0);
	}

	#[test]
	fn showing_twice_keeps_one_window() {
		installed_api();
		let fake = installed_backend();
		fake.set_library(ready());
		let owner = hidden_window();
		let first = WINDOW.open(owner).unwrap();
		let second = WINDOW.open(owner).unwrap();
		assert_eq!(first, second);
		assert_eq!(WINDOW.current(), Some(first));
		destroy(first);
		assert!(WINDOW.current().is_none());
	}
}
