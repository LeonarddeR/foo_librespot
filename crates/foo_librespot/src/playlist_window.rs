//! The window "Add to playlist…" opens: a checkable list of Liked Songs and the playlists the
//! account can change. A row starts ticked when its playlist holds every item the window works
//! on. The user ticks and unticks rows. OK saves the rows the user changed and closes the
//! window; Cancel closes it.
//!
//! [`open`] hands the window its items through a main-thread [`PENDING`], and the window takes
//! them the next time it lists the library, which drops the ticks the user gave before. The
//! window follows Open playlist…'s states while the library loads, is logged out or failed, and
//! lists it again after every change of it, keeping the ticks the user gave.

use std::cell::RefCell;
use std::collections::HashSet;

use foo_librespot_core::library::{Edit, LIKED_SONGS, Library, LibraryState, Target};
use foo_librespot_core::link::ItemId;
use foxide2k::dialog::host::{Dialog, DialogHandler, destroy};
use foxide2k::dialog::notify::{IDCANCEL, IDOK};
use foxide2k::dialog::style::CHECK_LIST;
use foxide2k::dialog::template::{ControlClass, DialogTemplate};
use foxide2k::popup::PopupIcon;

use crate::errors::show_message;
use crate::library_watch::{self, LibraryWatch};
use crate::link_list::{self, Controls, Ids};
use crate::playlist_save::{CANT_CHANGE, Planned, save};
use crate::spotify_menu::{Source, SpotifyItem};
use crate::status_line::StatusLine;
use crate::window::SingleWindow;

pub const LABEL: u16 = 501;
pub const LIST: u16 = 502;
pub const STATUS: u16 = 503;

const TITLE: &str = "Add to a Spotify playlist";

pub fn window_template() -> DialogTemplate {
	let ids = Ids { label: LABEL, list: LIST, status: STATUS };
	let controls = Controls {
		list_class: ControlClass::ListView,
		list_style: CHECK_LIST,
		ok: "OK",
		cancel: "Cancel",
	};
	link_list::template_with(TITLE, "&Playlists:", ids, controls, (260, 200), 140)
}

/// The "Add to playlist…" window.
pub static WINDOW: SingleWindow<PlaylistWindow> = SingleWindow::new(window_template);

thread_local! {
	/// The items [`show_for`] gave the window that it hasn't taken yet, each once, in order.
	/// Only foobar2000's main thread sets and reads it.
	static PENDING: RefCell<Option<Vec<ItemId>>> = const { RefCell::new(None) };
}

/// Opens the window for the Spotify `items`, taken from `source`, as [`show_for`] does. When
/// there are none, a popup titled `command` says what `source` should hold instead. Main thread
/// only.
pub fn open(items: impl IntoIterator<Item = SpotifyItem>, source: Source, command: &str) {
	let items: Vec<ItemId> = items.into_iter().map(|(item, _)| item).collect();
	if items.is_empty() {
		show_message(command, source.wants_any(), PopupIcon::Information);
	} else {
		show_for(items);
	}
}

/// Makes the window work on `items`, each once, in their order, and drops the ticks the user
/// gave before; opens the window if it is closed, and brings it up. Says in the console and a
/// popup when it can't open. Main thread only.
fn show_for(items: Vec<ItemId>) {
	let mut seen = HashSet::new();
	PENDING.set(Some(items.into_iter().filter(|item| seen.insert(*item)).collect()));
	WINDOW.show_fresh("can't open the Add to playlist window", CANT_CHANGE);
}

/// One row of the list: where its changes go, the name it shows, and whether it starts ticked.
#[derive(Debug, PartialEq, Eq)]
struct Row {
	target: Target,
	name: String,
	start: bool,
}

