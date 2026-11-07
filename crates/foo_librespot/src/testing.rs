//! A fake Spotify for the service tests: one Keith & Kristyn Getty track whose audio is the
//! chirp fixture of `foo_librespot_core`.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use foo_librespot_core::audio::decoder::PcmDecoder;
use foo_librespot_core::audio::format::FileFormat;
use foo_librespot_core::library::{
	Change, Edit, Library, LibraryPlaylist, LibraryState, OnChanged, PlaylistContents, Subscribers,
	Target,
};
use foo_librespot_core::link::{ItemId, Link, LinkKind};
use foo_librespot_core::login::DeviceCode;
use foo_librespot_core::metadata::{ItemLinks, TrackInfo};
use foo_librespot_core::related::{Found, Opened, Related, related_in};
use foo_librespot_core::replaygain::ReplayGain;
use foo_librespot_core::resolve::{ArtistTracks, Resolved};
use foo_librespot_core::{CoreError, SpotifyId};
use foxide2k::testing::{FakeApi, FakeFields};

use crate::backend::{Backend, LoginTask, OnCode, OnDone, OnRelated, PcmTrack};

pub const CHIRP: &[u8] = include_bytes!("../../foo_librespot_core/tests/fixtures/chirp.ogg");

/// "In Christ Alone" by Keith & Kristyn Getty, as foobar2000 stores it.
pub const GETTY_TRACK: &str = "sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO";

/// The Spotify ID of [`GETTY_TRACK`].
pub const TRACK_ID: &str = GETTY_TRACK.split_at("sptf://spotify:track:".len()).1;

pub fn track_id() -> SpotifyId {
	SpotifyId::from_base62(TRACK_ID).unwrap()
}

/// [`GETTY_TRACK`] as a Spotify item.
pub fn getty_track() -> ItemId {
	ItemId::Track(track_id())
}

/// A podcast episode, the one the maintainer shared as an example, as foobar2000 stores it.
pub const EPISODE: &str = "sptf://spotify:episode:4QomYTPgZ1mpVbUmxBsUpz";

/// The Spotify ID of [`EPISODE`].
pub const EPISODE_ID: &str = EPISODE.split_at("sptf://spotify:episode:".len()).1;

pub fn episode_id() -> SpotifyId {
	SpotifyId::from_base62(EPISODE_ID).unwrap()
}

/// A local file.
pub const LOCAL: &str = "file://C:\\a.mp3";

/// The album "In Christ Alone".
pub const ALBUM_ID: &str = "4wPtS24asgBAG4VOmYYB9O";
/// Keith Getty.
pub const KEITH_ID: &str = "0I4Bk2s2BUJyykCwtxx8Xx";
/// Kristyn Getty, by a made-up ID.
pub const KRISTYN_ID: &str = "2kxP07DLgs4xlWz8YHlvfh";

/// What foobar2000 stores for "In Christ Alone".
pub const BOTH_GETTYS: FakeFields<'static> =
	&[("ARTIST", &["Keith Getty", "Kristyn Getty"]), ("TITLE", &["In Christ Alone"])];
/// "In Christ Alone" stored with one artist.
pub const ONE_ARTIST: FakeFields<'static> =
	&[("ARTIST", &["Keith & Kristyn Getty"]), ("TITLE", &["In Christ Alone"])];
pub const NO_FIELDS: FakeFields<'static> = &[];

/// A menu entry's name and flags, as the test handles show them.
pub fn entry(name: &str, flags: u32) -> (String, u32) {
	(name.to_owned(), flags)
}

/// A fake foobar2000, installed as the host of this test process.
pub fn installed_api() -> &'static FakeApi {
	let api = FakeApi::new();
	api.install();
	api
}

/// A fake foobar2000 and a [`FakeBackend`], both installed for this test process.
pub fn installed_api_and_backend() -> (&'static FakeApi, FakeBackend) {
	(installed_api(), installed_backend())
}

