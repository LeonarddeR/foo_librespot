//! Opening what a Spotify track belongs to, its album, an artist, its radio or its show, in a
//! playlist tab of its own.
//!
//! The lookup runs in the background; the tab opens on the main thread once it ends. When a
//! track's artists are asked for and it has several, the artist window lists them instead.
//! Every failure is told in the console and a popup; none reaches foobar2000.

use std::fmt::Display;

use foo_librespot_core::CoreError;
use foo_librespot_core::link::ItemId;
use foo_librespot_core::related::{Found, Opened, Related};
use foxide2k::console;
use foxide2k::main_thread;
use foxide2k::popup::PopupIcon;

use crate::artist_window;
use crate::backend::backend;
use crate::errors::show_failure;
use crate::first_use::ask_for_login;
use crate::tabs::open_in_tab;

/// Starts finding `what` for `item` and returns at once; what was found then opens in its tab.
/// `title` names the track in a failure: its stored title, or its URI. Main thread only.
#[cfg_attr(not(test), expect(dead_code, reason = "the Spotify menus will open related items"))]
pub fn open_related(item: ItemId, what: Related, title: String) {
	let noun = noun(&what);
	backend().related(item, what, Box::new(move |result| end(result, noun, &title)));
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

/// On the main thread, opens what the lookup found, or lists the artists to pick from when it
/// found several. Otherwise asks for a login, or says why it failed.
fn end(result: Result<Found, CoreError>, noun: &'static str, title: &str) {
	let found = match result {
		Ok(found) => found,
		Err(CoreError::NotLoggedIn) => return ask_for_login(),
		Err(CoreError::Aborted) => return,
		Err(error) => return fail(noun, title, &error),
	};
	let queued_title = title.to_owned();
	let queued = main_thread::run(Box::new(move || match found {
		Found::One(opened) => open(&opened, noun, &queued_title),
		Found::Artists(artists) => artist_window::show_choices(artists),
	}));
	if let Err(error) = queued {
		fail(noun, title, &error.message);
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
	use foo_librespot_core::CoreError;
	use foo_librespot_core::link::ItemId;
	use foo_librespot_core::related::Related;
	use foxide2k::abi::{main_thread, playlist};
	use foxide2k::dialog::host::destroy;
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::FakeApi;

	use super::open_related;
	use crate::artist_window;
	use crate::testing::{
		installed_backend, installed_prompt, installed_screen, track_id, wait_until,
	};

	const GETTY_TRACK: &str = "sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO";

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
		open_related(getty_track(), Related::Album, GETTY_TRACK.to_owned());
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
		let screen = installed_screen();
		open_related(getty_track(), Related::Artist(None), "In Christ Alone".to_owned());
		run_main_thread_work(api);
		assert_eq!(fake.related_calls(), [(getty_track(), Related::Artist(None))]);
		let dialog = artist_window::WINDOW.current().expect("the artist window is open");
		let texts: Vec<String> = (0..dialog.list_count(artist_window::LIST))
			.map(|row| dialog.list_text(artist_window::LIST, row))
			.collect();
		assert_eq!(texts, ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(screen.brought_up(), 1);
		assert!(api.location_requests().is_empty());
		assert!(!reported_a_failure(api), "{:?} {:?}", api.popups(), api.console_lines());
		destroy(dialog);
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
		open_related(getty_track(), artist.clone(), "In Christ Alone".to_owned());
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
		open_related(getty_track(), Related::Radio, "In Christ Alone".to_owned());
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
		open_related(getty_track(), Related::Album, "In Christ Alone".to_owned());
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
		open_related(getty_track(), Related::Album, "In Christ Alone".to_owned());
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
		open_related(getty_track(), Related::Album, "In Christ Alone".to_owned());
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