/// Liked Songs, when `items` hold a track, then every playlist of `library` the account can
/// change. A row starts ticked when its playlist holds every one of `items`.
fn rows_of(library: &Library, items: &[ItemId]) -> Vec<Row> {
	let row =
		|target, name| Row { target, name, start: library.holds(target, items) == Some(true) };
	let has_track = items.iter().any(|item| matches!(item, ItemId::Track(_)));
	let liked = has_track.then(|| row(Target::LikedSongs, LIKED_SONGS.into()));
	let playlists = library
		.editable()
		.map(|playlist| row(Target::Playlist(playlist.id), playlist.display_name()));
	liked.into_iter().chain(playlists).collect()
}

#[derive(Default)]
pub struct PlaylistWindow {
	/// The items the window works on, each once, in order.
	items: Vec<ItemId>,
	/// The rows the list shows, in order.
	rows: Vec<Row>,
	/// The list was filled since the window opened, the account last logged out, or new items
	/// last came with no library to list them from. Until it is, the window says "Loading…" and
	/// waits for a complete library.
	filled: bool,
	status: Option<StatusLine>,
	library: LibraryWatch,
}

impl PlaylistWindow {
	/// The changes the user made: each row whose tick differs from its start, in row order. A
	/// ticked row adds the items; an unticked one removes them.
	fn planned(&self, dialog: Dialog) -> Vec<Planned> {
		self.rows
			.iter()
			.enumerate()
			.filter_map(|(index, row)| {
				let ticked = dialog.is_row_checked(LIST, index);
				(ticked != row.start).then(|| Planned {
					edit: if ticked { Edit::Add } else { Edit::Remove },
					target: row.target,
					name: row.name.clone(),
				})
			})
			.collect()
	}

	/// Shows `state` in the list and the status line; a new status is announced. Once filled,
	/// the list stays while the library loads or fails. New items without a library to list
	/// them from empty it. A filled list without rows says why.
	fn render(&mut self, dialog: Dialog, state: &LibraryState) {
		let new_items = if let Some(items) = PENDING.take() {
			self.items = items;
			true
		} else {
			false
		};
		if new_items && !matches!(state, LibraryState::Ready(_)) {
			self.empty(dialog);
		}
		let status = match state {
			LibraryState::Ready(library) if self.filled || library.is_complete() => {
				self.fill(dialog, library, new_items);
				// Only episodes, which Liked Songs can't hold, and no playlist to change.
				let none = "You have no Spotify playlists you can change.";
				if self.rows.is_empty() { none.to_owned() } else { String::new() }
			}
			LibraryState::Loading if self.filled => return,
			// Until the list is filled, a library that isn't complete counts as loading.
			LibraryState::Ready(_) => library_watch::status(&LibraryState::Loading),
			LibraryState::LoggedOut => {
				self.empty(dialog);
				library_watch::status(state)
			}
			LibraryState::Loading | LibraryState::Failed(_) => library_watch::status(state),
		};
		if let Some(line) = &mut self.status {
			line.show(dialog, status);
		}
	}

	/// Lists the rows `library` gives the items. A row the user changed keeps the user's tick,
	/// unless `new_items` drops the user's ticks; every other row shows its start. When the rows
	/// name the same playlists in the same order, only the ticks that must change do, and the
	/// selection and scroll position stay. Otherwise the list is filled again: the selected
	/// playlist stays selected while it is listed, otherwise the first row is.
	fn fill(&mut self, dialog: Dialog, library: &Library, new_items: bool) {
		let rows = rows_of(library, &self.items);
		self.filled = true;
		let changed = if new_items { Vec::new() } else { self.planned(dialog) };
		let ticks = rows.iter().map(|row| {
			let change = changed.iter().find(|change| change.target == row.target);
			change.map_or(row.start, |change| change.edit == Edit::Add)
		});
		let same_rows = rows.len() == self.rows.len()
			&& rows
				.iter()
				.zip(&self.rows)
				.all(|(new, old)| (new.target, &new.name) == (old.target, &old.name));
		if same_rows {
			for (index, ticked) in ticks.enumerate() {
				if dialog.is_row_checked(LIST, index) != ticked {
					dialog.set_row_checked(LIST, index, ticked);
				}
			}
		} else {
			let selected_target = dialog
				.check_list_selection(LIST)
				.and_then(|index| self.rows.get(index))
				.map(|row| row.target);
			dialog.check_list_clear(LIST);
			for (row, ticked) in rows.iter().zip(ticks) {
				dialog.check_list_add(LIST, &row.name, ticked);
			}
			if !rows.is_empty() {
				let selected_index =
					rows.iter().position(|row| Some(row.target) == selected_target);
				dialog.check_list_select(LIST, selected_index.unwrap_or(0));
			}
		}
		self.rows = rows;
	}

