//! The Spotify side of the component: `foo_librespot_core::Core` behind a small trait, so the
//! services can be tested with a fake.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use foo_librespot_core::audio::format::Bitrate;
use foo_librespot_core::audio::track::SpotifyTrack;
use foo_librespot_core::link::Link;
use foo_librespot_core::metadata::TrackInfo;
use foo_librespot_core::resolve::Resolved;
use foo_librespot_core::{Core, CoreError, SpotifyId};

use crate::lifecycle::Lifecycle;

/// Everything the services need from Spotify. Every call blocks and returns
/// [`CoreError::Aborted`] soon after `is_aborting` turns true.
pub trait Backend: Send + Sync {
	fn resolve(&self, link: &Link, is_aborting: &dyn Fn() -> bool) -> Result<Resolved, CoreError>;
	fn track_info(
		&self,
		id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Arc<TrackInfo>, CoreError>;
	fn open_track(
		&self,
		id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Box<dyn PcmTrack>, CoreError>;
	fn cover(
		&self,
		id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Option<Vec<u8>>, CoreError>;
}

/// A track opened for decoding: 44.1 kHz stereo, interleaved `f64`.
pub trait PcmTrack: Send {
	fn read(&mut self, out: &mut Vec<f64>, max_frames: usize) -> Result<usize, CoreError>;
	fn seek(&mut self, frame: u64) -> Result<(), CoreError>;
}

impl PcmTrack for SpotifyTrack {
	fn read(&mut self, out: &mut Vec<f64>, max_frames: usize) -> Result<usize, CoreError> {
		Self::read(self, out, max_frames)
	}

	fn seek(&mut self, frame: u64) -> Result<(), CoreError> {
		Self::seek(self, frame)
	}
}

/// Where foo_librespot keeps its state: `foo_librespot` in the foobar2000 profile, which the
/// host reports with a `file://` prefix.
pub fn profile_folder(profile_path: &str) -> PathBuf {
	let path = profile_path.strip_prefix("file://").unwrap_or(profile_path);
	Path::new(path.trim_end_matches(['\\', '/'])).join("foo_librespot")
}

/// `Core`, started the first time anything asks Spotify, and stopped when foobar2000 quits.
static CORE: Lifecycle<Core> = Lifecycle::new();

/// How long quitting waits for Spotify calls still running; foobar2000 aborts them as it quits.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

fn core() -> Result<Arc<Core>, CoreError> {
	CORE.get_or_start(|| Core::new(profile_folder(&oxibar::host::host().profile_path())))
}

/// Stops `Core` for good, if it was started; later calls fail instead of starting it again.
pub fn shutdown() {
	CORE.shutdown(SHUTDOWN_WAIT, Core::shutdown);
}

struct CoreBackend;

impl Backend for CoreBackend {
	fn resolve(&self, link: &Link, is_aborting: &dyn Fn() -> bool) -> Result<Resolved, CoreError> {
		core()?.resolve(link, is_aborting)
	}

	fn track_info(
		&self,
		id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Arc<TrackInfo>, CoreError> {
		core()?.track_info(id, is_aborting)
	}

	fn open_track(
		&self,
		id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Box<dyn PcmTrack>, CoreError> {
		// The bitrate becomes a preference in phase 4.
		Ok(Box::new(core()?.open_track(id, Bitrate::default(), is_aborting)?))
	}

	fn cover(
		&self,
		id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Option<Vec<u8>>, CoreError> {
		core()?.cover(id, is_aborting)
	}
}

static BACKEND: OnceLock<Box<dyn Backend>> = OnceLock::new();

/// The backend the services use: Spotify, or the one a test installed.
pub fn backend() -> &'static dyn Backend {
	BACKEND.get_or_init(|| Box::new(CoreBackend)).as_ref()
}

/// Makes `backend()` return `fake` in this test process.
#[cfg(test)]
pub fn install(fake: Box<dyn Backend>) {
	assert!(BACKEND.set(fake).is_ok(), "a backend was already in use");
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use super::profile_folder;

	#[test]
	fn profile_folder_strips_the_file_scheme() {
		assert_eq!(
			profile_folder(r"file://C:\fb2k\profile"),
			PathBuf::from(r"C:\fb2k\profile\foo_librespot")
		);
		assert_eq!(
			profile_folder(r"C:\fb2k\profile\"),
			PathBuf::from(r"C:\fb2k\profile\foo_librespot")
		);
	}
}
