//! File > Spotify > "Open playlist…": a window that lists Liked Songs and the playlists of the
//! Spotify library, and opens the one picked as a playlist tab named after it.
//!
//! The tab is filled as pasting the playlist's link would fill it, and appears once the tracks
//! have arrived. It carries its Spotify URI as a tab property, so choosing the same playlist
//! again brings that tab to the front; choosing it while its tracks load starts nothing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use foo_librespot_core::library::{LIKED_SONGS, Library, LibraryState};
use foo_librespot_core::link::Link;
use foxide2k::abi::mainmenu::{GROUP_FILE, SORT_PRIORITY_BASE};
use foxide2k::console;
use foxide2k::dialog::accessibility::{announce, make_live_region};
use foxide2k::dialog::host::{Dialog, DialogHandler, destroy, refresh_poster};
use foxide2k::dialog::notify::{BN_CLICKED, IDCANCEL, IDOK, LBN_DBLCLK};
use foxide2k::dialog::style::{
	BS_DEFPUSHBUTTON, DS_CENTER, DS_MODALFRAME, LBS_NOTIFY, SS_NOPREFIX, WS_BORDER, WS_CAPTION,
	WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VSCROLL,
};
use foxide2k::dialog::template::{ControlClass, DialogTemplate, shell_font};
use foxide2k::error::Result;
use foxide2k::guid::Guid;
use foxide2k::host::host;
use foxide2k::mainmenu::{MainMenuCommand, MainMenuPopup};
use foxide2k::metadb::HandleList;
use foxide2k::playlist;
use foxide2k::popup::PopupIcon;

use crate::account::item;
use crate::backend::backend;
use crate::errors::show_failure;
use crate::first_use::ask_for_login;
use crate::links::canonical;
use crate::login::controller;
use crate::window::SingleWindow;

/// The tab property that keeps the Spotify URI of the playlist a tab was opened from.
pub const TAB_PROPERTY: Guid = Guid::from_u128(0x0c10e611_31ed_4e6e_b689_6a93937de1bc);

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

	fn execute() -> Result<()> {
		WINDOW.show(host().main_window())
	}
}

pub fn window_template() -> DialogTemplate {
	DialogTemplate {
		style: WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME | DS_CENTER,
		title: "Open a Spotify playlist".to_owned(),
		cx: 260,
		cy: 200,
		font: shell_font(),
		items: vec![
			item(LABEL, ControlClass::Static, "&Playlists:", 0, (7, 7, 246, 10)),
			item(
				LIST,
				ControlClass::ListBox,
				"",
				WS_TABSTOP | WS_VSCROLL | WS_BORDER | LBS_NOTIFY,
				(7, 19, 246, 140),
			),
			item(STATUS, ControlClass::Static, "", SS_NOPREFIX, (7, 163, 246, 10)),
			item(
				IDOK,
				ControlClass::Button,
				"&Open",
				BS_DEFPUSHBUTTON | WS_TABSTOP,
				(115, 178, 66, 14),
			),
			item(IDCANCEL, ControlClass::Button, "Close", WS_TABSTOP, (187, 178, 66, 14)),
		],
	}
}

/// The "Open playlist…" window.
pub static WINDOW: SingleWindow<OpenPlaylistWindow> = SingleWindow::new(window_template);

/// One line of the list: what the tab is called and where its tracks come from.
#[derive(PartialEq, Eq)]
struct Entry {
	text: String,
	name: String,
	link: Link,
}

impl Entry {
	fn liked_songs() -> Self {
		Self {
			text: LIKED_SONGS.to_owned(),
			name: LIKED_SONGS.to_owned(),
			link: Link::LikedSongs { account: None },
		}
	}
}

/// Liked Songs, then every playlist of `library`.
fn entries_of(library: &Library) -> Vec<Entry> {
	std::iter::once(Entry::liked_songs())
		.chain(library.playlists.iter().map(|playlist| Entry {
			text: playlist.display_name(),
			name: playlist.name.clone(),
			link: playlist.link(),
		}))
		.collect()
}