	/// Empties the list, which drops the user's ticks; the list then counts as not filled yet.
	fn empty(&mut self, dialog: Dialog) {
		dialog.check_list_clear(LIST);
		self.rows.clear();
		self.filled = false;
	}
}

impl DialogHandler for PlaylistWindow {
	fn init(&mut self, dialog: &Dialog) -> Option<u16> {
		dialog.check_list_init(LIST);
		self.status = Some(StatusLine::new(*dialog, STATUS, String::new()));
		let state = self.library.start(*dialog);
		self.render(*dialog, &state);
		Some(LIST)
	}

	fn command(&mut self, dialog: &Dialog, id: u16, _notification: u16) {
		match id {
			IDOK => {
				save(&self.items, self.planned(*dialog));
				destroy(*dialog);
			}
			IDCANCEL => destroy(*dialog),
			_ => {}
		}
	}

	fn refresh(&mut self, dialog: &Dialog) {
		let state = self.library.refreshed();
		self.render(*dialog, &state);
	}
}

#[cfg(test)]
mod tests {
	use std::collections::HashSet;
	use std::sync::Arc;

	use foo_librespot_core::SpotifyId;
	use foo_librespot_core::library::{Edit, Library, LibraryPlaylist, LibraryState, Target};
	use foo_librespot_core::link::ItemId;
	use foxide2k::dialog::host::{Dialog, destroy, refresh_poster};
	use foxide2k::dialog::notify::{BN_CLICKED, IDCANCEL, IDOK};
	use foxide2k::dialog::style::{BS_DEFPUSHBUTTON, CHECK_LIST};
	use foxide2k::dialog::template::ControlClass;
	use foxide2k::testing::{hidden_window, pump_messages, send_close, send_command};

	use super::{LABEL, LIST, PlaylistWindow, STATUS, WINDOW, show_for, window_template};
	use crate::playlist_save::Planned;
	use crate::testing::{
		FakeBackend, contents_of, episode_id, getty_favourites, getty_track, installed_api,
		installed_backend, installed_prompt, library_holding, library_with, shared_playlist,
		sunday_set, sunday_set_id,
	};
	use crate::window::brought_up;

	fn t1() -> ItemId {
		getty_track()
	}

	fn t2() -> ItemId {
		ItemId::Track(SpotifyId::from_raw(&[0x77; 16]).unwrap())
	}

	fn episode() -> ItemId {
		ItemId::Episode(episode_id())
	}

	/// "Advent", a playlist the account can change, made after the others.
	fn advent() -> LibraryPlaylist {
		LibraryPlaylist {
			id: SpotifyId::from_raw(&[0x41; 16]).unwrap(),
			name: "Advent".to_owned(),
			folders: Vec::new(),
			can_edit: true,
			revision: b"r1".to_vec(),
		}
	}

	/// A fake Spotify whose library is `state`, and the window it shows for `items`.
	fn shown_for(state: LibraryState, items: Vec<ItemId>) -> (FakeBackend, Dialog) {
		installed_api();
		let fake = installed_backend();
		fake.set_library(state);
		show_for(items);
		(fake, WINDOW.current().expect("showing opens the window"))
	}

	/// Refreshes the window, as a change of the library does.
	fn refresh(dialog: Dialog) {
		refresh_poster(&dialog).post();
		pump_messages();
	}

