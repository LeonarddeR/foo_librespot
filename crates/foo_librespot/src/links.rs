//! Turning pasted, dragged or added Spotify links into playlist entries.
//!
//! Links reach the playlist loader two ways. foobar2000 asks services in an order that changes
//! every time it starts, so either can come first:
//! - our filesystem claims `sptf://` paths, `spotify:` URIs and `open.spotify.com` links. Its
//!   stub file for every link carries [`CONTENT_TYPE`], which only our loader takes.
//! - foobar2000's own HTTP filesystem fetches an `open.spotify.com` page and reports
//!   `text/html`, as it did for foo_spotify. Our loader takes that too, and turns away every other
//!   web page before any Spotify call. A loader asked first, such as `foo_youtube`'s, can take the
//!   page instead.

use std::fmt::Write as _;

use foo_librespot_core::CoreError;
use foo_librespot_core::link::{Link, LinkError, PATH_SCHEME, item_path, parse_link};
use foo_librespot_core::resolve::Skipped;
use foxide2k::abort::Abort;
use foxide2k::console;
use foxide2k::error::{Error, Result};
use foxide2k::file_info_impl::FileInfoImpl;
use foxide2k::filesystem_service::Filesystem;
use foxide2k::playlist_loader::{LoaderCallback, PlaylistLoader};
use foxide2k::popup::{self, PopupIcon};
use foxide2k::strings::strip_prefix_ignore_case;

use crate::backend::backend;
use crate::errors::host_error;
use crate::fields::fill_info;
use crate::first_use::login_on_first_use;

/// The content type of the stub file for a Spotify link.
pub const CONTENT_TYPE: &str = "application/x-foo-librespot";

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
	strip_prefix_ignore_case(text, prefix).is_some()
}

/// The one path foobar2000 stores for `link`.
fn canonical(link: &Link) -> String {
	format!("{PATH_SCHEME}{}", link.uri().to_uri())
}

/// Claims `sptf://` paths, `spotify:` URIs and Spotify's web links.
pub struct SpotifyFilesystem;

impl Filesystem for SpotifyFilesystem {
	fn is_our_path(path: &str) -> bool {
		starts_with_ignore_case(path, PATH_SCHEME)
			|| starts_with_ignore_case(path, "spotify:")
			// Only links that parse as Spotify links: every other web page stays with
			// foobar2000's own HTTP filesystem.
			|| (starts_with_ignore_case(path, "http") && parse_link(path).is_ok())
	}

	fn canonical_path(path: &str) -> Option<String> {
		parse_link(path).ok().map(|link| canonical(&link))
	}

	fn display_path(path: &str) -> Option<String> {
		Self::canonical_path(path).map(|canonical| canonical[PATH_SCHEME.len()..].to_owned())
	}