pub fn track_info(id: SpotifyId, title: &str) -> TrackInfo {
	TrackInfo {
		id: ItemId::Track(id),
		title: title.to_owned(),
		artists: vec!["Keith Getty".to_owned(), "Kristyn Getty".to_owned()],
		album: "In Christ Alone".to_owned(),
		album_artists: vec!["Keith & Kristyn Getty".to_owned()],
		track_number: 4,
		disc_number: 1,
		total_tracks: 11,
		total_discs: 1,
		date: Some("2007-01-02".to_owned()),
		duration_ms: 279_000,
		covers: Vec::new(),
		has_audio: true,
		unavailable: None,
		replay_gain: Some(ReplayGain {
			track_gain_db: -6.62,
			track_peak: 1.0127,
			album_gain_db: -8.51,
			album_peak: 1.0724,
		}),
		genres: vec!["Worship".to_owned(), "Gospel".to_owned()],
		description: None,
		external_url: None,
		isrc: Some("USY5K0600040".to_owned()),
		explicit: false,
		composers: Vec::new(),
		conductors: Vec::new(),
		orchestras: Vec::new(),
		remixers: Vec::new(),
		label: Some("Martingale Records".to_owned()),
		copyrights: vec![
			"© 2007 Martingale Records".to_owned(),
			"℗ 2007 Martingale Records".to_owned(),
		],
		barcode: Some("804879057321".to_owned()),
		release_type: Some("album"),
		missing_album_details: false,
		links: ItemLinks {
			album: Some(SpotifyId::from_base62(ALBUM_ID).unwrap()),
			artists: [KEITH_ID, KRISTYN_ID].map(|id| SpotifyId::from_base62(id).unwrap()).to_vec(),
			show: None,
		},
	}
}

/// An episode Spotify hosts.
pub fn episode_info(id: SpotifyId, title: &str) -> TrackInfo {
	TrackInfo {
		id: ItemId::Episode(id),
		title: title.to_owned(),
		artists: vec!["Sing! Podcast".to_owned()],
		album: "Sing! Podcast".to_owned(),
		album_artists: vec!["Sing! Podcast".to_owned()],
		track_number: 12,
		disc_number: 0,
		total_tracks: 0,
		total_discs: 0,
		date: Some("2026-10-03".to_owned()),
		duration_ms: 1_800_000,
		covers: Vec::new(),
		has_audio: true,
		unavailable: None,
		replay_gain: None,
		genres: Vec::new(),
		description: Some("How the Psalms shaped the songs of the church.".to_owned()),
		external_url: None,
		isrc: None,
		explicit: false,
		composers: Vec::new(),
		conductors: Vec::new(),
		orchestras: Vec::new(),
		remixers: Vec::new(),
		label: None,
		copyrights: Vec::new(),
		barcode: None,
		release_type: None,
		missing_album_details: false,
		links: ItemLinks::default(),
	}
}

#[derive(Default)]
struct State {
	calls: AtomicUsize,
	open_calls: AtomicUsize,
	failure: Mutex<Option<fn() -> CoreError>>,
	/// While set, reading the stored login fails as if Spotify's core couldn't start.
	start_failure: Mutex<Option<fn() -> CoreError>>,
	resolved: Mutex<Option<Resolved>>,
	/// The choice the last resolve was asked with.
	artist_tracks: Mutex<Option<ArtistTracks>>,
	cover: Mutex<Option<Vec<u8>>>,
	/// While set, reads and seeks block, like a Spotify download that has stalled.
	stalled: AtomicBool,
	logins: Mutex<Vec<ScriptedLogin>>,
	username: Mutex<Option<String>>,
	logouts: AtomicUsize,
	/// While set, Spotify calls fail with `NotLoggedIn` until a login is stored.
	require_login: AtomicBool,
	/// Runs once, during the next read of the stored login, after the value was read.
	during_username_read: Mutex<Option<Box<dyn FnOnce() + Send>>>,
	library: Mutex<Option<LibraryState>>,
	library_subscribers: Subscribers,
	library_calls: AtomicUsize,
	changes: Mutex<Vec<RecordedChange>>,
	change_result: Mutex<Option<ChangeResult>>,
	related_calls: Mutex<Vec<(ItemId, Related)>>,
	related_result: Mutex<Option<RelatedResult>>,
}

/// What a change ends with.
pub type ChangeResult = fn() -> Result<Change, CoreError>;

/// A change that was asked for, and the thread it ran and reported on.
pub type RecordedChange = (Edit, Target, Vec<ItemId>, std::thread::ThreadId);

