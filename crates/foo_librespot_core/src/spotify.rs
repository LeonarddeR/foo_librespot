//! `Core`: the one entry point the foobar2000 plugin uses.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use librespot_core::Session;

use crate::art::{CoverCache, fetch_cover};
use crate::audio::format::Bitrate;
use crate::audio::keys::AudioKeys;
use crate::audio::track::{
	FetchedTrack, KeyOrigin, SpotifyTrack, fetch_track, wrong_remembered_key,
};
use crate::library::{Edit, LibraryCache, LibraryState, LibrespotLibrary, OnChanged, Target};
use crate::link::{ItemId, Link, LinkKind};
use crate::login::{DeviceCode, LibrespotAuthorizer, device_login};
use crate::metadata::{TrackInfo, largest_cover};
use crate::profile::Profile;
use crate::related::{Found, NOT_A_TRACK, Opened, Related, radio_playlist, related_in};
use crate::resolve::{
	AlbumDetailsCache, ArtistTracks, CachedSource, LibrespotMetadata, MetadataSource, Resolved,
	TrackCache, item, resolve, retry_rate_limited,
};
use crate::runtime::CoreRuntime;
use crate::session::{LibrespotConnector, SessionManager};
use crate::{CoreError, SpotifyUri};

/// How long [`Core::shutdown`] gives running Spotify calls to finish.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// Spotify for foo_librespot: login, link resolution, metadata, album art and audio.
///
/// Every call blocks the calling thread and returns [`CoreError::Aborted`] soon after
/// `is_aborting` turns true. Never call it from async code.
pub struct Core {
	runtime: CoreRuntime,
	profile: Arc<Profile>,
	sessions: Arc<SessionManager<LibrespotConnector>>,
	cache: TrackCache,
	albums: AlbumDetailsCache,
	covers: CoverCache,
	keys: AudioKeys,
	library: LibraryCache,
}

impl Core {
	/// Uses `profile_dir` for the device id and the encrypted login, creating it if needed.
	pub fn new(profile_dir: impl Into<PathBuf>) -> Result<Self, CoreError> {
		let profile = Arc::new(Profile::new(profile_dir));
		let device_id = profile.device_id()?;
		let keys = profile.load_audio_keys().unwrap_or_else(|error| {
			// Without them, Spotify is asked for every key again.
			tracing::warn!("{error}; Spotify will be asked for every key again");
			AudioKeys::default()
		});
		let sessions =
			SessionManager::new(LibrespotConnector::new(device_id), Arc::clone(&profile));
		Ok(Self {
			runtime: CoreRuntime::new()?,
			profile,
			sessions: Arc::new(sessions),
			cache: TrackCache::default(),
			albums: AlbumDetailsCache::default(),
			covers: CoverCache::default(),
			keys,
			library: LibraryCache::default(),
		})
	}

	pub fn is_logged_in(&self) -> bool {
		self.sessions.is_logged_in()
	}

	/// The Spotify username in the stored login, or `None` when logged out.
	pub fn username(&self) -> Option<String> {
		self.sessions.username()
	}

	/// Starts a browser login in the background. `on_code` gets the code and page to show the
	/// user; `on_done` gets the Spotify username or the reason it failed. Both run on a runtime
	/// thread. When [`LoginHandle::cancel`] returns true, neither is called afterwards and no login
	/// is stored; when it returns false, the login had already finished and `on_done` gets its
	/// result.
	pub fn start_login(
		&self,
		on_code: impl FnOnce(&DeviceCode) + Send + 'static,
		on_done: impl FnOnce(Result<String, CoreError>) + Send + 'static,
	) -> LoginHandle {
		let sessions = Arc::clone(&self.sessions);
		let cache = self.cache.clone();
		let library = self.library.clone();
		let gate = LoginGate::default();
		let finishing = gate.clone();
		let task = self.runtime.spawn(async move {
			let result = async {
				let authorizer = LibrespotAuthorizer::new()?;
				let token = device_login(&authorizer, on_code).await?;
				let username = sessions.login_with_token(token).await?;
				// Forgets what was read for the account logged in before.
				cache.clear();
				library.clear();
				Ok(username)
			}
			.await;
			if finishing.finish() {
				on_done(result);
			} else if result.is_ok() {
				// Cancelled as the login was being stored: the stored login is undone.
				if let Err(error) = sessions.logout() {
					tracing::warn!("couldn't undo a cancelled Spotify login: {error}");
				}
			}
		});
		LoginHandle { task, gate }
	}