	fn ticks(dialog: Dialog) -> Vec<bool> {
		(0..dialog.check_list_count(LIST)).map(|row| dialog.is_row_checked(LIST, row)).collect()
	}

	/// What OK would send now.
	fn planned() -> Vec<Planned> {
		WINDOW.inspect(PlaylistWindow::planned).expect("the window is open")
	}

	fn sunday(edit: Edit) -> Planned {
		let name = "Church / Sunday set".to_owned();
		Planned { edit, target: Target::Playlist(sunday_set_id()), name }
	}

	fn shared(edit: Edit) -> Planned {
		Planned { edit, target: Target::Playlist(shared_playlist().id), name: "Shared".to_owned() }
	}

	const ALL_ROWS: [&str; 3] = ["Liked Songs", "Church / Sunday set", "Shared"];

	#[test]
	fn the_template_makes_label_list_status_ok_and_cancel_in_order() {
		let template = window_template();
		assert_eq!(template.title, "Add to a Spotify playlist");
		let made: Vec<(ControlClass, u16)> =
			template.items.iter().map(|item| (item.class, item.id)).collect();
		assert_eq!(
			made,
			[
				(ControlClass::Static, LABEL),
				(ControlClass::ListView, LIST),
				(ControlClass::Static, STATUS),
				(ControlClass::Button, IDOK),
				(ControlClass::Button, IDCANCEL),
			]
		);
		assert_eq!(template.items[0].text, "&Playlists:");
		assert_eq!(template.items[1].style, CHECK_LIST);
		assert_eq!(template.items[3].text, "OK");
		assert_ne!(template.items[3].style & BS_DEFPUSHBUTTON, 0);
		assert_eq!(template.items[4].text, "Cancel");
	}

	#[test]
	fn the_window_lists_liked_songs_then_each_playlist_it_can_change() {
		let (_, dialog) = shown_for(library_holding(&[], &[], &[]), vec![t1()]);
		assert_eq!(dialog.check_list_texts(LIST), ALL_ROWS);
		assert!(dialog.is_check_list(LIST), "each row has a checkbox");
		assert_eq!(dialog.tab_order(), [LIST, IDOK, IDCANCEL]);
		assert_eq!(dialog.focused(), Some(LIST));
		assert_eq!(dialog.text(STATUS), "");
		assert_eq!(brought_up(), 1);
		destroy(dialog);
	}

	#[test]
	fn the_first_row_starts_selected() {
		let (_, dialog) = shown_for(library_holding(&[], &[], &[]), vec![t1()]);
		assert_eq!(dialog.check_list_selection(LIST), Some(0));
		destroy(dialog);
	}

	#[test]
	fn a_row_starts_ticked_only_when_its_playlist_holds_every_item() {
		let library = library_holding(&[t1(), t2()], &[t1()], &[t2()]);
		let (_, dialog) = shown_for(library, vec![t1(), t2()]);
		assert_eq!(dialog.check_list_texts(LIST), ALL_ROWS);
		assert_eq!(ticks(dialog), [false, true, false]);
		assert_eq!(planned(), []);
		destroy(dialog);
	}

	#[test]
	fn liked_songs_is_left_out_when_only_episodes_are_selected() {
		let (_, dialog) = shown_for(library_holding(&[], &[], &[]), vec![episode()]);
		assert_eq!(dialog.check_list_texts(LIST), ["Church / Sunday set", "Shared"]);
		show_for(vec![t1(), episode()]);
		pump_messages();
		assert_eq!(dialog.check_list_texts(LIST), ALL_ROWS);
		destroy(dialog);
	}