/// What a lookup of an item ends with.
pub type RelatedResult = fn(ItemId, &Related) -> Result<Found, CoreError>;

/// "In Christ Alone Radio", the radio Spotify makes from "In Christ Alone".
const RADIO_ID: &str = "37i9dQZF1E8O9hzsgLi5jr";

/// What a lookup ends with unless [`FakeBackend::related_with`] says otherwise: the album, artists
/// or show of the info [`Backend::track_info`] gives, and "In Christ Alone Radio" for a track.
fn related_by_default(item: ItemId, what: &Related) -> Result<Found, CoreError> {
	match (item, what) {
		(ItemId::Track(_), Related::Radio) => {
			let radio = SpotifyId::from_base62(RADIO_ID).unwrap();
			Ok(Found::One(Opened::new("In Christ Alone Radio", LinkKind::Playlist, radio)))
		}
		(ItemId::Track(id), what) => related_in(&track_info(id, "In Christ Alone"), what),
		(ItemId::Episode(id), what) => related_in(&episode_info(id, "Singing the Psalms"), what),
	}
}

/// The artists of "In Christ Alone", in the track's order, as a lookup of every artist of
/// [`GETTY_TRACK`] finds them by default.
pub fn getty_artists() -> Vec<Opened> {
	match related_by_default(getty_track(), &Related::Artist(None)) {
		Ok(Found::Artists(artists)) => artists,
		other => panic!("the Getty track has several artists: {other:?}"),
	}
}

/// A login the test answers itself, with [`FakeBackend::send_code`] and
/// [`FakeBackend::finish_login`].
struct ScriptedLogin {
	on_code: Option<OnCode>,
	on_done: Option<OnDone>,
	cancelled: Arc<AtomicBool>,
	/// Set once the login is stored; a cancel after that comes too late.
	stored: Arc<AtomicBool>,
}

struct ScriptedTask {
	cancelled: Arc<AtomicBool>,
	stored: Arc<AtomicBool>,
}

impl LoginTask for ScriptedTask {
	fn cancel(&self) -> bool {
		if self.stored.load(Ordering::SeqCst) {
			return false;
		}
		self.cancelled.store(true, Ordering::SeqCst);
		true
	}
}

/// Clones share their state, so a test can install one and inspect another.
#[derive(Clone, Default)]
pub struct FakeBackend(Arc<State>);

impl FakeBackend {
	/// Every call fails with `error()` from now on.
	pub fn fail_with(&self, error: fn() -> CoreError) {
		*self.0.failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
	}

	/// Makes reading the stored login fail with `error`, as when the core can't start.
	pub fn fail_start(&self, error: fn() -> CoreError) {
		*self.0.start_failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
	}

	pub fn resolve_to(&self, resolved: Resolved) {
		*self.0.resolved.lock().unwrap_or_else(PoisonError::into_inner) = Some(resolved);
	}

	/// The choice the last resolve was asked with.
	pub fn last_artist_tracks(&self) -> Option<ArtistTracks> {
		*self.0.artist_tracks.lock().unwrap_or_else(PoisonError::into_inner)
	}

	pub fn set_cover(&self, bytes: Vec<u8>) {
		*self.0.cover.lock().unwrap_or_else(PoisonError::into_inner) = Some(bytes);
	}

	/// How many backend calls there have been.
	pub fn calls(&self) -> usize {
		self.0.calls.load(Ordering::SeqCst)
	}

	pub fn open_calls(&self) -> usize {
		self.0.open_calls.load(Ordering::SeqCst)
	}

	/// Makes reads and seeks of opened tracks block until `stall(false)`, at most 5 s.
	pub fn stall(&self, stalled: bool) {
		self.0.stalled.store(stalled, Ordering::SeqCst);
	}

	/// Spotify calls fail with `NotLoggedIn` until a login is stored, as for a new user.
	pub fn require_login(&self) {
		self.0.require_login.store(true, Ordering::SeqCst);
	}

	/// Runs `action` once, during the next read of the stored login, as another thread could.
	pub fn during_username_read(&self, action: impl FnOnce() + Send + 'static) {
		*self.0.during_username_read.lock().unwrap_or_else(PoisonError::into_inner) =
			Some(Box::new(action));
	}

