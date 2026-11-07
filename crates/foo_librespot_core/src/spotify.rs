//! `Core`: the one entry point the foobar2000 plugin uses.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::CoreError;
use crate::art::{CoverCache, fetch_cover};
use crate::audio::format::Bitrate;
use crate::audio::keys::AudioKeys;
use crate::audio::track::{
	FetchedTrack, KeyOrigin, SpotifyTrack, fetch_track, wrong_remembered_key,
};
use crate::library::{
	Change, LIBRARY_FETCH_TIMEOUT, LibraryCache, LibraryState, LibrespotLibrary, OnLibraryChange,
	Target,
};
use crate::link::{ItemId, Link};
use crate::login::{DeviceCode, LibrespotAuthorizer, device_login};
use crate::metadata::{TrackInfo, largest_cover};
use crate::profile::Profile;
use crate::resolve::{
	AlbumDetailsCache, ArtistTracks, CachedSource, LibrespotMetadata, Resolved, TrackCache, item,
	resolve,
};
use crate::runtime::CoreRuntime;
use crate::session::{LibrespotConnector, SessionManager};

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
			// They are only a shortcut: every key can be asked for again.
			log::warn!("{error}; Spotify will be asked for every key again");
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
				// Which tracks are available depends on the account's country.
				cache.clear();
				library.clear();
				Ok(username)
			}
			.await;
			if finishing.finish() {
				on_done(result);
			} else if result.is_ok() {
				// Cancelled as the login was being stored: what the user sees is the cancel.
				if let Err(error) = sessions.logout() {
					log::warn!("couldn't undo a cancelled Spotify login: {error}");
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
	/// they are missing or old, a read starts in the background; `on_change` runs once when it
	/// next publishes, on a runtime thread, and is dropped unrun when no read is running. A logout
	/// runs it on the thread that calls `logout`, and a login on the runtime thread that
	/// finishes it.
	pub fn library(&self, on_change: Option<OnLibraryChange>) -> LibraryState {
		if !self.is_logged_in() {
			return LibraryState::LoggedOut;
		}
		let now = Instant::now();
		let (state, ticket) = self.library.snapshot_ticket(now, on_change);
		if let Some(ticket) = ticket {
			let sessions = Arc::clone(&self.sessions);
			let library = self.library.clone();
			self.runtime.spawn(async move {
				let read = async {
					match sessions.session().await {
						Ok(session) => {
							library.fetch_of(ticket, &LibrespotLibrary::new(session)).await;
						}
						Err(error) => library.fail_of(ticket, &error),
					}
				};
				library.run_within(ticket, LIBRARY_FETCH_TIMEOUT, read).await;
			});
		}
		state
	}

	/// What [`Core::library`] would answer, without starting a read.
	pub fn held_library(&self) -> LibraryState {
		if self.is_logged_in() { self.library.held() } else { LibraryState::LoggedOut }
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
				let source = CachedSource::new(
					LibrespotMetadata::new(session, self.albums.clone()),
					self.cache.clone(),
				);
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
				let source = CachedSource::new(
					LibrespotMetadata::new(session, self.albums.clone()),
					self.cache.clone(),
				);
				item(&source, id).await
			},
			is_aborting,
		)?
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
		// The first reads block until data arrives; do them where Stop can still interrupt.
		match self.runtime.block_on_blocking(move || fetched.open(), is_aborting)? {
			Err(error) if wrong_remembered_key(key_origin, &error) => {
				log::warn!("the remembered key didn't decrypt the track ({error}); asking Spotify");
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

	/// Saves the remembered keys on the runtime's blocking pool, so playback doesn't wait for the
	/// disk.
	fn save_keys_in_background(&self) {
		let (profile, keys) = (Arc::clone(&self.profile), self.keys.clone());
		self.runtime.handle().spawn_blocking(move || {
			if let Err(error) = profile.save_audio_keys(&keys) {
				log::warn!("couldn't save the remembered audio keys: {error}");
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

	/// Adds `items` to a playlist or Liked Songs. Playlists are read first, so only what they
	/// lack is sent.
	pub fn add_to(
		&self,
		target: Target,
		items: &[ItemId],
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Change, CoreError> {
		self.runtime.block_on(
			async {
				let session = self.sessions.session().await?;
				self.library.add(&LibrespotLibrary::new(session), target, items).await
			},
			is_aborting,
		)?
	}

	/// Removes `items` from a playlist, every copy of each, or from Liked Songs.
	pub fn remove_from(
		&self,
		target: Target,
		items: &[ItemId],
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Change, CoreError> {
		self.runtime.block_on(
			async {
				let session = self.sessions.session().await?;
				self.library.remove(&LibrespotLibrary::new(session), target, items).await
			},
			is_aborting,
		)?
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

/// Decides whether a login that is finishing or a cancel came first. Without it, a cancel
/// during the last moments of a login, while the login is stored, would be ignored: tokio stops
/// a task only at its next await, and there is none left.
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
	use crate::library::{LibraryState, Target};
	use crate::link::{ItemId, Link, LinkKind};
	use crate::profile::Profile;
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
	fn library_without_login_is_logged_out() {
		let dir = tempfile::tempdir().unwrap();
		let core = Core::new(dir.path()).unwrap();
		assert!(matches!(core.library(None), LibraryState::LoggedOut));
		assert!(matches!(core.held_library(), LibraryState::LoggedOut));
		core.shutdown();
	}

	#[test]
	fn add_to_without_login_is_not_logged_in() {
		let dir = tempfile::tempdir().unwrap();
		let core = Core::new(dir.path()).unwrap();
		let track = ItemId::Track(SpotifyId::from_base62("4LH4d3cOWNNsVw41Gqt2kv").unwrap());
		let result = core.add_to(Target::LikedSongs, &[track], &|| false);
		assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
		let result = core.remove_from(Target::LikedSongs, &[track], &|| false);
		assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
		core.shutdown();
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
