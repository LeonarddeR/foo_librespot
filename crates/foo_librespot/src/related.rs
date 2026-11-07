//! Opening what a Spotify track belongs to, its album, an artist, its radio or its show, in a
//! playlist tab of its own.
//!
//! The lookup runs in the background; the tab opens on the main thread once it ends. When a
//! track's artists are asked for and it has several, the artist window lists them instead. A
//! lookup that the artist window says "Loading…" for settles that window whatever it finds
//! ([`artist_window::settle`]). Every failure is told in the console and a popup; none reaches
//! foobar2000.

use std::fmt::Display;

use foo_librespot_core::CoreError;
use foo_librespot_core::link::ItemId;
use foo_librespot_core::related::{Found, Opened, Related};
use foxide2k::console;
use foxide2k::main_thread;
use foxide2k::popup::PopupIcon;

use crate::artist_window::{self, Wait};
use crate::backend::backend;
use crate::errors::show_failure;
use crate::first_use::ask_for_login;
use crate::tabs::open_in_tab;

/// Starts finding `what` for `item` and returns at once; what was found then opens in its tab.
/// `title` names the track in a failure: its stored title, or its URI. `wait` is the artist
/// window's "Loading…" for this lookup, from [`artist_window::show_loading`], if it shows one.
/// Main thread only.
pub fn open_related(item: ItemId, what: Related, title: String, wait: Option<Wait>) {
	let noun = noun(&what);
	backend().related(item, what, Box::new(move |result| end(result, noun, &title, wait)));
}

/// What a failure calls `what`.
const fn noun(what: &Related) -> &'static str {
	match what {
		Related::Album => "album",
		Related::Artist(_) => "artist",
		Related::Radio => "radio",
		Related::Show => "show",
	}
}

/// On the main thread, settles the artist window for the lookup that carried `wait`, and opens
/// what the lookup found; several artists are the artist window's to list. Otherwise asks for a
/// login, or says why it failed, on this thread.
fn end(result: Result<Found, CoreError>, noun: &'static str, title: &str, wait: Option<Wait>) {
	let found = match result {
		Ok(found) => found,
		Err(error) => {
			if wait.is_some() {
				close_loading_window(wait);
			}
			return match error {
				CoreError::NotLoggedIn => ask_for_login(),
				CoreError::Aborted => {}
				error => fail(noun, title, &error),
			};
		}
	};
	let queued_title = title.to_owned();
	let queued = main_thread::run(Box::new(move || match found {
		Found::One(opened) => {
			artist_window::settle(wait, None);
			open(&opened, noun, &queued_title);
		}
		Found::Artists(artists) => artist_window::settle(wait, Some(artists)),
	}));
	if let Err(error) = queued {
		fail(noun, title, &error.message);
	}
}

/// On the main thread, closes the artist window if it still says "Loading…" for the lookup
/// that carried `wait`. Says in the console when it can't.
fn close_loading_window(wait: Option<Wait>) {
	if let Err(error) = main_thread::run(Box::new(move || artist_window::settle(wait, None))) {
		console::print(&format!("foo_librespot: can't close the artist window: {}", error.message));
	}
}

/// Opens `opened` in its tab, or says why it can't. Main thread only.
fn open(opened: &Opened, noun: &str, title: &str) {
	if let Err(error) = open_in_tab(&opened.name, &opened.link) {
		fail(noun, title, &error.message);
	}
}

