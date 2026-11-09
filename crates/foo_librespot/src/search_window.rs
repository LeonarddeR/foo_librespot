//! The Search Spotify window: the user types words into the search field and presses Enter, and
//! the window lists the tracks Spotify finds for them, as a search link
//! (`spotify:search:<words>`) finds them. The field's drop-down list holds the search history.
//!
//! Each search gets a number and runs in the background. Its answer arrives on another thread,
//! waits in the window's [`Inbox`] with that number, and a posted refresh shows it on the main
//! thread. Only the newest search's answer is shown; an older one is dropped. A search that
//! found the account logged out asks for a login and waits; once a login lands, the login's
//! notification posts a refresh, and the refresh runs that search again.

use std::cell::Cell;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use foo_librespot_core::CoreError;
use foo_librespot_core::metadata::TrackInfo;
use foo_librespot_core::resolve::Resolved;
use foxide2k::console;
use foxide2k_ui::host::{Dialog, DialogHandler, destroy, refresh_poster};
use foxide2k_ui::notify::{IDCANCEL, IDOK};
use foxide2k_ui::style::{
	BS_DEFPUSHBUTTON, CBS_AUTOHSCROLL, CBS_DROPDOWN, DS_CENTER, DS_MODALFRAME, REPORT_LIST,
	SS_NOPREFIX, WS_CAPTION, WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VSCROLL,
};
use foxide2k_ui::template::{ControlClass, DialogTemplate, shell_font};

use crate::account::item;
use crate::backend::backend;
use crate::first_use::ask_for_login;
use crate::links::{plural, print_skipped};
use crate::login::{LoginState, controller};
use crate::search_history::{self, normalize, remember};
use crate::status_line::StatusLine;
use crate::window::SingleWindow;

pub const SEARCH_LABEL: u16 = 601;
pub const SEARCH: u16 = 602;
pub const TRACKS_LABEL: u16 = 603;
pub const LIST: u16 = 604;
pub const STATUS: u16 = 605;

/// The list's columns: each one's header, and its share of the list's width in percent.
const COLUMNS: [(&str, u16); 5] =
	[("#", 6), ("Title", 34), ("Artist", 24), ("Album", 24), ("Duration", 12)];

/// The window's width and height, and the list's height, in dialog units.
const SIZE: (i16, i16) = (360, 241);
const LIST_HEIGHT: i16 = 150;

/// The window, at a fixed size. Each control is made in tab order, each label right before its
/// control. Add is the default button.
pub fn window_template() -> DialogTemplate {
	let (cx, cy) = SIZE;
	let width = cx - 14;
	let status_y = 54 + LIST_HEIGHT;
	let buttons_y = status_y + 15;
	let field = CBS_DROPDOWN | CBS_AUTOHSCROLL | WS_VSCROLL | WS_TABSTOP;
	DialogTemplate {
		style: WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME | DS_CENTER,
		title: "Search Spotify".to_owned(),
		cx,
		cy,
		font: shell_font(),
		items: vec![
			item(SEARCH_LABEL, ControlClass::Static, "&Search for:", 0, (7, 7, width, 10)),
			// A combo box's height includes its drop-down list.
			item(SEARCH, ControlClass::ComboBox, "", field, (7, 19, width, 120)),
			item(TRACKS_LABEL, ControlClass::Static, "&Tracks:", 0, (7, 38, width, 10)),
			item(LIST, ControlClass::ListView, "", REPORT_LIST, (7, 50, width, LIST_HEIGHT)),
			item(STATUS, ControlClass::Static, "", SS_NOPREFIX, (7, status_y, width, 10)),
			item(
				IDOK,
				ControlClass::Button,
				"&Add",
				BS_DEFPUSHBUTTON | WS_TABSTOP,
				(cx - 145, buttons_y, 66, 14),
			),
			item(IDCANCEL, ControlClass::Button, "Close", WS_TABSTOP, (cx - 73, buttons_y, 66, 14)),
		],
	}
}

/// The Search Spotify window.
pub static WINDOW: SingleWindow<SearchWindow> = SingleWindow::new(window_template);