	/// The stored login's username, as if a login had happened before.
	pub fn set_username(&self, username: Option<&str>) {
		*self.0.username.lock().unwrap_or_else(PoisonError::into_inner) =
			username.map(str::to_owned);
	}

	/// What `library` and `held_library` answer from now on; `LoggedOut` until set.
	pub fn set_library(&self, state: LibraryState) {
		*self.0.library.lock().unwrap_or_else(PoisonError::into_inner) = Some(state);
	}

	/// [`Self::set_library`], then tells the library's subscribers.
	pub fn publish_library(&self, state: LibraryState) {
		self.set_library(state);
		self.0.library_subscribers.notify();
	}

	/// How many subscribe to the library.
	pub fn library_subscribers(&self) -> usize {
		self.0.library_subscribers.count()
	}

	/// How many times `library` was called; `held_library` isn't counted.
	pub fn library_calls(&self) -> usize {
		self.0.library_calls.load(Ordering::SeqCst)
	}

	/// The changes asked for so far, each with the thread it ran and reported on.
	pub fn changes(&self) -> Vec<RecordedChange> {
		self.0.changes.lock().unwrap_or_else(PoisonError::into_inner).clone()
	}

	/// Makes later changes end with `result()`; by default every item counts as changed.
	pub fn change_with(&self, result: ChangeResult) {
		*self.0.change_result.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
	}

	/// Makes later lookups end with `result(item, what)`.
	pub fn related_with(&self, result: RelatedResult) {
		*self.0.related_result.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
	}

	/// The lookups asked for so far, in order.
	pub fn related_calls(&self) -> Vec<(ItemId, Related)> {
		self.0.related_calls.lock().unwrap_or_else(PoisonError::into_inner).clone()
	}

	fn library_state(&self) -> LibraryState {
		self.0
			.library
			.lock()
			.unwrap_or_else(PoisonError::into_inner)
			.clone()
			.unwrap_or(LibraryState::LoggedOut)
	}

	pub fn logins_started(&self) -> usize {
		self.0.logins.lock().unwrap_or_else(PoisonError::into_inner).len()
	}

	pub fn last_login_cancelled(&self) -> bool {
		self.0
			.logins
			.lock()
			.unwrap_or_else(PoisonError::into_inner)
			.last()
			.is_some_and(|login| login.cancelled.load(Ordering::SeqCst))
	}

	pub fn logouts(&self) -> usize {
		self.0.logouts.load(Ordering::SeqCst)
	}

	/// The latest login receives its code from Spotify. Unlike the real one, the fake answers
	/// even after it was cancelled, so tests can check that late answers are ignored.
	pub fn send_code(&self, code: DeviceCode) {
		let on_code = self.latest(|login| login.on_code.take());
		if let Some(on_code) = on_code {
			on_code(code);
		}
	}

	/// The latest login ends. Success stores the login, as the real one does, unless it was
	/// cancelled.
	pub fn finish_login(&self, result: Result<String, CoreError>) {
		let (on_done, cancelled) =
			self.latest(|login| (login.on_done.take(), login.cancelled.load(Ordering::SeqCst)));
		if let (Ok(username), false) = (&result, cancelled) {
			self.set_username(Some(username));
		}
		if let Some(on_done) = on_done {
			on_done(result);
		}
	}

	/// The latest login stores its login and is about to answer, but hasn't yet: the moment a
	/// cancel comes too late.
	pub fn store_login_before_answering(&self, username: &str) {
		self.latest(|login| login.stored.store(true, Ordering::SeqCst));
		self.set_username(Some(username));
	}

	fn latest<R>(&self, take: impl FnOnce(&mut ScriptedLogin) -> R) -> R {
		let mut logins = self.0.logins.lock().unwrap_or_else(PoisonError::into_inner);
		take(logins.last_mut().expect("no login was started"))
	}

	fn enter(&self, is_aborting: &dyn Fn() -> bool) -> Result<(), CoreError> {
		self.0.calls.fetch_add(1, Ordering::SeqCst);
		if is_aborting() {
			return Err(CoreError::Aborted);
		}
		if self.0.require_login.load(Ordering::SeqCst) && self.username()?.is_none() {
			return Err(CoreError::NotLoggedIn);
		}
		let failure = *self.0.failure.lock().unwrap_or_else(PoisonError::into_inner);
		failure.map_or(Ok(()), |error| Err(error()))
	}
}