	#[test]
	fn with_only_episodes_and_no_playlist_to_change_the_window_says_so() {
		let none_to_change = LibraryState::Ready(library_with(vec![getty_favourites()]));
		let (fake, dialog) = shown_for(none_to_change, vec![episode()]);
		assert_eq!(dialog.check_list_count(LIST), 0);
		assert_eq!(dialog.text(STATUS), "You have no Spotify playlists you can change.");
		show_for(vec![t1()]);
		pump_messages();
		assert_eq!(dialog.check_list_texts(LIST), ["Liked Songs"]);
		assert_eq!(dialog.text(STATUS), "");
		show_for(vec![episode()]);
		pump_messages();
		assert_eq!(dialog.text(STATUS), "You have no Spotify playlists you can change.");
		fake.set_library(library_holding(&[], &[], &[]));
		refresh(dialog);
		assert_eq!(dialog.check_list_texts(LIST), ["Church / Sunday set", "Shared"]);
		assert_eq!(dialog.text(STATUS), "");
		destroy(dialog);
	}

	#[test]
	fn showing_keeps_each_item_once_in_order() {
		let (_, dialog) = shown_for(library_holding(&[], &[], &[]), vec![t2(), t1(), t2()]);
		assert_eq!(WINDOW.inspect(|window, _| window.items.clone()), Some(vec![t2(), t1()]));
		destroy(dialog);
	}

	#[test]
	fn the_window_says_loading_until_the_library_is_complete() {
		let (fake, dialog) = shown_for(LibraryState::Loading, vec![t1()]);
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert_eq!(dialog.check_list_count(LIST), 0);
		let playlists = vec![sunday_set(), getty_favourites(), shared_playlist()];
		let incomplete = Library::from_parts(playlists, Vec::new(), None);
		fake.set_library(LibraryState::Ready(Arc::new(incomplete)));
		refresh(dialog);
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert_eq!(dialog.check_list_count(LIST), 0);
		fake.set_library(library_holding(&[], &[], &[]));
		refresh(dialog);
		assert_eq!(dialog.check_list_texts(LIST), ALL_ROWS);
		assert_eq!(dialog.check_list_selection(LIST), Some(0));
		assert_eq!(dialog.text(STATUS), "");
		destroy(dialog);
	}

	#[test]
	fn a_refresh_keeps_the_users_ticks_and_adds_new_playlists_unticked() {
		let (fake, dialog) = shown_for(library_holding(&[t1()], &[], &[]), vec![t1()]);
		assert_eq!(ticks(dialog), [false, true, false]);
		dialog.set_row_checked(LIST, 1, false);
		let playlists = vec![sunday_set(), getty_favourites(), shared_playlist(), advent()];
		let contents =
			vec![(sunday_set_id(), contents_of(&[t1()])), (shared_playlist().id, contents_of(&[]))];
		let incomplete = Library::from_parts(playlists, contents, Some(HashSet::new()));
		assert!(!incomplete.is_complete());
		fake.set_library(LibraryState::Ready(Arc::new(incomplete)));
		refresh(dialog);
		assert_eq!(
			dialog.check_list_texts(LIST),
			["Liked Songs", "Church / Sunday set", "Shared", "Advent"]
		);
		assert_eq!(ticks(dialog), [false, false, false, false]);
		assert_eq!(planned(), [sunday(Edit::Remove)]);
		destroy(dialog);
	}

	#[test]
	fn a_refresh_shows_the_new_starting_tick_of_rows_the_user_didnt_touch() {
		let (fake, dialog) = shown_for(library_holding(&[], &[], &[]), vec![t1()]);
		dialog.set_row_checked(LIST, 2, true);
		fake.set_library(library_holding(&[t1()], &[], &[]));
		refresh(dialog);
		assert_eq!(ticks(dialog), [false, true, true]);
		assert_eq!(planned(), [shared(Edit::Add)]);
		fake.set_library(library_holding(&[t1()], &[t1()], &[]));
		refresh(dialog);
		assert_eq!(ticks(dialog), [false, true, true]);
		assert_eq!(planned(), []);
		destroy(dialog);
	}