	/// Forgets the login, the account's library and the remembered audio keys, which show what
	/// was played.
	pub fn logout(&self) -> Result<(), CoreError> {
		let result = self.sessions.logout();
		self.library.clear();
		result?;
		self.keys.clear();
		self.profile.save_audio_keys(&self.keys)?;
		Ok(())
	}

	/// The account's playlists and Liked Songs as last read, without waiting for Spotify. When
	/// they are missing or old, a read starts in the background, and the
	/// [subscribers](Self::subscribe_library) hear when it publishes.
	pub fn library(&self) -> LibraryState {
		if !self.is_logged_in() {
			return LibraryState::LoggedOut;
		}
		let (state, fetch) = self.library.snapshot(Instant::now());
		if let Some(fetch) = fetch {
			let sessions = Arc::clone(&self.sessions);
			self.runtime.spawn(
				fetch.run(async move { sessions.session().await.map(LibrespotLibrary::new) }),
			);
		}
		state
	}

	/// What [`Core::library`] would answer, without starting a read.
	pub fn held_library(&self) -> LibraryState {
		if self.is_logged_in() { self.library.held() } else { LibraryState::LoggedOut }
	}

	/// Calls `notify` after each publish and failure of a library read and after each
	/// [`change`](Self::change) that changed something, on a runtime thread, and after each logout
	/// and login, on the thread that makes it. Returns the id to
	/// [`unsubscribe_library`](Self::unsubscribe_library) with.
	pub fn subscribe_library(&self, notify: Box<dyn Fn() + Send + Sync>) -> u64 {
		self.library.subscribe(notify)
	}

	pub fn unsubscribe_library(&self, id: u64) {
		self.library.unsubscribe(id);
	}