#[derive(Default)]
pub struct OpenPlaylistWindow {
	entries: Vec<Entry>,
	status: String,
	login_subscription: Option<u64>,
	library_subscription: Option<u64>,
	/// Set while the window hasn't learnt whether the account is logged in and the login hasn't
	/// changed since it opened: a logged-out answer then asks for a login, as opening logged out
	/// does.
	ask_when_logged_out: Arc<AtomicBool>,
}

impl OpenPlaylistWindow {
	/// Shows `state` in the list and the status line; a new status is announced.
	fn render(&mut self, dialog: Dialog, state: LibraryState) {
		let (entries, status) = match state {
			LibraryState::Ready(library) => (entries_of(&library), String::new()),
			LibraryState::Loading => (Vec::new(), "Loading…".to_owned()),
			LibraryState::Failed(reason) => {
				(Vec::new(), format!("Couldn't load your playlists: {reason}."))
			}
			LibraryState::LoggedOut => {
				(Vec::new(), "Log in to Spotify to see your playlists.".to_owned())
			}
		};
		self.fill(dialog, entries);
		dialog.set_text(STATUS, &status);
		if status != self.status {
			announce(&dialog, STATUS);
		}
		self.status = status;
	}

	/// Shows `entries` in the list. An unchanged list is left alone; a changed one is refilled
	/// with the entry of the same link selected, or the first.
	fn fill(&mut self, dialog: Dialog, entries: Vec<Entry>) {
		if entries == self.entries {
			return;
		}
		let picked = dialog
			.list_selection(LIST)
			.and_then(|row| self.entries.get(row))
			.map(|entry| entry.link.clone());
		dialog.list_clear(LIST);
		for entry in &entries {
			dialog.list_add(LIST, &entry.text);
		}
		let row = entries.iter().position(|entry| Some(&entry.link) == picked.as_ref());
		self.entries = entries;
		if !self.entries.is_empty() {
			dialog.list_select(LIST, row.unwrap_or(0));
		}
	}

	/// Opens the selected entry and closes the window; does nothing when none is selected.
	fn pick(&self, dialog: Dialog) {
		let Some(entry) = dialog.list_selection(LIST).and_then(|row| self.entries.get(row)) else {
			return;
		};
		if let Err(error) = open_entry(entry) {
			cant_open(&entry.name, &error.message);
		}
		destroy(dialog);
	}
}

/// Says in the console and a popup that the playlist `name` couldn't be opened, and why.
fn cant_open(name: &str, why: &str) {
	console::print(&format!("foo_librespot: can't open {name}: {why}"));
	show_failure("Can't open this Spotify playlist", PopupIcon::Error, name, &why);
}

impl DialogHandler for OpenPlaylistWindow {
	fn init(&mut self, dialog: &Dialog) -> Option<u16> {
		if let Err(error) = make_live_region(dialog, STATUS) {
			console::print(&format!("foo_librespot: {}", error.message));
		}
		let (poster, ask) = (refresh_poster(dialog), Arc::clone(&self.ask_when_logged_out));
		self.login_subscription = Some(controller().subscribe(Box::new(move || {
			ask.store(false, Ordering::SeqCst);
			poster.post();
		})));
		let poster = refresh_poster(dialog);
		self.library_subscription =
			Some(backend().subscribe_library(Box::new(move || poster.post())));
		let state = match backend().library() {
			LibraryState::LoggedOut => {
				ask_for_login();
				LibraryState::LoggedOut
			}
			LibraryState::Loading | LibraryState::Failed(_) => {
				self.ask_when_logged_out.store(true, Ordering::SeqCst);
				LibraryState::Loading
			}
			state @ LibraryState::Ready(_) => state,
		};
		self.render(*dialog, state);
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
		let state = match backend().held_library() {
			LibraryState::Loading => backend().library(),
			state => state,
		};
		match state {
			LibraryState::Loading => {}
			LibraryState::LoggedOut if self.ask_when_logged_out.swap(false, Ordering::SeqCst) => {
				ask_for_login();
			}
			_ => self.ask_when_logged_out.store(false, Ordering::SeqCst),
		}
		self.render(*dialog, state);
	}
}

impl Drop for OpenPlaylistWindow {
	/// Runs when the window is destroyed, also when foobar2000 quits with it open.
	fn drop(&mut self) {
		if let Some(subscription) = self.login_subscription {
			controller().unsubscribe(subscription);
		}
		if let Some(subscription) = self.library_subscription {
			backend().unsubscribe_library(subscription);
		}
	}
}