	#[test]
	fn a_change_to_a_playlist_no_longer_listed_is_dropped() {
		let (fake, dialog) = shown_for(library_holding(&[], &[], &[]), vec![t1()]);
		dialog.set_row_checked(LIST, 2, true);
		fake.set_library(LibraryState::Ready(library_with(vec![sunday_set()])));
		refresh(dialog);
		assert_eq!(dialog.check_list_texts(LIST), ["Liked Songs", "Church / Sunday set"]);
		assert_eq!(planned(), []);
		fake.set_library(library_holding(&[], &[], &[]));
		refresh(dialog);
		assert_eq!(dialog.check_list_texts(LIST), ALL_ROWS);
		assert_eq!(ticks(dialog), [false, false, false]);
		assert_eq!(planned(), []);
		destroy(dialog);
	}

	#[test]
	fn a_users_tick_that_matches_spotify_again_is_no_longer_a_change() {
		let (fake, dialog) = shown_for(library_holding(&[], &[], &[]), vec![t1()]);
		dialog.set_row_checked(LIST, 2, true);
		assert_eq!(planned(), [shared(Edit::Add)]);
		fake.set_library(library_holding(&[], &[t1()], &[]));
		refresh(dialog);
		assert_eq!(ticks(dialog), [false, false, true]);
		assert_eq!(planned(), []);
		destroy(dialog);
	}

	#[test]
	fn a_refresh_keeps_the_selected_playlist_selected() {
		let (fake, dialog) = shown_for(library_holding(&[], &[], &[]), vec![t1()]);
		dialog.check_list_select(LIST, 2);
		fake.set_library(LibraryState::Ready(library_with(vec![shared_playlist(), sunday_set()])));
		refresh(dialog);
		assert_eq!(dialog.check_list_texts(LIST), ["Liked Songs", "Shared", "Church / Sunday set"]);
		assert_eq!(dialog.check_list_selection(LIST), Some(1));
		fake.set_library(LibraryState::Ready(library_with(vec![sunday_set()])));
		refresh(dialog);
		assert_eq!(dialog.check_list_texts(LIST), ["Liked Songs", "Church / Sunday set"]);
		assert_eq!(dialog.check_list_selection(LIST), Some(0));
		destroy(dialog);
	}

	#[test]
	fn a_refresh_with_the_same_rows_leaves_the_list_alone() {
		let (fake, dialog) = shown_for(library_holding(&[], &[], &[]), vec![t1()]);
		// A rebuild would clear this row.
		dialog.check_list_add(LIST, "Marker", false);
		fake.set_library(library_holding(&[], &[], &[]));
		refresh(dialog);
		assert_eq!(dialog.check_list_count(LIST), 4);
		assert_eq!(dialog.check_list_text(LIST, 3), "Marker");
		destroy(dialog);
	}

	#[test]
	fn a_refresh_that_only_changes_ticks_keeps_the_list_and_its_selection() {
		let (fake, dialog) = shown_for(library_holding(&[], &[], &[]), vec![t1()]);
		dialog.check_list_select(LIST, 2);
		dialog.set_row_checked(LIST, 0, true);
		// A rebuild would clear this row.
		dialog.check_list_add(LIST, "Marker", false);
		fake.set_library(library_holding(&[t1()], &[], &[]));
		refresh(dialog);
		assert_eq!(
			dialog.check_list_texts(LIST),
			["Liked Songs", "Church / Sunday set", "Shared", "Marker"]
		);
		assert_eq!(ticks(dialog), [true, true, false, false]);
		assert_eq!(dialog.check_list_selection(LIST), Some(2));
		show_for(vec![t2()]);
		pump_messages();
		assert_eq!(dialog.check_list_count(LIST), 4, "new items with the same rows keep the list");
		assert_eq!(ticks(dialog), [false, false, false, false]);
		assert_eq!(dialog.check_list_selection(LIST), Some(2));
		destroy(dialog);
	}