thread_local! {
	/// Set by [`show`] until the open window's next refresh puts focus in the search field. Only
	/// foobar2000's main thread sets and reads it.
	static FOCUS_SEARCH: Cell<bool> = const { Cell::new(false) };
}

/// Opens the window, or brings the open one up with focus in the search field and its text
/// selected; the rows stay. When it can't open, says why in the console and a popup. Main
/// thread only.
pub fn show() {
	FOCUS_SEARCH.set(true);
	WINDOW.show_fresh("can't open the Search Spotify window", "Can't search Spotify");
}

/// What the status line says when a search found `found` tracks and left out `left_out` that
/// can't play.
pub fn status_found(found: usize, left_out: usize) -> String {
	let found = if found == 0 {
		"No tracks found".to_owned()
	} else {
		format!("Found {found} {}", plural(found, "track", "tracks"))
	};
	if left_out == 0 {
		format!("{found}.")
	} else {
		let was = plural(left_out, "was", "were");
		format!("{found}; {left_out} can't play here and {was} left out.")
	}
}

/// `ms` as m:ss, or as h:mm:ss from an hour on.
pub fn duration(ms: u32) -> String {
	let seconds = ms / 1000;
	let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
	if hours == 0 {
		format!("{minutes}:{seconds:02}")
	} else {
		format!("{hours}:{minutes:02}:{seconds:02}")
	}
}

/// The texts of `track`'s row, column by column: its `place` in the list from 1, its title, its
/// artists, its album (an episode's show), and its duration.
pub fn cells(place: usize, track: &TrackInfo) -> [String; 5] {
	[
		place.to_string(),
		track.title.clone(),
		track.artists.join(", "),
		track.album.clone(),
		duration(track.duration_ms),
	]
}

/// What reaches the window from other threads, until its next refresh takes it.
#[derive(Default)]
struct Inbox {
	/// Each search's answer, with the number of the search.
	answers: Vec<(u64, Result<Resolved, CoreError>)>,
}

fn lock(inbox: &Mutex<Inbox>) -> MutexGuard<'_, Inbox> {
	inbox.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
pub struct SearchWindow {
	status: Option<StatusLine>,
	/// The tracks the list shows, in list order.
	found: Vec<Arc<TrackInfo>>,
	/// The number of the newest search, counting from 1; 0 before the first.
	newest: u64,
	/// The words of the newest search.
	words: String,
	/// The words of a search that found the account logged out, until a login lands or a newer
	/// search starts.
	waiting: Option<String>,
	inbox: Arc<Mutex<Inbox>>,
	login_subscription: Option<u64>,
}

impl SearchWindow {
	/// Searches for the words typed in the search field, after putting them first in the
	/// history. Does nothing when only spaces were typed. A history that can't be saved is told
	/// in the console, and the search goes on.
	fn search_typed(&mut self, dialog: Dialog) {
		let Some(words) = normalize(&dialog.text(SEARCH)) else {
			return;
		};
		let history = remember(&search_history::load(), &words);
		if let Err(error) = search_history::save(&history) {
			console::print(&format!("foo_librespot: can't save the search history: {error}"));
		}
		dialog.combo_clear(SEARCH);
		for entry in &history {
			dialog.combo_add(SEARCH, entry);
		}
		// Emptying the drop-down list emptied the field too.
		dialog.set_text(SEARCH, &words);
		self.search(dialog, words);
	}

	/// Empties the list and starts the newest search, for `words`; an older search's answer will
	/// be dropped, and no words wait for a login any more. The answer comes through the inbox
	/// and a posted refresh.
	fn search(&mut self, dialog: Dialog, words: String) {
		self.waiting = None;
		self.found.clear();
		dialog.report_clear(LIST);
		self.show_status(dialog, "Searching…".to_owned());
		self.newest += 1;
		self.words.clone_from(&words);
		let (number, inbox, poster) =
			(self.newest, Arc::clone(&self.inbox), refresh_poster(&dialog));
		backend().search(
			words,
			Box::new(move |answer| {
				lock(&inbox).answers.push((number, answer));
				poster.post();
			}),
		);
	}