impl Backend for FakeBackend {
	fn resolve(
		&self,
		_link: &Link,
		artist_tracks: ArtistTracks,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Resolved, CoreError> {
		*self.0.artist_tracks.lock().unwrap_or_else(PoisonError::into_inner) = Some(artist_tracks);
		self.enter(is_aborting)?;
		Ok(self
			.0
			.resolved
			.lock()
			.unwrap_or_else(PoisonError::into_inner)
			.clone()
			.expect("resolve_to"))
	}

	fn track_info(
		&self,
		id: ItemId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Arc<TrackInfo>, CoreError> {
		self.enter(is_aborting)?;
		Ok(Arc::new(match id {
			ItemId::Track(id) => track_info(id, "In Christ Alone"),
			ItemId::Episode(id) => episode_info(id, "Singing the Psalms"),
		}))
	}

	fn open_track(
		&self,
		_id: ItemId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Box<dyn PcmTrack>, CoreError> {
		self.0.open_calls.fetch_add(1, Ordering::SeqCst);
		self.enter(is_aborting)?;
		let decoder = PcmDecoder::new(Box::new(Cursor::new(CHIRP)), Some("audio/ogg"))?;
		Ok(Box::new(FixtureTrack { decoder, state: Arc::clone(&self.0) }))
	}

	fn cover(
		&self,
		_id: ItemId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Option<Vec<u8>>, CoreError> {
		self.enter(is_aborting)?;
		Ok(self.0.cover.lock().unwrap_or_else(PoisonError::into_inner).clone())
	}

	fn start_login(
		&self,
		on_code: OnCode,
		on_done: OnDone,
	) -> Result<Box<dyn LoginTask>, CoreError> {
		let cancelled = Arc::new(AtomicBool::new(false));
		let stored = Arc::new(AtomicBool::new(false));
		self.0.logins.lock().unwrap_or_else(PoisonError::into_inner).push(ScriptedLogin {
			on_code: Some(on_code),
			on_done: Some(on_done),
			cancelled: Arc::clone(&cancelled),
			stored: Arc::clone(&stored),
		});
		Ok(Box::new(ScriptedTask { cancelled, stored }))
	}

	fn logout(&self) -> Result<(), CoreError> {
		self.0.logouts.fetch_add(1, Ordering::SeqCst);
		self.set_username(None);
		Ok(())
	}

	fn username(&self) -> Result<Option<String>, CoreError> {
		let start_failure = *self.0.start_failure.lock().unwrap_or_else(PoisonError::into_inner);
		if let Some(error) = start_failure {
			return Err(error());
		}
		let username = self.0.username.lock().unwrap_or_else(PoisonError::into_inner).clone();
		let during =
			self.0.during_username_read.lock().unwrap_or_else(PoisonError::into_inner).take();
		if let Some(action) = during {
			action();
		}
		Ok(username)
	}

	fn library(&self) -> LibraryState {
		self.0.library_calls.fetch_add(1, Ordering::SeqCst);
		self.library_state()
	}

	fn held_library(&self) -> LibraryState {
		self.library_state()
	}

	fn subscribe_library(&self, notify: Box<dyn Fn() + Send + Sync>) -> u64 {
		self.0.library_subscribers.subscribe(notify)
	}

	fn unsubscribe_library(&self, id: u64) {
		self.0.library_subscribers.unsubscribe(id);
	}

	/// Runs the change and `on_done` on a thread of its own, which starts once the change is
	/// recorded.
	fn change(&self, edit: Edit, target: Target, items: &[ItemId], on_done: OnChanged) {
		let (fake, count) = (self.clone(), items.len());
		let (recorded, wait) = std::sync::mpsc::channel::<()>();
		let thread = std::thread::spawn(move || {
			let _ = wait.recv();
			let result = fake.enter(&|| false).and_then(|()| {
				let result = *fake.0.change_result.lock().unwrap_or_else(PoisonError::into_inner);
				result.map_or_else(
					|| Ok(Change { changed: count, ..Change::default() }),
					|result| result(),
				)
			});
			on_done(result);
		});
		self.0.changes.lock().unwrap_or_else(PoisonError::into_inner).push((
			edit,
			target,
			items.to_vec(),
			thread.thread().id(),
		));
		let _ = recorded.send(());
	}

	/// Records the lookup, then runs it and `on_done` on a thread of its own.
	fn related(&self, item: ItemId, what: Related, on_done: OnRelated) {
		self.0
			.related_calls
			.lock()
			.unwrap_or_else(PoisonError::into_inner)
			.push((item, what.clone()));
		let fake = self.clone();
		std::thread::spawn(move || {
			let result = fake.enter(&|| false).and_then(|()| {
				let result = *fake.0.related_result.lock().unwrap_or_else(PoisonError::into_inner);
				result.unwrap_or(related_by_default)(item, &what)
			});
			on_done(result);
		});
	}
}

/// The fixture, decoded.
struct FixtureTrack {
	decoder: PcmDecoder,
	state: Arc<State>,
}

impl FixtureTrack {
	fn wait_while_stalled(&self) {
		let started = std::time::Instant::now();
		while self.state.stalled.load(Ordering::SeqCst)
			&& started.elapsed() < std::time::Duration::from_secs(5)
		{
			std::thread::sleep(std::time::Duration::from_millis(10));
		}
	}
}

impl PcmTrack for FixtureTrack {
	fn read(&mut self, out: &mut Vec<f64>, max_frames: usize) -> Result<usize, CoreError> {
		self.wait_while_stalled();
		self.decoder.read(out, max_frames)
	}

	fn seek(&mut self, frame: u64) -> Result<(), CoreError> {
		self.wait_while_stalled();
		self.decoder.seek(frame)
	}

	/// Not the preferred 320 kbit/s, so tests see the file's own bitrate.
	fn file_format(&self) -> FileFormat {
		FileFormat { codec: "Vorbis", kbps: Some(160) }
	}
}

/// The whole fixture, decoded straight through.
pub fn fixture_samples() -> Vec<f64> {
	let mut decoder = PcmDecoder::new(Box::new(Cursor::new(CHIRP)), Some("audio/ogg")).unwrap();
	let (mut all, mut buffer) = (Vec::new(), Vec::new());
	while decoder.read(&mut buffer, 4096).unwrap() > 0 {
		all.extend_from_slice(&buffer);
	}
	all
}

pub fn sunday_set_id() -> SpotifyId {
	SpotifyId::from_raw(&[0x51; 16]).unwrap()
}

pub fn getty_favourites_id() -> SpotifyId {
	SpotifyId::from_raw(&[0x47; 16]).unwrap()
}

/// "Sunday set", in the folder "Church", which the account can change.
pub fn sunday_set() -> LibraryPlaylist {
	LibraryPlaylist {
		id: sunday_set_id(),
		name: "Sunday set".to_owned(),
		folders: vec!["Church".to_owned()],
		can_edit: true,
		revision: b"r1".to_vec(),
	}
}

/// "Getty favourites", a playlist the account follows.
pub fn getty_favourites() -> LibraryPlaylist {
	LibraryPlaylist {
		id: getty_favourites_id(),
		name: "Getty favourites".to_owned(),
		folders: Vec::new(),
		can_edit: false,
		revision: b"r1".to_vec(),
	}
}

/// "Shared", a collaborative playlist the account can change.
pub fn shared_playlist() -> LibraryPlaylist {
	LibraryPlaylist {
		id: SpotifyId::from_raw(&[0x52; 16]).unwrap(),
		name: "Shared".to_owned(),
		folders: Vec::new(),
		can_edit: true,
		revision: b"r1".to_vec(),
	}
}

/// Polls every 5 ms until `condition` holds; panics after 5 s.
pub fn wait_until(condition: impl Fn() -> bool) {
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
	while !condition() {
		assert!(std::time::Instant::now() < deadline, "the condition did not hold within 5 s");
		std::thread::sleep(std::time::Duration::from_millis(5));
	}
}

/// A complete library of `playlists`: the ones the account can change are empty, and Liked
/// Songs is empty too.
pub fn library_with(playlists: Vec<LibraryPlaylist>) -> Arc<Library> {
	let contents = playlists
		.iter()
		.filter(|playlist| playlist.can_edit)
		.map(|playlist| {
			(
				playlist.id,
				PlaylistContents {
					revision: playlist.revision.clone(),
					rows: Vec::new(),
					truncated: false,
				},
			)
		})
		.collect();
	Arc::new(Library::from_parts(playlists, contents, Some(std::collections::HashSet::new())))
}

/// A login prompt that only counts how often it was shown.
#[derive(Clone, Default)]
pub struct FakePrompt(Arc<AtomicUsize>);

impl FakePrompt {
	pub fn shown(&self) -> usize {
		self.0.load(Ordering::SeqCst)
	}
}

impl crate::first_use::LoginPrompt for FakePrompt {
	fn show(&self) {
		self.0.fetch_add(1, Ordering::SeqCst);
	}
}

/// A [`FakeBackend`], installed as the backend of this test process.
pub fn installed_backend() -> FakeBackend {
	let fake = FakeBackend::default();
	crate::backend::install(Box::new(fake.clone()));
	fake
}

/// Installs a [`FakePrompt`] for this test process.
pub fn installed_prompt() -> FakePrompt {
	let prompt = FakePrompt::default();
	crate::first_use::install_prompt(Box::new(prompt.clone()));
	prompt
}

/// A login code from Spotify.
pub fn device_code() -> DeviceCode {
	DeviceCode {
		user_code: "KKGT-2024".to_owned(),
		verification_uri: "https://spotify.com/pair".to_owned(),
		url: "https://spotify.com/pair?code=KKGT-2024".to_owned(),
	}
}

/// Runs `action` on another thread once `logins` logins have started, as the user or Spotify
/// would answer them.
fn once_logins_started(
	fake: &FakeBackend,
	logins: usize,
	action: impl FnOnce(&FakeBackend) + Send + 'static,
) -> std::thread::JoinHandle<()> {
	let fake = fake.clone();
	std::thread::spawn(move || {
		wait_until(|| fake.logins_started() >= logins);
		action(&fake);
	})
}

/// Plays the user: once `logins` logins have started, confirms the latest in the browser.
pub fn confirm_login_when_started(
	fake: &FakeBackend,
	logins: usize,
) -> std::thread::JoinHandle<()> {
	once_logins_started(fake, logins, |fake| {
		fake.send_code(device_code());
		fake.finish_login(Ok("Keith Getty".to_owned()));
	})
}

/// Plays Spotify: once a login has started, fails it, as a login declined in the browser does.
pub fn fail_login_when_started(fake: &FakeBackend) -> std::thread::JoinHandle<()> {
	once_logins_started(fake, 1, |fake| {
		let declined = foo_librespot_core::login::LoginError::Failed("declined".to_owned());
		fake.finish_login(Err(CoreError::Login(declined)));
	})
}

/// Plays the user: once a login has started, cancels it, as Close in the login window does.
pub fn cancel_login_when_started(fake: &FakeBackend) -> std::thread::JoinHandle<()> {
	once_logins_started(fake, 1, |_| crate::login::controller().cancel())
}

/// Lets another thread press Stop on a `FakeAbort` the test keeps alive.
struct StopLater(*const foxide2k::testing::FakeAbort);

// SAFETY: every test joins the thread before its FakeAbort is dropped.
unsafe impl Send for StopLater {}

/// Presses Stop on `abort` after `delay`, from another thread. Join the handle before `abort`
/// goes out of scope.
pub fn stop_after(
	abort: &foxide2k::testing::FakeAbort,
	delay: std::time::Duration,
) -> std::thread::JoinHandle<()> {
	let target = StopLater(abort);
	std::thread::spawn(move || {
		let target = target;
		std::thread::sleep(delay);
		unsafe { &*target.0 }.set_aborting(true);
	})
}

/// Holds off the other tests that change what the whole test process shares: the log level, the
/// detailed-logging switch and the installed fake foobar2000. `cargo test` runs tests side by
/// side on threads; nextest runs each in its own process, where this costs nothing.
pub fn lock_shared_state() -> std::sync::MutexGuard<'static, ()> {
	static SHARED: Mutex<()> = Mutex::new(());
	SHARED.lock().unwrap_or_else(PoisonError::into_inner)
}