	#[test]
	fn loading_after_the_fill_keeps_the_rows_and_ticks() {
		let (fake, dialog) = shown_for(library_holding(&[t1()], &[], &[]), vec![t1()]);
		dialog.set_row_checked(LIST, 1, false);
		fake.set_library(LibraryState::Loading);
		refresh(dialog);
		assert_eq!(dialog.check_list_texts(LIST), ALL_ROWS);
		assert_eq!(planned(), [sunday(Edit::Remove)]);
		assert_eq!(dialog.text(STATUS), "");
		destroy(dialog);
	}

	#[test]
	fn a_logout_empties_the_list_and_drops_the_ticks() {
		let prompt = installed_prompt();
		let (fake, dialog) = shown_for(library_holding(&[t1()], &[], &[]), vec![t1()]);
		dialog.set_row_checked(LIST, 2, true);
		fake.set_library(LibraryState::LoggedOut);
		refresh(dialog);
		assert_eq!(dialog.check_list_count(LIST), 0);
		assert_eq!(dialog.text(STATUS), "Log in to Spotify to see your playlists.");
		assert_eq!(planned(), []);
		assert_eq!(prompt.shown(), 0);
		fake.set_library(library_holding(&[t1()], &[], &[]));
		refresh(dialog);
		assert_eq!(ticks(dialog), [false, true, false]);
		assert_eq!(planned(), []);
		destroy(dialog);
	}

	#[test]
	fn logged_out_the_window_asks_for_a_login_once() {
		let prompt = installed_prompt();
		let (fake, dialog) = shown_for(LibraryState::LoggedOut, vec![t1()]);
		assert_eq!(dialog.text(STATUS), "Log in to Spotify to see your playlists.");
		assert_eq!(dialog.check_list_count(LIST), 0);
		assert_eq!((prompt.shown(), fake.logins_started()), (1, 1));
		refresh(dialog);
		assert_eq!(prompt.shown(), 1);
		destroy(dialog);
	}

	#[test]
	fn a_failed_load_says_why() {
		let (fake, dialog) = shown_for(LibraryState::Loading, vec![t1()]);
		let failed = || LibraryState::Failed("no network".to_owned());
		fake.publish_library(failed());
		pump_messages();
		assert_eq!(dialog.text(STATUS), "Couldn't load your playlists: no network.");
		assert_eq!(dialog.check_list_count(LIST), 0);
		fake.publish_library(library_holding(&[], &[], &[]));
		pump_messages();
		assert_eq!(dialog.text(STATUS), "");
		fake.publish_library(failed());
		pump_messages();
		assert_eq!(dialog.text(STATUS), "Couldn't load your playlists: no network.");
		assert_eq!(dialog.check_list_texts(LIST), ALL_ROWS);
		destroy(dialog);
	}

	#[test]
	fn showing_for_new_items_replaces_them_and_drops_the_ticks() {
		let (_, dialog) = shown_for(library_holding(&[t1()], &[], &[]), vec![t1()]);
		assert_eq!(ticks(dialog), [false, true, false]);
		dialog.set_row_checked(LIST, 2, true);
		show_for(vec![t2()]);
		pump_messages();
		assert_eq!(WINDOW.current(), Some(dialog));
		assert_eq!(brought_up(), 2);
		assert_eq!(ticks(dialog), [false, false, false]);
		assert_eq!(planned(), []);
		destroy(dialog);
	}

	#[test]
	fn new_items_with_no_library_to_list_them_empty_the_list() {
		let (fake, dialog) = shown_for(library_holding(&[t1()], &[], &[]), vec![t1()]);
		dialog.set_row_checked(LIST, 2, true);
		fake.set_library(LibraryState::Failed("no network".to_owned()));
		show_for(vec![t2()]);
		pump_messages();
		assert_eq!(dialog.check_list_count(LIST), 0);
		assert_eq!(planned(), []);
		assert_eq!(dialog.text(STATUS), "Couldn't load your playlists: no network.");
		fake.set_library(library_holding(&[t1()], &[], &[]));
		refresh(dialog);
		assert_eq!(ticks(dialog), [false, false, false]);
		destroy(dialog);
	}