/// The URIs of the playlists whose tracks are on their way to a tab.
static LOADING: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn loading() -> MutexGuard<'static, Vec<String>> {
	LOADING.lock().unwrap_or_else(PoisonError::into_inner)
}

fn done_loading(uri: &str) {
	loading().retain(|loading| loading != uri);
}

/// Brings the tab of `entry` to the front. Without one, loads its link as pasting it would,
/// unless that load runs already; the tab appears once the tracks have arrived.
fn open_entry(entry: &Entry) -> Result<()> {
	let uri = entry.link.uri();
	if let Some(index) = playlist::find_by_property(&TAB_PROPERTY, uri.as_bytes())? {
		return playlist::activate(index);
	}
	{
		let mut loading = loading();
		if loading.contains(&uri) {
			return Ok(());
		}
		loading.push(uri.clone());
	}
	let (name, done_uri, aborted_uri) = (entry.name.clone(), uri.clone(), uri.clone());
	let loaded = playlist::load_locations(
		&[canonical(&entry.link)],
		Box::new(move |items| {
			done_loading(&done_uri);
			fill(&name, &done_uri, items)
		}),
		Box::new(move || done_loading(&aborted_uri)),
	);
	if loaded.is_err() {
		done_loading(&uri);
	}
	loaded
}