	/// Shows the newest search's `answer`, without moving keyboard focus.
	fn show_answer(&mut self, dialog: Dialog, answer: Result<Resolved, CoreError>) {
		match answer {
			Ok(resolved) => {
				for (index, track) in resolved.tracks.iter().enumerate() {
					let cells = cells(index + 1, track);
					dialog.report_add(LIST, &cells.each_ref().map(String::as_str));
				}
				if !resolved.tracks.is_empty() {
					dialog.report_select_only(LIST, 0);
				}
				print_skipped(&resolved.skipped);
				let status = status_found(resolved.tracks.len(), resolved.skipped.len());
				self.show_status(dialog, status);
				self.found = resolved.tracks;
			}
			Err(CoreError::NotLoggedIn) => {
				self.show_status(dialog, "Log in to Spotify to search.".to_owned());
				ask_for_login();
				self.waiting = Some(self.words.clone());
			}
			Err(CoreError::Aborted) => {}
			Err(error) => self.show_status(dialog, format!("Couldn't search Spotify: {error}.")),
		}
	}

	/// Shows `text` on the status line, announcing it when it is new.
	fn show_status(&mut self, dialog: Dialog, text: String) {
		if let Some(line) = &mut self.status {
			line.show(dialog, text);
		}
	}
}

impl DialogHandler for SearchWindow {
	fn init(&mut self, dialog: &Dialog) -> Option<u16> {
		for entry in search_history::load() {
			dialog.combo_add(SEARCH, &entry);
		}
		dialog.report_init(LIST, &COLUMNS);
		self.status = Some(StatusLine::new(*dialog, STATUS, String::new()));
		let poster = refresh_poster(dialog);
		self.login_subscription = Some(controller().subscribe(Box::new(move || poster.post())));
		// A new window starts with focus in the search field; no refresh needs to put it there.
		FOCUS_SEARCH.set(false);
		Some(SEARCH)
	}

	fn command(&mut self, dialog: &Dialog, id: u16, _notification: u16) {
		match id {
			// Enter in the search field.
			IDOK if dialog.has_focus_within(SEARCH) => self.search_typed(*dialog),
			IDCANCEL => destroy(*dialog),
			_ => {}
		}
	}

	/// Puts focus in the search field when [`show`] asked for it; nothing else moves focus. Runs
	/// the search waiting for a login once the account is logged in, then shows the newest
	/// search's answer and drops older ones. The waiting search runs first, so a search that
	/// finds the account logged out during this refresh waits for the next change of the login,
	/// and a refresh can't search again and again.
	fn refresh(&mut self, dialog: &Dialog) {
		if FOCUS_SEARCH.take() {
			dialog.focus(SEARCH);
			dialog.select_all_text(SEARCH);
		}
		if self.waiting.is_some()
			&& matches!(controller().state(), LoginState::LoggedIn(_))
			&& let Some(words) = self.waiting.take()
		{
			self.search(*dialog, words);
		}
		let answers = std::mem::take(&mut lock(&self.inbox).answers);
		let newest = self.newest;
		let answer =
			answers.into_iter().find_map(|(number, answer)| (number == newest).then_some(answer));
		if let Some(answer) = answer {
			self.show_answer(*dialog, answer);
		}
	}
}