	pub fn resolve(
		&self,
		link: &Link,
		artist_tracks: ArtistTracks,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Resolved, CoreError> {
		self.runtime.block_on(
			async {
				let session = self.sessions.session().await?;
				let source = CachedSource::new(self.metadata(session), self.cache.clone());
				resolve(&source, link, artist_tracks).await
			},
			is_aborting,
		)?
	}

	pub fn track_info(
		&self,
		id: ItemId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Arc<TrackInfo>, CoreError> {
		if let Some(info) = self.cache.get(&id) {
			return Ok(info);
		}
		self.runtime.block_on(
			async {
				let session = self.sessions.session().await?;
				let source = CachedSource::new(self.metadata(session), self.cache.clone());
				item(&source, id).await
			},
			is_aborting,
		)?
	}

	/// The album, artist or show `item` belongs to, from its metadata, or the radio playlist
	/// Spotify makes from a track, named as Spotify names it.
	pub fn related(
		&self,
		item: ItemId,
		what: &Related,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Found, CoreError> {
		if *what != Related::Radio {
			return related_in(&*self.track_info(item, is_aborting)?, what);
		}
		let ItemId::Track(id) = item else {
			return Err(CoreError::Unavailable(NOT_A_TRACK.to_owned()));
		};
		let seed = SpotifyUri::Track { id };
		self.runtime.block_on(
			async {
				let session = self.sessions.session().await?;
				let answer =
					retry_rate_limited(|| session.spclient().get_radio_for_track(&seed)).await?;
				let id = radio_playlist(&answer)?;
				let (name, _) = self.metadata(session).playlist(id).await?;
				Ok(Found::One(Opened::new(name, LinkKind::Playlist, id)))
			},
			is_aborting,
		)?
	}

	/// Spotify's metadata, read through `session`, with the album details this core keeps.
	fn metadata(&self, session: Session) -> LibrespotMetadata {
		LibrespotMetadata::new(session, self.albums.clone())
	}

	/// Opens a track or episode for decoding. The download starts in the background; reading
	/// blocks the caller until the data it needs has arrived. A track played before uses its
	/// remembered key instead of asking Spotify.
	pub fn open_track(
		&self,
		id: ItemId,
		bitrate: Bitrate,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<SpotifyTrack, CoreError> {
		let fetched = self.fetch_track(id, bitrate, is_aborting)?;
		let (file_id, key_origin) = (fetched.file_id(), fetched.key_origin());
		// The first reads, which block until data arrives, run where Stop can interrupt them.
		match self.runtime.block_on_blocking(move || fetched.open(), is_aborting)? {
			Err(error) if wrong_remembered_key(key_origin, &error) => {
				tracing::warn!(
					"the remembered key didn't decrypt the track ({error}); asking Spotify"
				);
				self.keys.forget(&file_id);
				let fetched = self.fetch_track(id, bitrate, is_aborting)?;
				self.runtime.block_on_blocking(move || fetched.open(), is_aborting)?
			}
			opened => opened,
		}
	}

	fn fetch_track(
		&self,
		id: ItemId,
		bitrate: Bitrate,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<FetchedTrack, CoreError> {
		let fetched = self.runtime.block_on(
			async {
				let session = self.sessions.session().await?;
				fetch_track(&session, id, bitrate, &self.keys).await
			},
			is_aborting,
		)??;
		if fetched.key_origin() == KeyOrigin::Requested {
			self.save_keys_in_background();
		}
		Ok(fetched)
	}

	/// Saves the remembered keys on the runtime's blocking pool, without waiting for the disk.
	fn save_keys_in_background(&self) {
		let (profile, keys) = (Arc::clone(&self.profile), self.keys.clone());
		self.runtime.handle().spawn_blocking(move || {
			if let Err(error) = profile.save_audio_keys(&keys) {
				tracing::warn!("couldn't save the remembered audio keys: {error}");
			}
		});
	}

	/// The largest album cover of a track, or cover of an episode, as JPEG bytes; `None` when it
	/// has none. Each image is downloaded once and kept in memory, up to
	/// [`COVER_CACHE_LIMIT`](crate::art::COVER_CACHE_LIMIT) bytes.
	pub fn cover(
		&self,
		id: ItemId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Option<Vec<u8>>, CoreError> {
		let info = self.track_info(id, is_aborting)?;
		let Some(cover) = largest_cover(&info.covers).cloned() else {
			return Ok(None);
		};
		let jpeg = self.runtime.block_on(
			self.covers.get_or_fetch(cover.file_id, move || async move {
				let session = self.sessions.session().await?;
				fetch_cover(&session, &cover).await
			}),
			is_aborting,
		)??;
		Ok(Some(jpeg.to_vec()))
	}

	/// Starts adding `items` to a playlist or Liked Songs, or removing them, every copy of each,
	/// once every change begun before has ended, and returns at once. Playlists are read first,
	/// so an add sends only what they lack. When the change lands and changed something, library
	/// subscribers hear it, on a runtime thread, before `on_done` runs. `on_done` gets what was
	/// done, on a runtime thread; it fails when Spotify hasn't answered within a minute of the
	/// change's turn. A change still running at [`shutdown`](Self::shutdown) is dropped, and
	/// `on_done` with it.
	pub fn change(&self, edit: Edit, target: Target, items: &[ItemId], on_done: OnChanged) {
		let (library, sessions) = (self.library.clone(), Arc::clone(&self.sessions));
		let items = items.to_vec();
		self.runtime.spawn(async move {
			let result = library
				.in_turn(async {
					let session = sessions.session().await?;
					library.change(&LibrespotLibrary::new(session), edit, target, &items).await
				})
				.await;
			on_done(result);
		});
	}

	/// Stops the runtime; foobar2000 calls this when it quits.
	pub fn shutdown(self) {
		self.runtime.shutdown(SHUTDOWN_TIMEOUT);
	}
}

/// A login running in the background.
pub struct LoginHandle {
	task: tokio::task::JoinHandle<()>,
	gate: LoginGate,
}

impl LoginHandle {
	/// Stops the login. True when it was stopped in time: nothing is stored and nothing is
	/// reported. False when it had already finished; its result is reported as usual.
	pub fn cancel(&self) -> bool {
		let in_time = self.gate.cancel();
		if in_time {
			self.task.abort();
		}
		in_time
	}

	pub fn is_finished(&self) -> bool {
		self.task.is_finished()
	}
}

/// Decides whether a login that is finishing or a cancel came first, also for a cancel in the
/// last moments of a login, while the login is stored and the task has no await left to stop
/// at.
#[derive(Clone, Default)]
struct LoginGate(Arc<std::sync::Mutex<Option<Outcome>>>);

#[derive(Clone, Copy)]
enum Outcome {
	Finished,
	Cancelled,
}

impl LoginGate {
	/// Called once the login has ended. True when it may report its result; false when a cancel
	/// came first, so a stored login must be undone.
	fn finish(&self) -> bool {
		self.settle(Outcome::Finished)
	}

	/// True when the cancel came before the login finished.
	fn cancel(&self) -> bool {
		self.settle(Outcome::Cancelled)
	}

	fn settle(&self, outcome: Outcome) -> bool {
		let mut settled = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
		if settled.is_some() {
			return false;
		}
		*settled = Some(outcome);
		true
	}
}

#[cfg(test)]
mod tests {
	use librespot_core::audio_key::AudioKey;

	use super::{Core, LoginGate, LoginHandle};
	use crate::audio::keys::AudioKeys;
	use crate::audio::track::SpotifyTrack;
	use crate::library::{Edit, LibraryState, Target};
	use crate::link::{ItemId, Link, LinkKind};
	use crate::profile::Profile;
	use crate::related::Related;
	use crate::resolve::ArtistTracks;
	use crate::{CoreError, FileId, SpotifyId};

	#[test]
	fn a_cancel_before_the_login_is_stored_wins() {
		let gate = LoginGate::default();
		assert!(gate.cancel());
		// The login, finishing after the cancel, must undo itself and not report.
		assert!(!gate.finish());
	}

	#[test]
	fn a_cancel_after_the_login_is_stored_comes_too_late() {
		let gate = LoginGate::default();
		assert!(gate.finish());
		assert!(!gate.cancel());
	}

	fn album() -> Link {
		Link::Id {
			kind: LinkKind::Album,
			id: SpotifyId::from_base62("4LH4d3cOWNNsVw41Gqt2kv").unwrap(),
		}
	}

	#[test]
	fn new_creates_the_profile_and_is_logged_out() {
		let dir = tempfile::tempdir().unwrap();
		let profile = dir.path().join("foo_librespot");
		let core = Core::new(&profile).unwrap();
		assert!(profile.join("device_id").is_file());
		assert!(!core.is_logged_in());
		core.shutdown();
	}

	#[test]
	fn logout_forgets_the_remembered_keys() {
		let dir = tempfile::tempdir().unwrap();
		let keys = AudioKeys::default();
		keys.insert(FileId::from_raw(&[1; 20]), AudioKey([2; 16]));
		Profile::new(dir.path()).save_audio_keys(&keys).unwrap();
		let core = Core::new(dir.path()).unwrap();
		core.logout().unwrap();
		assert!(!dir.path().join("audio_keys.bin").exists());
		core.shutdown();
	}

	#[test]
	fn a_garbled_key_file_doesnt_stop_the_core() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("audio_keys.bin"), b"garbage").unwrap();
		Core::new(dir.path()).unwrap().shutdown();
	}

	#[test]
	fn resolve_without_login_is_not_logged_in() {
		let dir = tempfile::tempdir().unwrap();
		let core = Core::new(dir.path()).unwrap();
		let result = core.resolve(&album(), ArtistTracks::Popular, &|| false);
		assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
		core.shutdown();
	}

	#[test]
	fn related_without_login_is_not_logged_in() {
		let dir = tempfile::tempdir().unwrap();
		let core = Core::new(dir.path()).unwrap();
		let track = ItemId::Track(SpotifyId::from_base62("2hQxLiI93xdjqENJuKeXjO").unwrap());
		for what in [Related::Album, Related::Artist, Related::Radio] {
			let result = core.related(track, &what, &|| false);
			assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{what:?}: {result:?}");
		}
		core.shutdown();
	}

	#[test]
	fn an_episode_has_no_radio_to_ask_spotify_for() {
		let dir = tempfile::tempdir().unwrap();
		let core = Core::new(dir.path()).unwrap();
		let episode = ItemId::Episode(SpotifyId::from_base62("4QomYTPgZ1mpVbUmxBsUpz").unwrap());
		let result = core.related(episode, &Related::Radio, &|| false);
		assert!(
			matches!(&result, Err(CoreError::Unavailable(reason)) if reason == "this is an episode, not a track"),
			"{result:?}"
		);
		core.shutdown();
	}

	#[test]
	fn library_without_login_is_logged_out() {
		let dir = tempfile::tempdir().unwrap();
		let core = Core::new(dir.path()).unwrap();
		assert!(matches!(core.library(), LibraryState::LoggedOut));
		assert!(matches!(core.held_library(), LibraryState::LoggedOut));
		core.shutdown();
	}

	#[test]
	fn a_change_without_login_is_not_logged_in() {
		let dir = tempfile::tempdir().unwrap();
		let core = Core::new(dir.path()).unwrap();
		let track = ItemId::Track(SpotifyId::from_base62("4LH4d3cOWNNsVw41Gqt2kv").unwrap());
		for edit in [Edit::Add, Edit::Remove] {
			let (done, result) = std::sync::mpsc::channel();
			core.change(
				edit,
				Target::LikedSongs,
				&[track],
				Box::new(move |result| {
					let _ = done.send((result, std::thread::current().id()));
				}),
			);
			let (result, thread) = result.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
			assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
			assert_ne!(thread, std::thread::current().id());
		}
		core.shutdown();
	}

	#[test]
	fn a_change_still_waiting_at_shutdown_is_dropped_with_its_callback() {
		let dir = tempfile::tempdir().unwrap();
		let core = Core::new(dir.path()).unwrap();
		let (held, turn_taken) = std::sync::mpsc::channel();
		let library = core.library.clone();
		core.runtime.spawn(async move {
			library
				.in_turn(async move {
					let _ = held.send(());
					std::future::pending::<Result<(), CoreError>>().await
				})
				.await
		});
		turn_taken.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
		let track = ItemId::Track(SpotifyId::from_base62("4LH4d3cOWNNsVw41Gqt2kv").unwrap());
		let (done, result) = std::sync::mpsc::channel();
		core.change(
			Edit::Add,
			Target::LikedSongs,
			&[track],
			Box::new(move |result| {
				let _ = done.send(result);
			}),
		);
		// Logged out, a change that ran would fail at once; this one waits for its turn.
		assert_eq!(
			result.recv_timeout(std::time::Duration::from_millis(500)).map(|_| ()),
			Err(std::sync::mpsc::RecvTimeoutError::Timeout)
		);
		core.shutdown();
		assert_eq!(
			result.recv_timeout(std::time::Duration::from_secs(5)).map(|_| ()),
			Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
		);
	}

	#[test]
	fn aborted_call_is_aborted() {
		let dir = tempfile::tempdir().unwrap();
		let core = Core::new(dir.path()).unwrap();
		let result = core.resolve(&album(), ArtistTracks::Popular, &|| true);
		assert!(matches!(result, Err(CoreError::Aborted)), "{result:?}");
		core.shutdown();
	}

	#[test]
	fn core_is_send_and_sync() {
		fn send_and_sync<T: Send + Sync>() {}
		fn send<T: Send>() {}
		send_and_sync::<Core>();
		send::<SpotifyTrack>();
		send::<LoginHandle>();
	}
}