	/// Every path we claim goes through the loader. It asks for a login when the user is logged
	/// out, which the input never does while adding, and it adds the entry with its info filled
	/// in. An episode Spotify doesn't host becomes the podcast's own audio link, which only the
	/// loader can add. A Spotify link that can't be played goes there too, so the user hears why.
	fn content_type(_path: &str) -> Option<String> {
		Some(CONTENT_TYPE.to_owned())
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

/// Expands Spotify links into one entry per track or episode.
pub struct SpotifyLoader;

impl PlaylistLoader for SpotifyLoader {
	fn is_our_content_type(content_type: &str) -> bool {
		content_type == CONTENT_TYPE || starts_with_ignore_case(content_type, "text/html")
	}

	fn open(path: &str, callback: &mut LoaderCallback<'_>, abort: &Abort) -> Result<()> {
		// Decided from the path alone, so other web pages cost nothing.
		let link = match parse_link(path) {
			Ok(link) => link,
			// Another web page, which another loader may take; nothing to tell the user.
			Err(LinkError::NotSpotify) => {
				return Err(Error::unsupported_format(format!("not a Spotify link: {path}")));
			}
			// foobar2000 drops a loader's error without a word, so the user is told here.
			Err(error) => {
				console::print(&format!("foo_librespot: {path}: {error}"));
				let message = format!(
					"{path}

{error}."
				);
				if let Err(shown) = popup::show("foo_librespot", &message, PopupIcon::Information) {
					console::print(&format!("foo_librespot: {}", shown.message));
				}
				return Err(Error::unsupported_format(error.to_string()));
			}
		};
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
			// An episode Spotify doesn't host is the podcast's own audio link, which foobar2000
			// plays itself.
			let path = track.external_url.clone().unwrap_or_else(|| item_path(track.id));
			callback.add_entry(&path, &mut info)?;
		}
		for skipped in &resolved.skipped {
			console::print(&format!("foo_librespot: skipped {}: {}", skipped.uri, skipped.reason));
		}
		let source = resolved.name.as_deref().unwrap_or("the link");
		let added = resolved.tracks.len();
		console::print(&format!(
			"foo_librespot: added {added} {} from {source}",
			plural(added, "track", "tracks")
		));
		if !resolved.skipped.is_empty() {
			let message = skipped_message(added, source, &resolved.skipped);
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
	use foxide2k::abi::file::OPEN_MODE_READ;
	use foxide2k::cxx::ExceptionKind;
	use foxide2k::filesystem_service::filesystem_service;
	use foxide2k::playlist_loader::playlist_loader_service;
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::{
		FakeAbort, FakeApi, FakeLoaderCallback, FilesystemHandle, PlaylistLoaderHandle,
	};

	use super::{CONTENT_TYPE, SpotifyFilesystem, SpotifyLoader};

	use crate::testing::{
		EPISODE_ID, TRACK_ID, cancel_login_when_started, confirm_login_when_started, episode_info,
		installed_backend, installed_prompt, stop_after, track_id, track_info,
	};

	const ALBUM: &str = "4wPtS24asgBAG4VOmYYB9O";

	fn filesystem() -> FilesystemHandle {
		FilesystemHandle::new(filesystem_service::<SpotifyFilesystem>())
	}

	fn loader() -> PlaylistLoaderHandle {
		PlaylistLoaderHandle::new(playlist_loader_service::<SpotifyLoader>())
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
	fn filesystem_claims_sptf_paths_spotify_uris_and_spotify_web_links() {
		let filesystem = filesystem();
		assert!(filesystem.is_our_path(&format!("sptf://spotify:track:{TRACK_ID}")));
		assert!(filesystem.is_our_path(&format!("spotify:album:{ALBUM}")));
		assert!(filesystem.is_our_path(&format!("https://open.spotify.com/album/{ALBUM}?si=x")));
		assert!(
			filesystem.is_our_path(&format!("https://open.spotify.com/intl-nl/track/{TRACK_ID}"))
		);
		assert!(filesystem.is_our_path("https://open.spotify.com/artist/1hZbT7cOjvmLOdb5Ey5SHu"));
		assert!(!filesystem.is_our_path("https://open.spotify.com/genre/1hZbT7cOjvmLOdb5Ey5SHu"));
		assert!(!filesystem.is_our_path("https://www.youtube.com/watch?v=abc"));
		assert!(!filesystem.is_our_path("file://C:\\a.mp3"));
	}

	#[test]
	fn web_links_open_as_the_stub_of_their_canonical_path() {
		let filesystem = filesystem();
		let album = filesystem
			.open(&format!("https://open.spotify.com/album/{ALBUM}?si=x"), OPEN_MODE_READ)
			.unwrap();
		assert_eq!(album.content_type().as_deref(), Some(CONTENT_TYPE));
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
		assert_eq!(
			filesystem
				.canonical_path(&format!("https://open.spotify.com/episode/{EPISODE_ID}?si=x"))
				.as_deref(),
			Some(format!("sptf://spotify:episode:{EPISODE_ID}").as_str())
		);
		assert_eq!(
			filesystem.canonical_path("spotify:show:5CfCWKI5pZ28U0uOzXkDHe").as_deref(),
			Some("sptf://spotify:show:5CfCWKI5pZ28U0uOzXkDHe")
		);
		assert_eq!(filesystem.canonical_path("file://C:\\a.mp3"), None);
		assert_eq!(
			filesystem.display_path(&canonical).as_deref(),
			Some(&canonical["sptf://".len()..])
		);
	}

	#[test]
	fn every_link_kind_has_the_private_content_type() {
		let filesystem = filesystem();
		let album =
			filesystem.open(&format!("sptf://spotify:album:{ALBUM}"), OPEN_MODE_READ).unwrap();
		assert_eq!(album.content_type().as_deref(), Some(CONTENT_TYPE));
		// Through the loader, so adding a track link asks for a login like any other link.
		let track =
			filesystem.open(&format!("sptf://spotify:track:{TRACK_ID}"), OPEN_MODE_READ).unwrap();
		assert_eq!(track.content_type().as_deref(), Some(CONTENT_TYPE));
		let show =
			filesystem.open("sptf://spotify:show:5CfCWKI5pZ28U0uOzXkDHe", OPEN_MODE_READ).unwrap();
		assert_eq!(show.content_type().as_deref(), Some(CONTENT_TYPE));
		let episode = filesystem
			.open(&format!("sptf://spotify:episode:{EPISODE_ID}"), OPEN_MODE_READ)
			.unwrap();
		assert_eq!(episode.content_type().as_deref(), Some(CONTENT_TYPE));
	}

	#[test]
	fn one_added_item_is_one_track_on_the_console() {
		let api = FakeApi::new();
		api.install();
		let fake = installed_backend();
		fake.resolve_to(Resolved {
			name: None,
			tracks: vec![Arc::new(episode_info(id(1), "Our Ransom (Part 1)"))],
			skipped: Vec::new(),
		});
		let (callback, _record) = FakeLoaderCallback::new();
		let path = format!("sptf://spotify:episode:{}", id(1).to_base62());
		loader().open(&path, callback, &FakeAbort::new(false)).unwrap();
		let lines = api.console_lines();
		assert!(
			lines.iter().any(|line| line == "foo_librespot: added 1 track from the link"),
			"{lines:?}"
		);
	}

	#[test]
	fn an_episode_link_hosted_elsewhere_adds_the_podcasts_own_link() {
		let fake = installed_backend();
		let mut elsewhere = episode_info(id(2), "Hosted elsewhere");
		elsewhere.has_audio = false;
		elsewhere.external_url = Some("https://podcasts.example/12.mp3".to_owned());
		fake.resolve_to(Resolved {
			name: None,
			tracks: vec![Arc::new(elsewhere)],
			skipped: Vec::new(),
		});
		let (callback, record) = FakeLoaderCallback::new();
		let path = format!("sptf://spotify:episode:{}", id(2).to_base62());
		loader().open(&path, callback, &FakeAbort::new(false)).unwrap();
		let record = record.lock().unwrap();
		let paths = record.paths();
		assert_eq!(paths, ["https://podcasts.example/12.mp3"]);
	}

	#[test]
	fn show_adds_spotify_episodes_as_paths_and_others_under_their_own_link() {
		let fake = installed_backend();
		let mut elsewhere = episode_info(id(2), "Hosted elsewhere");
		elsewhere.has_audio = false;
		elsewhere.external_url = Some("https://podcasts.example/12.mp3".to_owned());
		fake.resolve_to(Resolved {
			name: Some("Sing! Podcast".to_owned()),
			tracks: vec![Arc::new(episode_info(id(1), "On Spotify")), Arc::new(elsewhere)],
			skipped: Vec::new(),
		});
		let (callback, record) = FakeLoaderCallback::new();
		loader()
			.open("sptf://spotify:show:5CfCWKI5pZ28U0uOzXkDHe", callback, &FakeAbort::new(false))
			.unwrap();
		let record = record.lock().unwrap();
		let paths = record.paths();
		assert_eq!(
			paths,
			[
				format!("sptf://spotify:episode:{}", id(1).to_base62()),
				"https://podcasts.example/12.mp3".to_owned()
			]
		);
		assert_eq!(record.entries[1].meta("TITLE"), ["Hosted elsewhere"]);
		assert_eq!(record.entries[1].meta("GENRE"), ["Podcast"]);
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
		let fake = installed_backend();
		for path in ["https://example.com/", "https://www.youtube.com/watch?v=abc"] {
			let (callback, record) = FakeLoaderCallback::new();
			let error = loader().open(path, callback, &FakeAbort::new(false)).unwrap_err();
			assert_eq!(error.kind, ExceptionKind::IoUnsupportedFormat, "{path}");
			assert!(record.lock().unwrap().entries.is_empty());
		}
		assert_eq!(fake.calls(), 0);
	}

	#[test]
	fn album_link_adds_its_tracks_in_order_with_info() {
		let fake = installed_backend();
		fake.resolve_to(album());
		let (callback, record) = FakeLoaderCallback::new();
		let path = format!("https://open.spotify.com/album/{ALBUM}");
		loader().open(&path, callback, &FakeAbort::new(false)).unwrap();
		let record = record.lock().unwrap();
		let paths = record.paths();
		let expected: Vec<String> =
			(1..=3).map(|byte| format!("sptf://spotify:track:{}", id(byte).to_base62())).collect();
		assert_eq!(paths, expected);
		assert_eq!(record.entries[1].meta("TITLE"), ["Across The Lands"]);
		assert!((record.entries[0].replaygain.track_gain - -6.62).abs() < 1e-4);
		assert_eq!(record.live_handles(), 0);
	}

	#[test]
	fn artist_link_goes_to_the_loader_and_adds_the_popular_tracks() {
		let path = "https://open.spotify.com/artist/2UV3Ygbhy1gXAbbrQ9BHdQ";
		let stub = filesystem().open(path, OPEN_MODE_READ).unwrap();
		assert_eq!(stub.content_type().as_deref(), Some(CONTENT_TYPE));
		let fake = installed_backend();
		fake.resolve_to(Resolved {
			name: Some("Keith & Kristyn Getty".to_owned()),
			tracks: vec![
				Arc::new(track_info(id(2), "In Christ Alone")),
				Arc::new(track_info(id(1), "The Power of the Cross")),
			],
			skipped: Vec::new(),
		});
		let (callback, record) = FakeLoaderCallback::new();
		loader().open(path, callback, &FakeAbort::new(false)).unwrap();
		let record = record.lock().unwrap();
		assert_eq!(record.entries[0].meta("TITLE"), ["In Christ Alone"]);
		assert_eq!(record.entries.len(), 2);
	}

	#[test]
	fn track_link_adds_one_entry_with_info() {
		let fake = installed_backend();
		fake.resolve_to(Resolved {
			name: None,
			tracks: vec![Arc::new(track_info(track_id(), "In Christ Alone"))],
			skipped: Vec::new(),
		});
		let (callback, record) = FakeLoaderCallback::new();
		let path = format!("https://open.spotify.com/track/{TRACK_ID}");
		loader().open(&path, callback, &FakeAbort::new(false)).unwrap();
		let record = record.lock().unwrap();
		assert_eq!(record.paths(), [format!("sptf://spotify:track:{TRACK_ID}")]);
		assert_eq!(record.entries[0].meta("TITLE"), ["In Christ Alone"]);
	}

	#[test]
	fn skipped_items_are_reported_to_the_console() {
		let api = FakeApi::new();
		api.install();
		let fake = installed_backend();
		let mut resolved = album();
		resolved.skipped.push(Skipped {
			uri: "spotify:local:a:b:c:1".to_owned(),
			reason: SkipReason::LocalFile,
		});
		fake.resolve_to(resolved);
		let (callback, _record) = FakeLoaderCallback::new();
		loader()
			.open(&format!("spotify:playlist:{ALBUM}"), callback, &FakeAbort::new(false))
			.unwrap();
		let lines = api.console_lines();
		assert!(
			lines.iter().any(|line| line
				== "foo_librespot: skipped spotify:local:a:b:c:1: local files in Spotify playlists can't be played"),
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
				uri: format!("spotify:local:a:b:{index}:1"),
				reason: SkipReason::LocalFile,
			})
			.collect();
		resolved
	}

	fn says_why_it_cant_play(path: &str) {
		let api = FakeApi::new();
		api.install();
		let fake = installed_backend();
		// Our filesystem claims spotify: URIs it can't parse, so the loader gets them too.
		let stub = filesystem().open("spotify:local:a:b:c:180", OPEN_MODE_READ).unwrap();
		assert_eq!(stub.content_type().as_deref(), Some(CONTENT_TYPE));
		let (callback, record) = FakeLoaderCallback::new();
		let error = loader().open(path, callback, &FakeAbort::new(false)).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::IoUnsupportedFormat, "{path}");
		assert!(record.lock().unwrap().entries.is_empty());
		let popups = api.popups();
		assert_eq!(popups.len(), 1, "{path}: {popups:?}");
		assert!(popups[0].1.contains("can't be played"), "{path}: {popups:?}");
		assert!(
			api.console_lines().iter().any(|line| line.contains("can't be played")),
			"{path}: {:?}",
			api.console_lines()
		);
		assert_eq!(fake.calls(), 0);
	}

	#[test]
	fn a_local_file_uri_says_why_it_cant_be_played() {
		says_why_it_cant_play("spotify:local:a:b:c:180");
	}

	#[test]
	fn a_user_profile_page_says_why_it_cant_be_played() {
		says_why_it_cant_play("https://open.spotify.com/user/someone");
	}

	#[test]
	fn a_page_that_isnt_spotify_is_turned_away_silently() {
		let api = FakeApi::new();
		api.install();
		let _fake = installed_backend();
		let (callback, _record) = FakeLoaderCallback::new();
		loader().open("https://example.com/", callback, &FakeAbort::new(false)).unwrap_err();
		assert!(api.popups().is_empty(), "{:?}", api.popups());
	}

	#[test]
	fn import_with_skipped_items_shows_one_popup() {
		let api = FakeApi::new();
		api.install();
		let fake = installed_backend();
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
			 spotify:local:a:b:0:1: local files in Spotify playlists can't be played\n\
			 spotify:local:a:b:1:1: local files in Spotify playlists can't be played"
		);
	}

	#[test]
	fn import_without_skips_shows_no_popup() {
		let api = FakeApi::new();
		api.install();
		let fake = installed_backend();
		fake.resolve_to(album());
		let (callback, _record) = FakeLoaderCallback::new();
		loader().open(&format!("spotify:album:{ALBUM}"), callback, &FakeAbort::new(false)).unwrap();
		assert!(api.popups().is_empty());
	}

	#[test]
	fn one_popup_lists_at_most_20_skipped_items() {
		let api = FakeApi::new();
		api.install();
		let fake = installed_backend();
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
			"spotify:local:a:b:19:1: local files in Spotify playlists can't be played"
		);
		assert_eq!(lines[23], "…and 5 more; see the console.");
	}

