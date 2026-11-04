//! Turning pasted, dragged or added Spotify links into playlist entries.
//!
//! Links reach the playlist loader two ways:
//! - `spotify:` URIs and `sptf://` paths are claimed by our filesystem, whose stub file for an
//!   album or playlist carries [`CONTENT_TYPE`];
//! - `https://open.spotify.com/…` links are claimed by foobar2000's own HTTP filesystem, which
//!   fetches the page and reports `text/html`, as it did for foo_spotify. Every other web page
//!   the user adds arrives too, and is turned away before any Spotify call.

use std::fmt::Write as _;

use foo_librespot_core::CoreError;
use foo_librespot_core::link::{Link, LinkKind, PATH_SCHEME, parse_link, track_path};
use foo_librespot_core::resolve::Skipped;
use oxibar::abort::Abort;
use oxibar::console;
use oxibar::error::{Error, Result};
use oxibar::file_info_impl::FileInfoImpl;
use oxibar::filesystem_service::Filesystem;
use oxibar::playlist_loader::{LoaderCallback, PlaylistLoader};
use oxibar::popup::{self, PopupIcon};

use crate::backend::backend;
use crate::errors::host_error;
use crate::fields::fill_info;
use crate::first_use::login_on_first_use;

/// The content type of the stub file for a Spotify album or playlist path.
pub const CONTENT_TYPE: &str = "application/x-foo-librespot";

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
	text.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// The one path foobar2000 stores for `link`.
fn canonical(link: &Link) -> String {
	let kind = match link.kind {
		LinkKind::Track => return track_path(link.id),
		LinkKind::Album => "album",
		LinkKind::Playlist => "playlist",
	};
	format!("{PATH_SCHEME}spotify:{kind}:{}", link.id.to_base62())
}

/// Claims `sptf://` paths and `spotify:` URIs.
pub struct SpotifyFilesystem;

impl Filesystem for SpotifyFilesystem {
	fn is_our_path(path: &str) -> bool {
		starts_with_ignore_case(path, PATH_SCHEME) || starts_with_ignore_case(path, "spotify:")
	}

	fn canonical_path(path: &str) -> Option<String> {
		parse_link(path).ok().map(|link| canonical(&link))
	}

	fn display_path(path: &str) -> Option<String> {
		Self::canonical_path(path).map(|canonical| canonical[PATH_SCHEME.len()..].to_owned())
	}

	fn content_type(path: &str) -> Option<String> {
		match parse_link(path) {
			Ok(Link { kind: LinkKind::Album | LinkKind::Playlist, .. }) => {
				Some(CONTENT_TYPE.to_owned())
			}
			_ => None,
		}
	}
}

/// How many skipped items one popup lists; the console has them all.
const MAX_LISTED: usize = 20;

const fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
	if count == 1 { one } else { many }
}

/// The popup after an import that skipped items: what was added, then up to [`MAX_LISTED`]
/// of the skipped items with the reason for each.
fn skipped_message(added: usize, source: &str, skipped: &[Skipped]) -> String {
	let tracks = plural(added, "track", "tracks");
	let items = plural(skipped.len(), "item", "items");
	let mut message = format!(
		"Added {added} {tracks} from {source}.\n\n{} {items} could not be added:",
		skipped.len()
	);
	for item in skipped.iter().take(MAX_LISTED) {
		let _ = write!(message, "\n{}: {}", item.uri, item.reason);
	}
	if skipped.len() > MAX_LISTED {
		let _ = write!(message, "\n…and {} more; see the console.", skipped.len() - MAX_LISTED);
	}
	message
}

/// Expands Spotify track, album and playlist links into one entry per track.
pub struct SpotifyLoader;

impl PlaylistLoader for SpotifyLoader {
	fn is_our_content_type(content_type: &str) -> bool {
		content_type == CONTENT_TYPE || starts_with_ignore_case(content_type, "text/html")
	}

