//! `Core`: the one entry point the foobar2000 plugin uses.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::CoreError;
use crate::art::{CoverCache, fetch_cover};
use crate::audio::format::Bitrate;
use crate::audio::track::{SpotifyTrack, fetch_track};
use crate::link::{ItemId, Link};
use crate::login::{DeviceCode, LibrespotAuthorizer, device_login};
use crate::metadata::{TrackInfo, largest_cover};
use crate::profile::Profile;
use crate::resolve::{
	AlbumTotalsCache, CachedSource, LibrespotMetadata, Resolved, TrackCache, item, resolve,
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
	sessions: Arc<SessionManager<LibrespotConnector>>,
	cache: TrackCache,
	totals: AlbumTotalsCache,
	covers: CoverCache,
}

impl Core {
	/// Uses `profile_dir` for the device id and the encrypted login, creating it if needed.
	pub fn new(profile_dir: impl Into<PathBuf>) -> Result<Self, CoreError> {
		let profile = Arc::new(Profile::new(profile_dir));
		let device_id = profile.device_id()?;
		let sessions = SessionManager::new(LibrespotConnector::new(device_id), profile);
		Ok(Self {
			runtime: CoreRuntime::new()?,
			sessions: Arc::new(sessions),
			cache: TrackCache::default(),
			totals: AlbumTotalsCache::default(),
			covers: CoverCache::default(),
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
		let gate = LoginGate::default();
		let finishing = gate.clone();
		let task = self.runtime.spawn(async move {
			let result = async {
				let authorizer = LibrespotAuthorizer::new()?;
				let token = device_login(&authorizer, on_code).await?;
				let username = sessions.login_with_token(token).await?;
				// Which tracks are available depends on the account's country.
				cache.clear();
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

	pub fn logout(&self) -> Result<(), CoreError> {
		self.sessions.logout()
	}

	pub fn resolve(
		&self,
		link: &Link,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Resolved, CoreError> {
		self.runtime.block_on(
			async {
				let session = self.sessions.session().await?;
				let source = CachedSource::new(
					LibrespotMetadata::new(session, self.totals.clone()),
					self.cache.clone(),
				);
				resolve(&source, link).await
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
					LibrespotMetadata::new(session, self.totals.clone()),
					self.cache.clone(),
				);
				item(&source, id).await
			},
			is_aborting,
		)?
	}

	/// Opens a track or episode for decoding. The download starts in the background; reading
	/// blocks the caller until the data it needs has arrived.
	pub fn open_track(
		&self,
		id: ItemId,
		bitrate: Bitrate,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<SpotifyTrack, CoreError> {
		let fetched = self.runtime.block_on(
			async {
				let session = self.sessions.session().await?;
				fetch_track(&session, id, bitrate).await
			},
			is_aborting,
		)??;
		// The first reads block until data arrives; do them where Stop can still interrupt.
		self.runtime.block_on_blocking(move || fetched.open(), is_aborting)?
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
	use super::{Core, LoginGate, LoginHandle};
	use crate::CoreError;
	use crate::SpotifyId;
	use crate::audio::track::SpotifyTrack;
	use crate::link::{Link, LinkKind};

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
		Link {
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
	fn resolve_without_login_is_not_logged_in() {
		let dir = tempfile::tempdir().unwrap();
		let core = Core::new(dir.path()).unwrap();
		let result = core.resolve(&album(), &|| false);
		assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
		core.shutdown();
	}

	#[test]
	fn aborted_call_is_aborted() {
		let dir = tempfile::tempdir().unwrap();
		let core = Core::new(dir.path()).unwrap();
		let result = core.resolve(&album(), &|| true);
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