	#[test]
	fn aborted_import_throws_aborted() {
		let fake = installed_backend();
		fake.fail_with(|| CoreError::Aborted);
		let (callback, _record) = FakeLoaderCallback::new();
		let error = loader()
			.open(&format!("spotify:album:{ALBUM}"), callback, &FakeAbort::new(false))
			.unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Aborted);
	}

	fn import_album(abort: &FakeAbort) -> foxide2k::error::Result<usize> {
		let (callback, record) = FakeLoaderCallback::new();
		loader().open(&format!("spotify:album:{ALBUM}"), callback, abort)?;
		Ok(record.lock().unwrap().entries.len())
	}

	#[test]
	fn loader_logged_out_prompts_waits_and_adds_the_tracks() {
		let fake = installed_backend();
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
		let fake = installed_backend();
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
		let fake = installed_backend();
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
		let fake = installed_backend();
		let _prompt = installed_prompt();
		fake.require_login();
		let user = cancel_login_when_started(&fake);
		let error = import_album(&FakeAbort::new(false)).unwrap_err();
		user.join().unwrap();
		assert_eq!(error.kind, ExceptionKind::Io);
		assert!(error.message.contains("Not logged in to Spotify"), "{}", error.message);
	}

	#[test]
	fn two_loaders_share_one_window() {
		let fake = installed_backend();
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
		let fake = installed_backend();
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