	#[test]
	fn ok_and_cancel_close_the_window() {
		let (_, dialog) = shown_for(library_holding(&[], &[], &[]), vec![t1()]);
		send_command(&dialog, IDOK, BN_CLICKED);
		assert!(!dialog.is_alive());
		assert_eq!(WINDOW.current(), None);
		show_for(vec![t1()]);
		send_command(&WINDOW.current().unwrap(), IDCANCEL, BN_CLICKED);
		assert_eq!(WINDOW.current(), None);
		show_for(vec![t1()]);
		send_close(&WINDOW.current().unwrap());
		pump_messages();
		assert_eq!(WINDOW.current(), None);
	}

	/// The changes sent so far: what, where and for which items.
	fn sent(fake: &FakeBackend) -> Vec<(Edit, Target, Vec<ItemId>)> {
		fake.changes().into_iter().map(|(edit, target, items, _)| (edit, target, items)).collect()
	}

	#[test]
	fn ok_saves_the_changed_rows_and_closes() {
		let library = library_holding(&[t1(), t2()], &[], &[]);
		let (fake, dialog) = shown_for(library, vec![t1(), t2()]);
		dialog.set_row_checked(LIST, 2, true);
		dialog.set_row_checked(LIST, 1, false);
		send_command(&dialog, IDOK, BN_CLICKED);
		assert!(!dialog.is_alive());
		assert_eq!(WINDOW.current(), None);
		assert_eq!(
			sent(&fake),
			[
				(Edit::Remove, Target::Playlist(sunday_set_id()), vec![t1(), t2()]),
				(Edit::Add, Target::Playlist(shared_playlist().id), vec![t1(), t2()]),
			]
		);
	}

	#[test]
	fn cancel_and_escape_send_nothing() {
		let (fake, dialog) = shown_for(library_holding(&[t1()], &[], &[]), vec![t1()]);
		dialog.set_row_checked(LIST, 1, false);
		dialog.set_row_checked(LIST, 2, true);
		send_command(&dialog, IDCANCEL, BN_CLICKED);
		assert!(!dialog.is_alive());
		show_for(vec![t1()]);
		let dialog = WINDOW.current().expect("showing opens the window");
		dialog.set_row_checked(LIST, 2, true);
		send_close(&dialog);
		pump_messages();
		assert_eq!(WINDOW.current(), None);
		assert_eq!(sent(&fake), []);
	}

	#[test]
	fn ok_while_loading_sends_nothing_and_closes() {
		let (fake, dialog) = shown_for(LibraryState::Loading, vec![t1()]);
		send_command(&dialog, IDOK, BN_CLICKED);
		assert!(!dialog.is_alive());
		assert_eq!(WINDOW.current(), None);
		assert_eq!(sent(&fake), []);
	}

	#[test]
	fn the_window_listens_to_the_library_while_it_is_open() {
		let (fake, dialog) = shown_for(LibraryState::Loading, vec![t1()]);
		assert_eq!(fake.library_subscribers(), 1);
		refresh(dialog);
		assert_eq!(fake.library_calls(), 2);
		assert_eq!(fake.library_subscribers(), 1);
		destroy(dialog);
		assert_eq!(fake.library_subscribers(), 0);
	}

	#[test]
	fn a_window_that_cant_open_says_why() {
		let api = installed_api();
		installed_backend().set_library(library_holding(&[], &[], &[]));
		let gone = hidden_window();
		destroy(Dialog::from_hwnd(gone));
		api.set_main_window(gone);
		show_for(vec![t1()]);
		let popups = api.popups();
		let (title, message, _) = &popups[0];
		assert_eq!(title, "Can't change the Spotify playlist");
		assert!(
			message.starts_with("Add to a Spotify playlist\n\ncan't create a dialog: "),
			"{message}"
		);
		assert!(
			api.console_lines().iter().any(|line| line.starts_with(
				"foo_librespot: can't open the Add to playlist window: can't create a dialog: "
			)),
			"{:?}",
			api.console_lines()
		);
	}
}
