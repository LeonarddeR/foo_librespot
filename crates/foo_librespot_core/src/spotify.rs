//! `Core`: the one entry point the foobar2000 plugin uses.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::art::fetch_cover;
use crate::audio::format::Bitrate;
use crate::audio::track::{SpotifyTrack, fetch_track};
use crate::link::Link;
use crate::login::{DeviceCode, LibrespotAuthorizer, device_login};
use crate::metadata::{TrackInfo, largest_cover};
use crate::profile::Profile;
use crate::resolve::{
	CachedSource, LibrespotMetadata, MetadataSource, Resolved, TrackCache, resolve,
};
use crate::runtime::CoreRuntime;
use crate::session::{LibrespotConnector, SessionManager};
use crate::{CoreError, SpotifyId};

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
	/// thread. After [`LoginHandle::cancel`], neither is called.
	pub fn start_login(
		&self,
		on_code: impl FnOnce(&DeviceCode) + Send + 'static,
		on_done: impl FnOnce(Result<String, CoreError>) + Send + 'static,
	) -> LoginHandle {
		let sessions = Arc::clone(&self.sessions);
		let cache = self.cache.clone();
		LoginHandle(self.runtime.spawn(async move {
			let result = async {
				let authorizer = LibrespotAuthorizer::new()?;
				let token = device_login(&authorizer, on_code).await?;
				let username = sessions.login_with_token(token).await?;
				// Which tracks are available depends on the account's country.
				cache.clear();
				Ok(username)
			}
			.await;
			on_done(result);
		}))
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
				let source = CachedSource::new(LibrespotMetadata::new(session), self.cache.clone());
				resolve(&source, link).await
			},
			is_aborting,
		)?
	}

	pub fn track_info(
		&self,
		id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Arc<TrackInfo>, CoreError> {
		if let Some(info) = self.cache.get(&id) {
			return Ok(info);
		}
		self.runtime.block_on(
			async {
				let session = self.sessions.session().await?;
				CachedSource::new(LibrespotMetadata::new(session), self.cache.clone())
					.track(id)
					.await
			},
			is_aborting,
		)?
	}

	/// Opens a track for decoding. The download starts in the background; reading blocks the
	/// caller until the data it needs has arrived.
	pub fn open_track(
		&self,
		id: SpotifyId,
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

	/// The largest album cover of a track, as JPEG bytes; `None` when it has none.
	pub fn cover(
		&self,
		id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Option<Vec<u8>>, CoreError> {
		let info = self.track_info(id, is_aborting)?;
		let Some(cover) = largest_cover(&info.covers).cloned() else {
			return Ok(None);
		};
		self.runtime
			.block_on(
				async {
					let session = self.sessions.session().await?;
					fetch_cover(&session, &cover).await
				},
				is_aborting,
			)?
			.map(Some)
	}

	/// Stops the runtime; foobar2000 calls this when it quits.
	pub fn shutdown(self) {
		self.runtime.shutdown(SHUTDOWN_TIMEOUT);
	}
}

/// A login running in the background.
pub struct LoginHandle(tokio::task::JoinHandle<()>);

impl LoginHandle {
	pub fn cancel(&self) {
		self.0.abort();
	}

	pub fn is_finished(&self) -> bool {
		self.0.is_finished()
	}
}

#[cfg(test)]
mod tests {
	use super::{Core, LoginHandle};
	use crate::CoreError;
	use crate::SpotifyId;
	use crate::audio::track::SpotifyTrack;
	use crate::link::{Link, LinkKind};

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