/// Appends `items` to the tab holding `uri`. Without one, makes a tab named `name` that holds
/// it, with `items`, and brings it to the front.
fn fill(name: &str, uri: &str, items: &HandleList<'_>) -> Result<()> {
	if let Some(index) = playlist::find_by_property(&TAB_PROPERTY, uri.as_bytes())? {
		return playlist::append(index, items);
	}
	let index = playlist::create(name)?;
	playlist::set_property(index, &TAB_PROPERTY, uri.as_bytes())?;
	playlist::append(index, items)?;
	playlist::activate(index)
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::library::LibraryState;
	use foxide2k::abi::mainmenu::{COMMANDS_IID, GROUP_FILE, GROUP_IID, SORT_PRIORITY_BASE};
	use foxide2k::dialog::host::{destroy, refresh_poster};
	use foxide2k::dialog::notify::{BN_CLICKED, IDCANCEL, IDOK, LBN_DBLCLK};
	use foxide2k::dialog::template::ControlClass;
	use foxide2k::guid::Guid;
	use foxide2k::mainmenu::{
		MainMenuCommand, MainMenuPopup, mainmenu_command_service, mainmenu_popup_service,
	};
	use foxide2k::testing::{
		FakeApi, MainMenuHandle, hidden_window, pump_messages, send_close, send_command,
	};

	use super::{
		LABEL, LIST, OpenPlaylist, STATUS, SpotifyMenu, TAB_PROPERTY, WINDOW, cant_open,
		window_template,
	};
	use crate::login::controller;
	use crate::testing::{
		confirm_login_when_started, getty_favourites, installed_backend, installed_prompt,
		library_with, sunday_set, sunday_set_id,
	};
	use foxide2k::popup::PopupIcon;

	const GETTY_TRACK: &str = "sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO";

	fn ready() -> LibraryState {
		LibraryState::Ready(library_with(vec![sunday_set(), getty_favourites()]))
	}

	fn sunday_uri() -> String {
		format!("spotify:playlist:{}", sunday_set_id().to_base62())
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
		assert_eq!(TAB_PROPERTY, Guid::from_u128(0x0c10e611_31ed_4e6e_b689_6a93937de1bc));
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
		FakeApi::new().install();
		let fake = installed_backend();
		fake.set_library(ready());
		let dialog = WINDOW.open(hidden_window()).unwrap();
		let texts: Vec<String> =
			(0..dialog.list_count(LIST)).map(|row| dialog.list_text(LIST, row)).collect();
		assert_eq!(texts, ["Liked Songs", "Church / Sunday set", "Getty favourites"]);
		assert_eq!(dialog.tab_order(), [LIST, IDOK, IDCANCEL]);
		assert_eq!(dialog.focused(), Some(LIST));
		assert_eq!(dialog.list_selection(LIST), Some(0));
		assert_eq!(dialog.text(STATUS), "");
		destroy(dialog);
	}

	#[test]
	fn the_window_says_loading_until_the_list_arrives() {
		FakeApi::new().install();
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
		FakeApi::new().install();
		let fake = installed_backend();
		let failed = || LibraryState::Failed("Spotify is unreachable".to_owned());
		fake.set_library(failed());
		let dialog = WINDOW.open(hidden_window()).unwrap();
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert_eq!(dialog.list_count(LIST), 0);
		fake.publish_library(failed());
		pump_messages();
		assert_eq!(dialog.text(STATUS), "Couldn't load your playlists: Spotify is unreachable.");
		refresh_poster(&dialog).post();
		pump_messages();
		assert_eq!(fake.library_calls(), 1);
		destroy(dialog);
	}

	#[test]
	fn a_playlist_that_cant_be_opened_shows_a_popup_saying_why() {
		let api = FakeApi::new();
		api.install();
		cant_open("Sunday set", "foobar2000 did not create the playlist");
		assert_eq!(
			api.popups(),
			[(
				"Can't open this Spotify playlist".to_owned(),
				"Sunday set\n\nfoobar2000 did not create the playlist.".to_owned(),
				PopupIcon::Error
			)]
		);
		assert!(
			api.console_lines().contains(
				&"foo_librespot: can't open Sunday set: foobar2000 did not create the playlist"
					.to_owned()
			)
		);
	}

	#[test]
	fn logged_out_the_window_asks_for_a_login_and_loads_after_it() {
		FakeApi::new().install();
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
		FakeApi::new().install();
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
		refresh_poster(&dialog).post();
		pump_messages();
		assert_eq!(prompt.shown(), 1);
		destroy(dialog);
	}

	#[test]
	fn a_logout_while_the_window_loads_asks_for_no_login() {
		FakeApi::new().install();
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
		FakeApi::new().install();
		let fake = installed_backend();
		let prompt = installed_prompt();
		fake.set_library(ready());
		let dialog = WINDOW.open(hidden_window()).unwrap();
		assert_eq!(dialog.list_count(LIST), 3);
		fake.set_library(LibraryState::LoggedOut);
		refresh_poster(&dialog).post();
		pump_messages();
		assert_eq!(dialog.list_count(LIST), 0);
		assert_eq!(dialog.text(STATUS), "Log in to Spotify to see your playlists.");
		assert_eq!(fake.logins_started(), 0);
		assert_eq!(prompt.shown(), 0);
		destroy(dialog);
	}

	#[test]
	fn a_refresh_with_the_same_playlists_keeps_the_selection() {
		FakeApi::new().install();
		let fake = installed_backend();
		fake.set_library(ready());
		let dialog = WINDOW.open(hidden_window()).unwrap();
		dialog.list_select(LIST, 2);
		fake.publish_library(ready());
		refresh_poster(&dialog).post();
		pump_messages();
		assert_eq!(dialog.list_selection(LIST), Some(2));
		destroy(dialog);
	}

	#[test]
	fn a_refresh_that_moves_the_picked_playlist_selects_it_again() {
		FakeApi::new().install();
		let fake = installed_backend();
		fake.set_library(ready());
		let dialog = WINDOW.open(hidden_window()).unwrap();
		dialog.list_select(LIST, 2);
		fake.set_library(LibraryState::Ready(library_with(vec![getty_favourites(), sunday_set()])));
		refresh_poster(&dialog).post();
		pump_messages();
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
		let api = FakeApi::new();
		api.install();
		installed_backend().set_library(ready());
		api
	}

	#[test]
	fn opening_a_playlist_loads_its_link_and_makes_no_tab_yet() {
		let api = ready_api();
		pick_row(1);
		assert_eq!(api.location_requests(), [[format!("sptf://{}", sunday_uri())]]);
		assert!(api.tabs().is_empty());
	}

	#[test]
	fn the_tracks_arrive_in_a_new_active_tab_named_and_tagged_after_the_playlist() {
		let api = ready_api();
		api.add_tab("Default");
		pick_row(1);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		let tabs = api.tabs();
		assert_eq!(tabs.len(), 2);
		assert_eq!(tabs[1].name, "Sunday set");
		assert_eq!(tabs[1].properties, [(TAB_PROPERTY, sunday_uri().into_bytes())]);
		assert_eq!(tabs[1].paths, [GETTY_TRACK]);
		assert_eq!(api.active_tab(), Some(1));
	}

	#[test]
	fn opening_liked_songs_makes_a_liked_songs_tab() {
		let api = ready_api();
		pick_row(0);
		assert_eq!(api.location_requests(), [["sptf://spotify:collection:tracks"]]);
		api.finish_locations(0, &[]).unwrap();
		let tabs = api.tabs();
		assert_eq!(tabs.last().unwrap().name, "Liked Songs");
		assert_eq!(
			tabs.last().unwrap().properties,
			[(TAB_PROPERTY, b"spotify:collection:tracks".to_vec())]
		);
	}

	#[test]
	fn a_cancelled_load_leaves_no_tab_and_can_be_tried_again() {
		let api = ready_api();
		pick_row(1);
		api.cancel_locations(0);
		assert!(api.tabs().is_empty());
		pick_row(1);
		assert_eq!(api.location_requests().len(), 2);
	}

	#[test]
	fn a_second_pick_during_the_load_starts_no_second_import() {
		let api = ready_api();
		pick_row(1);
		pick_row(1);
		assert_eq!(api.location_requests().len(), 1);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		assert_eq!(api.tabs().len(), 1);
	}

	#[test]
	fn opening_a_playlist_that_has_a_tab_switches_to_it() {
		let api = ready_api();
		pick_row(1);
		api.finish_locations(0, &[]).unwrap();
		let opened = api.tabs().len() - 1;
		api.add_tab("Other");
		foxide2k::playlist::activate(opened + 1).unwrap();
		let tabs_before = api.tabs().len();
		pick_row(1);
		assert_eq!(api.tabs().len(), tabs_before);
		assert_eq!(api.location_requests().len(), 1);
		assert_eq!(api.active_tab(), Some(opened));
	}

	/// Adds a tab that holds the Sunday set's URI, as one made while its tracks load would.
	fn add_sunday_tab(api: &FakeApi) -> usize {
		let index = api.add_tab("Sunday set");
		foxide2k::playlist::set_property(index, &TAB_PROPERTY, sunday_uri().as_bytes()).unwrap();
		index
	}

	#[test]
	fn a_tab_made_during_the_load_gets_the_tracks() {
		let api = ready_api();
		pick_row(1);
		let index = add_sunday_tab(api);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		assert_eq!(api.tabs().len(), 1);
		assert_eq!(api.tabs()[index].paths, [GETTY_TRACK]);
	}

	#[test]
	fn a_tab_closed_during_the_load_is_made_anew() {
		let api = ready_api();
		pick_row(1);
		let index = add_sunday_tab(api);
		api.remove_tab(index);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		let tabs = api.tabs();
		assert_eq!(tabs.len(), 1);
		assert_eq!(tabs[0].name, "Sunday set");
		assert_eq!(tabs[0].paths, [GETTY_TRACK]);
	}

	#[test]
	fn double_clicking_a_playlist_opens_it() {
		let api = ready_api();
		let dialog = WINDOW.open(hidden_window()).unwrap();
		dialog.list_select(LIST, 2);
		send_command(&dialog, LIST, LBN_DBLCLK);
		assert!(!dialog.is_alive());
		api.finish_locations(0, &[]).unwrap();
		assert_eq!(api.tabs().last().unwrap().name, "Getty favourites");
	}

	#[test]
	fn closing_the_window_destroys_it() {
		FakeApi::new().install();
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
		FakeApi::new().install();
		let fake = installed_backend();
		fake.set_library(LibraryState::Loading);
		let dialog = WINDOW.open(hidden_window()).unwrap();
		assert_eq!(fake.library_subscribers(), 1);
		refresh_poster(&dialog).post();
		pump_messages();
		assert_eq!(fake.library_calls(), 2);
		assert_eq!(fake.library_subscribers(), 1);
		destroy(dialog);
		assert_eq!(fake.library_subscribers(), 0);
	}

	#[test]
	fn showing_twice_keeps_one_window() {
		FakeApi::new().install();
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