	fn open(path: &str, callback: &mut LoaderCallback<'_>, abort: &Abort) -> Result<()> {
		// Decided from the path alone, so other web pages cost nothing.
		let link = parse_link(path)
			.map_err(|error| Error::unsupported_format(format!("not a Spotify link: {error}")))?;
		callback.on_progress(path)?;
		let is_aborting = || abort.is_aborting();
		let resolved = match backend().resolve(&link, &is_aborting) {
			// The first link of a new user: log in, then try once more.
			Err(CoreError::NotLoggedIn) => login_on_first_use(&is_aborting)
				.and_then(|()| backend().resolve(&link, &is_aborting)),
			resolved => resolved,
		}
		.map_err(host_error)?;
		for track in &resolved.tracks {
			abort.check()?;
			let mut info = FileInfoImpl::new();
			fill_info(&mut info.file_info(), track)?;
			callback.add_entry(&track_path(track.id), &mut info)?;
		}
		for skipped in &resolved.skipped {
			console::print(&format!("foo_librespot: skipped {}: {}", skipped.uri, skipped.reason));
		}
		let source = resolved.name.as_deref().unwrap_or("the link");
		console::print(&format!(
			"foo_librespot: added {} tracks from {source}",
			resolved.tracks.len()
		));
		if !resolved.skipped.is_empty() {
			let message = skipped_message(resolved.tracks.len(), source, &resolved.skipped);
			if let Err(error) = popup::show("foo_librespot", &message, PopupIcon::Information) {
				console::print(&format!("foo_librespot: {}", error.message));
			}
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::time::{Duration, Instant};

	use foo_librespot_core::resolve::{Resolved, SkipReason, Skipped};
	use foo_librespot_core::{CoreError, SpotifyId};
	use oxibar::abi::file::OPEN_MODE_READ;
	use oxibar::cxx::ExceptionKind;
	use oxibar::filesystem_service::filesystem_service;
	use oxibar::playlist_loader::playlist_loader_service;
	use oxibar::popup::PopupIcon;
	use oxibar::testing::{
		FakeAbort, FakeApi, FakeLoaderCallback, FilesystemHandle, PlaylistLoaderHandle,
	};

	use super::{CONTENT_TYPE, SpotifyFilesystem, SpotifyLoader};
	use crate::backend;
	use crate::login::controller;
	use crate::testing::{
		FakeBackend, TRACK_ID, confirm_login_when_started, installed_prompt, stop_after, track_info,
	};

	const ALBUM: &str = "4wPtS24asgBAG4VOmYYB9O";

	fn filesystem() -> FilesystemHandle {
		FilesystemHandle::new(filesystem_service::<SpotifyFilesystem>())
	}

	fn loader() -> PlaylistLoaderHandle {
		PlaylistLoaderHandle::new(playlist_loader_service::<SpotifyLoader>())
	}

	fn installed() -> FakeBackend {
		let fake = FakeBackend::default();
		backend::install(Box::new(fake.clone()));
		fake
	}

	fn id(byte: u8) -> SpotifyId {
		SpotifyId::from_raw(&[byte; 16]).unwrap()
	}

	fn album() -> Resolved {
		Resolved {
			name: Some("In Christ Alone".to_owned()),
			tracks: ["There Is A Higher Throne", "Across The Lands", "Jesus, Draw Me Ever Nearer"]
				.into_iter()
				.zip(1u8..)
				.map(|(title, byte)| Arc::new(track_info(id(byte), title)))
				.collect(),
			skipped: Vec::new(),
		}
	}

	#[test]
	fn filesystem_claims_sptf_and_spotify_uris_only() {
		let filesystem = filesystem();
		assert!(filesystem.is_our_path(&format!("sptf://spotify:track:{TRACK_ID}")));
		assert!(filesystem.is_our_path(&format!("spotify:album:{ALBUM}")));
		assert!(!filesystem.is_our_path(&format!("https://open.spotify.com/album/{ALBUM}")));
		assert!(!filesystem.is_our_path("file://C:\\a.mp3"));
	}

	#[test]
	fn canonical_paths_for_each_kind() {
		let filesystem = filesystem();
		let canonical = format!("sptf://spotify:album:{ALBUM}");
		assert_eq!(
			filesystem.canonical_path(&format!("spotify:album:{ALBUM}")).as_deref(),
			Some(canonical.as_str())
		);
		assert_eq!(
			filesystem
				.canonical_path(&format!("https://open.spotify.com/intl-nl/album/{ALBUM}?si=x"))
				.as_deref(),
			Some(canonical.as_str())
		);
		assert_eq!(
			filesystem.canonical_path(&format!("spotify:track:{TRACK_ID}")).as_deref(),
			Some(format!("sptf://spotify:track:{TRACK_ID}").as_str())
		);
		assert_eq!(filesystem.canonical_path("file://C:\\a.mp3"), None);
		assert_eq!(
			filesystem.display_path(&canonical).as_deref(),
			Some(&canonical["sptf://".len()..])
		);
	}

	#[test]
	fn album_paths_have_the_private_content_type_and_tracks_none() {
		let filesystem = filesystem();
		let album =
			filesystem.open(&format!("sptf://spotify:album:{ALBUM}"), OPEN_MODE_READ).unwrap();
		assert_eq!(album.content_type().as_deref(), Some(CONTENT_TYPE));
		let track =
			filesystem.open(&format!("sptf://spotify:track:{TRACK_ID}"), OPEN_MODE_READ).unwrap();
		assert_eq!(track.content_type(), None);
	}

	#[test]
	fn loader_takes_our_content_type_and_html_pages() {
		let loader = loader();
		assert!(loader.is_our_content_type(CONTENT_TYPE));
		assert!(loader.is_our_content_type("text/html; charset=utf-8"));
		assert!(loader.is_our_content_type("TEXT/HTML"));
		assert!(!loader.is_our_content_type("audio/mpeg"));
	}

	#[test]
	fn loader_rejects_non_spotify_pages_without_the_backend() {
		let fake = installed();
		for path in ["https://example.com/", &format!("https://open.spotify.com/artist/{ALBUM}")] {
			let (callback, record) = FakeLoaderCallback::new();
			let error = loader().open(path, callback, &FakeAbort::new(false)).unwrap_err();
			assert_eq!(error.kind, ExceptionKind::IoUnsupportedFormat, "{path}");
			assert!(record.lock().unwrap().entries.is_empty());
		}
		assert_eq!(fake.calls(), 0);
	}

	#[test]
	fn album_link_adds_its_tracks_in_order_with_info() {
		let fake = installed();
		fake.resolve_to(album());
		let (callback, record) = FakeLoaderCallback::new();
		let path = format!("https://open.spotify.com/album/{ALBUM}");
		loader().open(&path, callback, &FakeAbort::new(false)).unwrap();
		let record = record.lock().unwrap();
		let paths: Vec<String> = record.entries.iter().map(|entry| entry.path.clone()).collect();
		let expected: Vec<String> =
			(1..=3).map(|byte| format!("sptf://spotify:track:{}", id(byte).to_base62())).collect();
		assert_eq!(paths, expected);
		assert_eq!(record.entries[1].meta("TITLE"), ["Across The Lands"]);
		assert!((record.entries[0].replaygain.track_gain - -6.62).abs() < 1e-4);
		assert_eq!(record.live_handles(), 0);
	}

	#[test]
	fn skipped_items_are_reported_to_the_console() {
		let api = FakeApi::new();
		api.install();
		let fake = installed();
		let mut resolved = album();
		resolved
			.skipped
			.push(Skipped { uri: "spotify:episode:abc".to_owned(), reason: SkipReason::Episode });
		fake.resolve_to(resolved);
		let (callback, _record) = FakeLoaderCallback::new();
		loader()
			.open(&format!("spotify:playlist:{ALBUM}"), callback, &FakeAbort::new(false))
			.unwrap();
		let lines = api.console_lines();
		assert!(
			lines.iter().any(|line| line
				== "foo_librespot: skipped spotify:episode:abc: podcast episodes can't be played"),
			"{lines:?}"
		);
		assert!(
			lines.iter().any(|line| line == "foo_librespot: added 3 tracks from In Christ Alone"),
			"{lines:?}"
		);
	}

	fn with_skipped(count: usize) -> Resolved {
		let mut resolved = album();
		resolved.skipped = (0..count)
			.map(|index| Skipped {
				uri: format!("spotify:episode:{index:022}"),
				reason: SkipReason::Episode,
			})
			.collect();
		resolved
	}

	#[test]
	fn import_with_skipped_items_shows_one_popup() {
		let api = FakeApi::new();
		api.install();
		let fake = installed();
		fake.resolve_to(with_skipped(2));
		let (callback, _record) = FakeLoaderCallback::new();
		loader()
			.open(&format!("spotify:playlist:{ALBUM}"), callback, &FakeAbort::new(false))
			.unwrap();
		let popups = api.popups();
		assert_eq!(popups.len(), 1, "{popups:?}");
		let (title, message, icon) = &popups[0];
		assert_eq!(title, "foo_librespot");
		assert_eq!(*icon, PopupIcon::Information);
		assert_eq!(
			message,
			"Added 3 tracks from In Christ Alone.\n\n2 items could not be added:\n\
			 spotify:episode:0000000000000000000000: podcast episodes can't be played\n\
			 spotify:episode:0000000000000000000001: podcast episodes can't be played"
		);
	}

	#[test]
	fn import_without_skips_shows_no_popup() {
		let api = FakeApi::new();
		api.install();
		let fake = installed();
		fake.resolve_to(album());
		let (callback, _record) = FakeLoaderCallback::new();
		loader().open(&format!("spotify:album:{ALBUM}"), callback, &FakeAbort::new(false)).unwrap();
		assert!(api.popups().is_empty());
	}

	#[test]
	fn one_popup_lists_at_most_20_skipped_items() {
		let api = FakeApi::new();
		api.install();
		let fake = installed();
		fake.resolve_to(with_skipped(25));
		let (callback, _record) = FakeLoaderCallback::new();
		loader()
			.open(&format!("spotify:playlist:{ALBUM}"), callback, &FakeAbort::new(false))
			.unwrap();
		let popups = api.popups();
		assert_eq!(popups.len(), 1);
		let lines: Vec<&str> = popups[0].1.lines().collect();
		assert_eq!(lines[2], "25 items could not be added:");
		assert_eq!(lines.len(), 3 + 20 + 1, "{lines:?}");
		assert_eq!(
			lines[22],
			"spotify:episode:0000000000000000000019: podcast episodes can't be played"
		);
		assert_eq!(lines[23], "…and 5 more; see the console.");
	}

	#[test]
	fn aborted_import_throws_aborted() {
		let fake = installed();
		fake.fail_with(|| CoreError::Aborted);
		let (callback, _record) = FakeLoaderCallback::new();
		let error = loader()
			.open(&format!("spotify:album:{ALBUM}"), callback, &FakeAbort::new(false))
			.unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Aborted);
	}

	fn import_album(abort: &FakeAbort) -> oxibar::error::Result<usize> {
		let (callback, record) = FakeLoaderCallback::new();
		loader().open(&format!("spotify:album:{ALBUM}"), callback, abort)?;
		Ok(record.lock().unwrap().entries.len())
	}

	#[test]
	fn loader_logged_out_prompts_waits_and_adds_the_tracks() {
		let fake = installed();
		let prompt = installed_prompt();
		fake.require_login();
		fake.resolve_to(album());
		let user = confirm_login_when_started(&fake, 1);
		assert_eq!(import_album(&FakeAbort::new(false)).unwrap(), 3);
		user.join().unwrap();
		assert_eq!(prompt.shown(), 1);
		assert_eq!(fake.logins_started(), 1);
	}

	#[test]
	fn loader_tries_again_only_once() {
		let fake = installed();
		let _prompt = installed_prompt();
		fake.fail_with(|| CoreError::NotLoggedIn);
		let user = confirm_login_when_started(&fake, 1);
		let error = import_album(&FakeAbort::new(false)).unwrap_err();
		user.join().unwrap();
		assert_eq!(error.kind, ExceptionKind::Io);
		assert!(error.message.contains("Not logged in to Spotify"), "{}", error.message);
		assert_eq!(fake.calls(), 2);
	}

	#[test]
	fn loader_stops_waiting_when_aborted() {
		let fake = installed();
		let _prompt = installed_prompt();
		fake.require_login();
		let abort = FakeAbort::new(false);
		let stopper = stop_after(&abort, Duration::from_millis(100));
		let started = Instant::now();
		let error = import_album(&abort).unwrap_err();
		stopper.join().unwrap();
		assert_eq!(error.kind, ExceptionKind::Aborted);
		assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
	}

	#[test]
	fn loader_reports_a_cancelled_login() {
		let fake = installed();
		let _prompt = installed_prompt();
		fake.require_login();
		let user = std::thread::spawn(move || {
			while fake.logins_started() == 0 {
				std::thread::sleep(Duration::from_millis(5));
			}
			controller().cancel();
		});
		let error = import_album(&FakeAbort::new(false)).unwrap_err();
		user.join().unwrap();
		assert_eq!(error.kind, ExceptionKind::Io);
		assert!(error.message.contains("Not logged in to Spotify"), "{}", error.message);
	}

	#[test]
	fn two_loaders_share_one_window() {
		let fake = installed();
		let _prompt = installed_prompt();
		fake.require_login();
		fake.resolve_to(album());
		let imports: Vec<_> =
			(0..2).map(|_| std::thread::spawn(|| import_album(&FakeAbort::new(false)))).collect();
		let user = confirm_login_when_started(&fake, 1);
		for import in imports {
			assert_eq!(import.join().unwrap().unwrap(), 3);
		}
		user.join().unwrap();
		assert_eq!(fake.logins_started(), 1);
	}

	#[test]
	fn loader_on_the_main_thread_prompts_but_does_not_wait() {
		// The fake host answers that every thread is its main thread.
		FakeApi::new().install();
		let fake = installed();
		let prompt = installed_prompt();
		fake.require_login();
		let started = Instant::now();
		let error = import_album(&FakeAbort::new(false)).unwrap_err();
		assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
		assert_eq!(error.kind, ExceptionKind::Io);
		assert_eq!(prompt.shown(), 1);
		assert_eq!(fake.logins_started(), 1);
	}
}