impl Drop for SearchWindow {
	/// Runs when the window is destroyed, also when foobar2000 quits with it open. A search
	/// that answers later only fills an inbox nobody reads.
	fn drop(&mut self) {
		if let Some(subscription) = self.login_subscription {
			controller().unsubscribe(subscription);
		}
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use foo_librespot_core::resolve::{Resolved, SkipReason, Skipped};
	use foo_librespot_core::{CoreError, SpotifyId};
	use foxide2k::abi::config_store;
	use foxide2k::testing::FakeApi;
	use foxide2k_ui::host::{Dialog, destroy};
	use foxide2k_ui::notify::{BN_CLICKED, IDCANCEL, IDOK};
	use foxide2k_ui::style::{
		BS_DEFPUSHBUTTON, CBS_AUTOHSCROLL, CBS_DROPDOWN, REPORT_LIST, WS_TABSTOP, WS_VSCROLL,
	};
	use foxide2k_ui::template::ControlClass;
	use foxide2k_ui::testing::{hidden_window, pump_messages, send_command};

	use super::{
		LIST, SEARCH, SEARCH_LABEL, STATUS, TRACKS_LABEL, WINDOW, cells, duration, show,
		status_found, window_template,
	};
	use crate::login::controller;
	use crate::search_history;
	use crate::testing::{
		FakeBackend, cancel_login_when_started, confirm_login_when_started, episode_id,
		episode_info, installed_api_and_backend, installed_prompt, refresh, track_info, wait_until,
	};
	use crate::window::brought_up;

	/// What a search for `words` finds: tracks with `titles`, each with an ID of its own.
	fn found(words: &str, titles: &[&str]) -> Resolved {
		let tracks = titles
			.iter()
			.zip(1u8..)
			.map(|(title, n)| Arc::new(track_info(SpotifyId::from_raw(&[n; 16]).unwrap(), title)))
			.collect();
		Resolved { name: Some(words.to_owned()), tracks, skipped: Vec::new(), duplicates: 0 }
	}

	fn getty() -> Resolved {
		found("getty", &["In Christ Alone", "Christ Our Hope In Life And Death"])
	}

	fn bach() -> Resolved {
		found("bach", &["Jesu, Joy of Man's Desiring", "Sheep May Safely Graze", "Air"])
	}

	/// A fake foobar2000, a fake Spotify whose searches find `resolved`, and the window, open.
	fn opened(resolved: Resolved) -> (&'static FakeApi, FakeBackend, Dialog) {
		let (api, fake) = installed_api_and_backend();
		fake.resolve_to(resolved);
		(api, fake, WINDOW.open(hidden_window()).unwrap())
	}

	/// Types `text` into the search field and presses Enter there.
	fn search_for(dialog: Dialog, text: &str) {
		dialog.set_text(SEARCH, text);
		dialog.focus(SEARCH);
		send_command(&dialog, IDOK, BN_CLICKED);
	}

	/// Waits until `answers` searches have answered, and shows what they answered.
	fn take_answers(fake: &FakeBackend, answers: usize) {
		wait_until(|| fake.searches_answered() == answers);
		pump_messages();
	}

	/// The text of every row of the list, column by column.
	fn rows(dialog: Dialog) -> Vec<Vec<String>> {
		(0..dialog.report_count(LIST)).map(|row| dialog.report_row(LIST, row)).collect()
	}

	/// The rows the list shows for `resolved`.
	fn rows_of(resolved: &Resolved) -> Vec<Vec<String>> {
		let tracks = resolved.tracks.iter().enumerate();
		tracks.map(|(index, track)| cells(index + 1, track).to_vec()).collect()
	}

	/// The titles of the tracks the window keeps, in list order.
	fn kept_titles() -> Vec<String> {
		let titles = |window: &super::SearchWindow, _: Dialog| {
			window.found.iter().map(|track| track.title.clone()).collect()
		};
		WINDOW.inspect(titles).expect("the window is open")
	}

	fn strings(items: &[&str]) -> Vec<String> {
		items.iter().map(|&item| item.to_owned()).collect()
	}

	#[test]
	fn the_template_makes_its_controls_in_tab_order() {
		let template = window_template();
		assert_eq!(template.title, "Search Spotify");
		let made: Vec<(ControlClass, u16, &str)> =
			template.items.iter().map(|item| (item.class, item.id, item.text.as_str())).collect();
		assert_eq!(
			made,
			[
				(ControlClass::Static, SEARCH_LABEL, "&Search for:"),
				(ControlClass::ComboBox, SEARCH, ""),
				(ControlClass::Static, TRACKS_LABEL, "&Tracks:"),
				(ControlClass::ListView, LIST, ""),
				(ControlClass::Static, STATUS, ""),
				(ControlClass::Button, IDOK, "&Add"),
				(ControlClass::Button, IDCANCEL, "Close"),
			]
		);
		assert_eq!(
			template.items[1].style,
			CBS_DROPDOWN | CBS_AUTOHSCROLL | WS_VSCROLL | WS_TABSTOP
		);
		assert_eq!(template.items[3].style, REPORT_LIST);
		assert_ne!(template.items[5].style & BS_DEFPUSHBUTTON, 0);

		let (_, _, dialog) = opened(getty());
		assert_eq!(dialog.tab_order(), [SEARCH, LIST, IDOK, IDCANCEL]);
		assert_eq!(dialog.report_columns(LIST), ["#", "Title", "Artist", "Album", "Duration"]);
		assert!(dialog.has_focus_within(SEARCH));
		assert_eq!(dialog.text(STATUS), "");
		destroy(dialog);
	}

	#[test]
	fn the_search_field_offers_the_history() {
		let (_, fake) = installed_api_and_backend();
		fake.resolve_to(getty());
		search_history::save(&strings(&["getty", "bach"])).unwrap();
		let dialog = WINDOW.open(hidden_window()).unwrap();
		assert_eq!(dialog.combo_texts(SEARCH), ["getty", "bach"]);
		destroy(dialog);
	}

	#[test]
	fn enter_in_the_search_field_searches_and_lists_the_tracks() {
		let (_, fake, dialog) = opened(getty());
		search_for(dialog, "  in christ   alone ");
		assert_eq!(fake.searches(), ["in christ alone"]);
		assert_eq!(dialog.text(STATUS), "Searching…");
		take_answers(&fake, 1);
		assert_eq!(rows(dialog), rows_of(&getty()));
		assert_eq!(dialog.report_selected(LIST), [0]);
		assert_eq!(dialog.text(STATUS), "Found 2 tracks.");
		assert!(dialog.has_focus_within(SEARCH));
		assert_eq!(search_history::load().first().map(String::as_str), Some("in christ alone"));
		assert_eq!(dialog.combo_texts(SEARCH), ["in christ alone"]);
		assert_eq!(dialog.text(SEARCH), "in christ alone");
		assert_eq!(kept_titles(), ["In Christ Alone", "Christ Our Hope In Life And Death"]);
		destroy(dialog);
	}

	#[test]
	fn a_search_with_only_spaces_does_nothing() {
		let (_, fake) = installed_api_and_backend();
		fake.resolve_to(getty());
		search_history::save(&strings(&["getty"])).unwrap();
		let dialog = WINDOW.open(hidden_window()).unwrap();
		search_for(dialog, "  \t ");
		assert!(fake.searches().is_empty());
		assert_eq!(search_history::load(), ["getty"]);
		assert_eq!(dialog.combo_texts(SEARCH), ["getty"]);
		assert_eq!(dialog.text(STATUS), "");
		destroy(dialog);
	}

	#[test]
	fn a_history_that_cant_be_saved_is_reported_and_the_search_goes_on() {
		let (api, fake, dialog) = opened(getty());
		api.remove_services(config_store::IID);
		search_for(dialog, "getty");
		assert_eq!(fake.searches(), ["getty"]);
		let console = api.console_lines();
		assert!(
			console
				.iter()
				.any(|line| line.starts_with("foo_librespot: can't save the search history: ")),
			"{console:?}"
		);
		assert_eq!(dialog.combo_texts(SEARCH), ["getty"]);
		take_answers(&fake, 1);
		assert_eq!(dialog.text(STATUS), "Found 2 tracks.");
		destroy(dialog);
	}

	#[test]
	fn tracks_that_cant_play_are_counted_and_printed() {
		let mut resolved = found("getty", &["In Christ Alone"]);
		resolved.skipped.push(Skipped {
			uri: "spotify:track:0iYaBOYiWTPt2A7JMNK6eO".to_owned(),
			reason: SkipReason::Unavailable("not available in your country".to_owned()),
		});
		let (api, fake, dialog) = opened(resolved);
		search_for(dialog, "getty");
		take_answers(&fake, 1);
		assert_eq!(dialog.report_count(LIST), 1);
		assert_eq!(dialog.text(STATUS), "Found 1 track; 1 can't play here and was left out.");
		let console = api.console_lines();
		assert!(
			console.contains(
				&"foo_librespot: skipped spotify:track:0iYaBOYiWTPt2A7JMNK6eO: not available: not available in your country"
					.to_owned()
			),
			"{console:?}"
		);
		destroy(dialog);
	}

	#[test]
	fn an_older_search_answering_late_is_dropped() {
		let (_, fake, dialog) = opened(getty());
		fake.hold_searches();
		search_for(dialog, "bach");
		search_for(dialog, "getty");
		fake.answer_search(1, Ok(getty()));
		pump_messages();
		fake.answer_search(0, Ok(bach()));
		pump_messages();
		assert_eq!(rows(dialog), rows_of(&getty()));
		assert_eq!(dialog.text(STATUS), "Found 2 tracks.");
		destroy(dialog);
	}

	#[test]
	fn answers_arriving_together_show_only_the_newest() {
		let (_, fake, dialog) = opened(getty());
		fake.hold_searches();
		search_for(dialog, "bach");
		search_for(dialog, "getty");
		fake.answer_search(1, Ok(getty()));
		fake.answer_search(0, Ok(bach()));
		pump_messages();
		assert_eq!(rows(dialog), rows_of(&getty()));
		assert_eq!(dialog.text(STATUS), "Found 2 tracks.");
		destroy(dialog);
	}

	#[test]
	fn a_failed_search_says_why() {
		let (_, fake, dialog) = opened(getty());
		fake.fail_with(|| CoreError::Unavailable("no network".into()));
		search_for(dialog, "getty");
		take_answers(&fake, 1);
		let error = CoreError::Unavailable("no network".into());
		assert_eq!(dialog.text(STATUS), format!("Couldn't search Spotify: {error}."));
		assert_eq!(dialog.report_count(LIST), 0);
		destroy(dialog);
	}

	#[test]
	fn an_aborted_search_changes_nothing() {
		let (api, fake, dialog) = opened(getty());
		let prompt = installed_prompt();
		fake.hold_searches();
		search_for(dialog, "getty");
		fake.answer_search(0, Err(CoreError::Aborted));
		pump_messages();
		assert_eq!(dialog.text(STATUS), "Searching…");
		assert_eq!(dialog.report_count(LIST), 0);
		assert_eq!(prompt.shown(), 0);
		assert!(api.console_lines().is_empty(), "{:?}", api.console_lines());
		destroy(dialog);
	}

	#[test]
	fn a_search_waiting_for_a_login_runs_once_the_login_lands() {
		let (_, fake, dialog) = opened(getty());
		let prompt = installed_prompt();
		fake.require_login();
		search_for(dialog, "getty");
		take_answers(&fake, 1);
		assert_eq!(dialog.text(STATUS), "Log in to Spotify to search.");
		assert_eq!(prompt.shown(), 1);
		assert_eq!(fake.logins_started(), 1);

		cancel_login_when_started(&fake).join().unwrap();
		pump_messages();
		assert_eq!(dialog.text(STATUS), "Log in to Spotify to search.");
		assert_eq!(fake.searches(), ["getty"]);

		// The user logs in another way, as with Log in in the preferences.
		controller().press();
		confirm_login_when_started(&fake, 2).join().unwrap();
		pump_messages();
		assert_eq!(fake.searches(), ["getty", "getty"]);
		take_answers(&fake, 2);
		assert_eq!(rows(dialog), rows_of(&getty()));
		assert_eq!(dialog.text(STATUS), "Found 2 tracks.");
		assert_eq!(search_history::load(), ["getty"]);
		destroy(dialog);
	}

	#[test]
	fn a_newer_search_drops_the_one_waiting_for_a_login() {
		let (_, fake, dialog) = opened(getty());
		installed_prompt();
		fake.require_login();
		search_for(dialog, "bach");
		take_answers(&fake, 1);
		assert_eq!(dialog.text(STATUS), "Log in to Spotify to search.");
		fake.hold_searches();
		search_for(dialog, "getty");
		confirm_login_when_started(&fake, 1).join().unwrap();
		pump_messages();
		assert_eq!(fake.searches(), ["bach", "getty"]);
		fake.answer_search(0, Ok(getty()));
		pump_messages();
		assert_eq!(rows(dialog), rows_of(&getty()));
		destroy(dialog);
	}

	#[test]
	fn status_found_and_duration_give_the_exact_texts() {
		assert_eq!(status_found(20, 0), "Found 20 tracks.");
		assert_eq!(status_found(1, 0), "Found 1 track.");
		assert_eq!(status_found(18, 2), "Found 18 tracks; 2 can't play here and were left out.");
		assert_eq!(status_found(19, 1), "Found 19 tracks; 1 can't play here and was left out.");
		assert_eq!(status_found(0, 0), "No tracks found.");
		assert_eq!(status_found(0, 3), "No tracks found; 3 can't play here and were left out.");
		assert_eq!(duration(279_000), "4:39");
		assert_eq!(duration(59_000), "0:59");
		assert_eq!(duration(3_600_000), "1:00:00");
	}

	#[test]
	fn a_track_shows_its_place_title_artists_album_and_duration() {
		let track = track_info(SpotifyId::from_raw(&[1; 16]).unwrap(), "In Christ Alone");
		assert_eq!(
			cells(4, &track),
			["4", "In Christ Alone", "Keith Getty, Kristyn Getty", "In Christ Alone", "4:39"]
		);
	}

	#[test]
	fn an_episode_shows_its_show_as_album() {
		let episode = episode_info(episode_id(), "Singing the Psalms");
		assert_eq!(cells(1, &episode)[3], "Sing! Podcast");
	}

	#[test]
	fn closing_the_window_drops_a_late_answer_and_the_login_subscription() {
		let (_, fake) = installed_api_and_backend();
		fake.resolve_to(getty());
		let subscribers = controller().subscribers();
		let dialog = WINDOW.open(hidden_window()).unwrap();
		assert_eq!(controller().subscribers(), subscribers + 1);
		fake.hold_searches();
		search_for(dialog, "getty");
		send_command(&dialog, IDCANCEL, BN_CLICKED);
		assert!(!dialog.is_alive());
		fake.answer_search(0, Ok(getty()));
		pump_messages();
		assert!(WINDOW.current().is_none());
		assert_eq!(controller().subscribers(), subscribers);
	}

	#[test]
	fn an_answer_never_moves_focus() {
		let (_, fake, dialog) = opened(getty());
		fake.hold_searches();
		search_for(dialog, "getty");
		dialog.focus(LIST);
		fake.answer_search(0, Ok(getty()));
		pump_messages();
		assert_eq!(dialog.report_count(LIST), 2);
		assert_eq!(dialog.focused(), Some(LIST));
		destroy(dialog);
	}

	#[test]
	fn showing_again_puts_focus_in_the_search_field_and_keeps_the_rows() {
		let (api, fake) = installed_api_and_backend();
		api.set_main_window(hidden_window());
		fake.resolve_to(getty());
		show();
		let dialog = WINDOW.current().expect("showing opens the window");
		assert!(dialog.has_focus_within(SEARCH));
		dialog.focus(LIST);
		// A refresh, such as a change of the login posts, leaves focus alone.
		refresh(dialog);
		assert_eq!(dialog.focused(), Some(LIST));
		search_for(dialog, "getty");
		take_answers(&fake, 1);
		dialog.focus(LIST);
		show();
		pump_messages();
		assert_eq!(WINDOW.current(), Some(dialog));
		assert!(dialog.has_focus_within(SEARCH));
		assert_eq!(rows(dialog), rows_of(&getty()));
		assert_eq!(brought_up(), 2);
		destroy(dialog);
	}
}