/// Says in the console and a popup that the `noun` of the track `title` can't be opened, and
/// why.
fn fail(noun: &str, title: &str, why: &dyn Display) {
	console::print(&format!("foo_librespot: can't open the {noun} of {title}: {why}"));
	show_failure(&format!("Can't open this {noun}"), PopupIcon::Error, title, why);
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::link::{ItemId, Link, LinkKind};
	use foo_librespot_core::related::{Found, Opened, Related};
	use foo_librespot_core::{CoreError, SpotifyId};
	use foxide2k::abi::{main_thread, playlist};
	use foxide2k::dialog::host::{Dialog, destroy};
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::{FakeApi, pump_messages};

	use super::open_related;
	use crate::artist_window;
	use crate::testing::{installed_backend, installed_prompt, track_id, wait_until};
	use crate::window::brought_up;

	const GETTY_TRACK: &str = "sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO";
	const KEITH: &str = "0I4Bk2s2BUJyykCwtxx8Xx";

	fn installed_api() -> &'static FakeApi {
		let api = FakeApi::new();
		api.install();
		api
	}

	fn getty_track() -> ItemId {
		ItemId::Track(track_id())
	}

	/// Waits until the work the lookup queued for the main thread has run.
	fn run_main_thread_work(api: &FakeApi) {
		wait_until(|| api.run_main_thread() > 0);
	}

	/// Whether anything said the lookup failed.
	fn reported_a_failure(api: &FakeApi) -> bool {
		!api.popups().is_empty()
			|| api.console_lines().iter().any(|line| line.starts_with("foo_librespot: can't open"))
	}

	#[test]
	fn an_album_opens_in_a_tab_named_after_it() {
		let api = installed_api();
		let fake = installed_backend();
		open_related(getty_track(), Related::Album, GETTY_TRACK.to_owned(), None);
		run_main_thread_work(api);
		assert_eq!(fake.related_calls(), [(getty_track(), Related::Album)]);
		assert_eq!(api.location_requests(), [["sptf://spotify:album:4wPtS24asgBAG4VOmYYB9O"]]);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		assert_eq!(api.tabs()[0].name, "In Christ Alone");
	}

	#[test]
	fn several_artists_open_the_artist_window() {
		let api = installed_api();
		let fake = installed_backend();
		open_related(getty_track(), Related::Artist(None), "In Christ Alone".to_owned(), None);
		run_main_thread_work(api);
		assert_eq!(fake.related_calls(), [(getty_track(), Related::Artist(None))]);
		let dialog = artist_window::WINDOW.current().expect("the artist window is open");
		assert_eq!(listed(dialog), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(brought_up(), 1);
		assert!(api.location_requests().is_empty());
		assert!(!reported_a_failure(api), "{:?} {:?}", api.popups(), api.console_lines());
		destroy(dialog);
	}

	/// What a lookup finds when Spotify names Keith Getty alone.
	fn keith_alone() -> Found {
		let id = SpotifyId::from_base62(KEITH).unwrap();
		Found::One(Opened {
			name: "Keith Getty".to_owned(),
			link: Link::Id { kind: LinkKind::Artist, id },
		})
	}

	/// The names the artist window lists.
	fn listed(dialog: Dialog) -> Vec<String> {
		(0..dialog.list_count(artist_window::LIST))
			.map(|row| dialog.list_text(artist_window::LIST, row))
			.collect()
	}

	/// Shows the artist window saying "Loading…" and starts looking up every artist of the
	/// Getty track, as the shortcut-only Open artist… does.
	fn look_up_artists_while_loading() -> Dialog {
		let wait = artist_window::show_loading();
		let dialog = artist_window::WINDOW.current().unwrap();
		open_related(
			getty_track(),
			Related::Artist(None),
			"In Christ Alone".to_owned(),
			Some(wait),
		);
		dialog
	}

	#[test]
	fn artists_found_fill_the_loading_window() {
		let api = installed_api();
		installed_backend();
		let dialog = look_up_artists_while_loading();
		run_main_thread_work(api);
		pump_messages();
		assert_eq!(artist_window::WINDOW.current(), Some(dialog));
		assert_eq!(listed(dialog), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(dialog.text(artist_window::STATUS), "");
		assert_eq!(brought_up(), 2);
		destroy(dialog);
	}

	#[test]
	fn one_artist_found_closes_the_loading_window_and_opens_the_tab() {
		let api = installed_api();
		let fake = installed_backend();
		fake.related_with(|_, _| Ok(keith_alone()));
		let dialog = look_up_artists_while_loading();
		run_main_thread_work(api);
		assert!(!dialog.is_alive());
		assert!(artist_window::WINDOW.current().is_none());
		assert_eq!(api.location_requests(), [[format!("sptf://spotify:artist:{KEITH}")]]);
		assert!(!reported_a_failure(api), "{:?} {:?}", api.popups(), api.console_lines());
	}

	#[test]
	fn logged_out_closes_the_loading_window_and_asks_for_a_login() {
		let api = installed_api();
		let fake = installed_backend();
		let prompt = installed_prompt();
		fake.require_login();
		let dialog = look_up_artists_while_loading();
		wait_until(|| prompt.shown() == 1);
		run_main_thread_work(api);
		assert!(!dialog.is_alive());
		assert_eq!(fake.logins_started(), 1);
		assert!(api.location_requests().is_empty());
		assert!(!reported_a_failure(api), "{:?} {:?}", api.popups(), api.console_lines());
	}

	#[test]
	fn an_aborted_lookup_closes_the_loading_window() {
		let api = installed_api();
		let fake = installed_backend();
		fake.related_with(|_, _| Err(CoreError::Aborted));
		let dialog = look_up_artists_while_loading();
		run_main_thread_work(api);
		assert!(!dialog.is_alive());
		assert!(api.location_requests().is_empty());
		assert!(!reported_a_failure(api), "{:?} {:?}", api.popups(), api.console_lines());
	}

	#[test]
	fn a_failure_closes_the_loading_window_and_says_why() {
		let api = installed_api();
		let fake = installed_backend();
		fake.related_with(|_, _| {
			Err(CoreError::Unavailable("Spotify names no artist for this track".to_owned()))
		});
		let dialog = look_up_artists_while_loading();
		wait_until(|| !api.popups().is_empty());
		run_main_thread_work(api);
		assert!(!dialog.is_alive());
		assert_eq!(
			api.popups(),
			[(
				"Can't open this artist".to_owned(),
				"In Christ Alone\n\nSpotify names no artist for this track.".to_owned(),
				PopupIcon::Error
			)]
		);
		assert!(api.console_lines().contains(
			&"foo_librespot: can't open the artist of In Christ Alone: Spotify names no artist for this track".to_owned()
		));
	}

	#[test]
	fn a_window_listing_artists_is_left_alone_by_other_results() {
		let api = installed_api();
		let fake = installed_backend();
		let earlier = artist_window::show_loading();
		let dialog = look_up_artists_while_loading();
		run_main_thread_work(api);
		pump_messages();
		assert_eq!(listed(dialog), ["Keith Getty", "Kristyn Getty"]);
		let look_up = |wait| {
			open_related(getty_track(), Related::Artist(None), "In Christ Alone".to_owned(), wait);
		};
		fake.related_with(|_, _| Ok(keith_alone()));
		look_up(Some(earlier));
		run_main_thread_work(api);
		look_up(None);
		run_main_thread_work(api);
		assert_eq!(api.location_requests(), [[format!("sptf://spotify:artist:{KEITH}")]]);
		fake.related_with(|_, _| Err(CoreError::Unavailable("gone".to_owned())));
		look_up(Some(earlier));
		wait_until(|| !api.popups().is_empty());
		run_main_thread_work(api);
		look_up(None);
		wait_until(|| api.popups().len() == 2);
		assert_eq!(artist_window::WINDOW.current(), Some(dialog));
		assert_eq!(listed(dialog), ["Keith Getty", "Kristyn Getty"]);
		destroy(dialog);
	}

	#[test]
	fn another_kind_of_lookup_leaves_the_loading_window_alone() {
		let api = installed_api();
		installed_backend();
		artist_window::show_loading();
		let dialog = artist_window::WINDOW.current().unwrap();
		open_related(getty_track(), Related::Album, "In Christ Alone".to_owned(), None);
		run_main_thread_work(api);
		assert_eq!(api.location_requests(), [["sptf://spotify:album:4wPtS24asgBAG4VOmYYB9O"]]);
		assert_eq!(artist_window::WINDOW.current(), Some(dialog));
		assert_eq!(dialog.text(artist_window::STATUS), "Loading…");
		destroy(dialog);
	}

	#[test]
	fn a_lookup_without_a_window_leaves_another_lookups_loading_window_alone() {
		let api = installed_api();
		let fake = installed_backend();
		artist_window::show_loading();
		let dialog = artist_window::WINDOW.current().unwrap();
		fake.related_with(|_, _| Ok(keith_alone()));
		open_related(getty_track(), Related::Artist(None), "In Christ Alone".to_owned(), None);
		run_main_thread_work(api);
		assert_eq!(api.location_requests(), [[format!("sptf://spotify:artist:{KEITH}")]]);
		fake.related_with(|_, _| Err(CoreError::Unavailable("gone".to_owned())));
		open_related(getty_track(), Related::Artist(None), "In Christ Alone".to_owned(), None);
		wait_until(|| !api.popups().is_empty());
		assert_eq!(artist_window::WINDOW.current(), Some(dialog));
		assert_eq!(dialog.text(artist_window::STATUS), "Loading…");
		destroy(dialog);
	}

	#[test]
	fn without_main_thread_work_an_artist_failure_is_still_told() {
		let api = installed_api();
		let fake = installed_backend();
		api.remove_services(main_thread::MANAGER_IID);
		fake.related_with(|_, _| Err(CoreError::Unavailable("gone".to_owned())));
		let dialog = look_up_artists_while_loading();
		wait_until(|| !api.popups().is_empty());
		destroy(dialog);
		assert_eq!(
			api.popups(),
			[(
				"Can't open this artist".to_owned(),
				"In Christ Alone\n\ngone.".to_owned(),
				PopupIcon::Error
			)]
		);
		assert!(api.console_lines().contains(
			&"foo_librespot: can't close the artist window: foobar2000 has no main_thread_callback_manager".to_owned()
		));
	}

	#[test]
	fn a_failure_shows_a_popup_naming_the_track() {
		let api = installed_api();
		let fake = installed_backend();
		fake.related_with(|_, _| {
			Err(CoreError::Unavailable(
				"Spotify no longer lists Someone Else on this track".to_owned(),
			))
		});
		let artist = Related::Artist(Some("Someone Else".to_owned()));
		open_related(getty_track(), artist.clone(), "In Christ Alone".to_owned(), None);
		wait_until(|| !api.popups().is_empty());
		assert_eq!(fake.related_calls(), [(getty_track(), artist)]);
		assert_eq!(
			api.popups(),
			[(
				"Can't open this artist".to_owned(),
				"In Christ Alone\n\nSpotify no longer lists Someone Else on this track.".to_owned(),
				PopupIcon::Error
			)]
		);
		assert!(api.console_lines().contains(
			&"foo_librespot: can't open the artist of In Christ Alone: Spotify no longer lists Someone Else on this track".to_owned()
		));
		assert!(api.location_requests().is_empty());
	}

	#[test]
	fn logged_out_opens_the_login_window_and_shows_no_popup() {
		let api = installed_api();
		let fake = installed_backend();
		let prompt = installed_prompt();
		fake.require_login();
		open_related(getty_track(), Related::Radio, "In Christ Alone".to_owned(), None);
		wait_until(|| prompt.shown() == 1);
		assert_eq!(fake.logins_started(), 1);
		std::thread::sleep(std::time::Duration::from_millis(200));
		assert_eq!(api.run_main_thread(), 0);
		assert!(!reported_a_failure(api), "{:?} {:?}", api.popups(), api.console_lines());
	}

	#[test]
	fn an_aborted_lookup_reports_nothing() {
		let api = installed_api();
		let fake = installed_backend();
		fake.related_with(|_, _| Err(CoreError::Aborted));
		open_related(getty_track(), Related::Album, "In Christ Alone".to_owned(), None);
		wait_until(|| fake.related_calls().len() == 1);
		std::thread::sleep(std::time::Duration::from_millis(200));
		assert_eq!(api.run_main_thread(), 0);
		assert!(!reported_a_failure(api), "{:?} {:?}", api.popups(), api.console_lines());
	}

	#[test]
	fn a_tab_that_cant_be_opened_shows_a_popup_naming_the_track() {
		let api = installed_api();
		installed_backend();
		api.remove_services(playlist::MANAGER_IID);
		open_related(getty_track(), Related::Album, "In Christ Alone".to_owned(), None);
		run_main_thread_work(api);
		assert_eq!(
			api.popups(),
			[(
				"Can't open this album".to_owned(),
				"In Christ Alone\n\nfoobar2000 has no playlist_manager.".to_owned(),
				PopupIcon::Error
			)]
		);
		assert!(api.console_lines().contains(
			&"foo_librespot: can't open the album of In Christ Alone: foobar2000 has no playlist_manager".to_owned()
		));
	}

	#[test]
	fn without_main_thread_work_the_failure_is_still_told() {
		let api = installed_api();
		installed_backend();
		api.remove_services(main_thread::MANAGER_IID);
		open_related(getty_track(), Related::Album, "In Christ Alone".to_owned(), None);
		wait_until(|| !api.popups().is_empty());
		assert_eq!(
			api.popups(),
			[(
				"Can't open this album".to_owned(),
				"In Christ Alone\n\nfoobar2000 has no main_thread_callback_manager.".to_owned(),
				PopupIcon::Error
			)]
		);
		assert!(api.location_requests().is_empty());
	}
}
