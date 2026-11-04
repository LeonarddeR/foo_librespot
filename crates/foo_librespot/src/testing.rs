//! A fake Spotify for the service tests: one Keith & Kristyn Getty track whose audio is the
//! chirp fixture of `foo_librespot_core`.

use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use foo_librespot_core::audio::decoder::PcmDecoder;
use foo_librespot_core::link::Link;
use foo_librespot_core::metadata::TrackInfo;
use foo_librespot_core::replaygain::ReplayGain;
use foo_librespot_core::resolve::Resolved;
use foo_librespot_core::{CoreError, SpotifyId};

use crate::backend::{Backend, PcmTrack};

pub const CHIRP: &[u8] = include_bytes!("../../foo_librespot_core/tests/fixtures/chirp.ogg");

/// "In Christ Alone" by Keith & Kristyn Getty.
pub const TRACK_ID: &str = "2hQxLiI93xdjqENJuKeXjO";

pub fn track_id() -> SpotifyId {
	SpotifyId::from_base62(TRACK_ID).unwrap()
}

pub fn track_info(id: SpotifyId, title: &str) -> TrackInfo {
	TrackInfo {
		id,
		title: title.to_owned(),
		artists: vec!["Keith Getty".to_owned(), "Kristyn Getty".to_owned()],
		album: "In Christ Alone".to_owned(),
		album_artists: vec!["Keith & Kristyn Getty".to_owned()],
		track_number: 4,
		disc_number: 1,
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
	}
}

#[allow(dead_code, reason = "the loader and album art tests use them from the next commits")]
#[derive(Default)]
struct State {
	calls: AtomicUsize,
	open_calls: AtomicUsize,
	failure: Mutex<Option<fn() -> CoreError>>,
	resolved: Mutex<Option<Resolved>>,
	cover: Mutex<Option<Vec<u8>>>,
}

/// Clones share their state, so a test can install one and inspect another.
#[derive(Clone, Default)]
pub struct FakeBackend(Arc<State>);

#[allow(dead_code, reason = "the loader and album art tests use them from the next commits")]
impl FakeBackend {
	/// Every call fails with `error()` from now on.
	pub fn fail_with(&self, error: fn() -> CoreError) {
		*self.0.failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
	}

	pub fn resolve_to(&self, resolved: Resolved) {
		*self.0.resolved.lock().unwrap_or_else(PoisonError::into_inner) = Some(resolved);
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

	fn enter(&self, is_aborting: &dyn Fn() -> bool) -> Result<(), CoreError> {
		self.0.calls.fetch_add(1, Ordering::SeqCst);
		if is_aborting() {
			return Err(CoreError::Aborted);
		}
		let failure = *self.0.failure.lock().unwrap_or_else(PoisonError::into_inner);
		failure.map_or(Ok(()), |error| Err(error()))
	}
}

impl Backend for FakeBackend {
	fn resolve(&self, _link: &Link, is_aborting: &dyn Fn() -> bool) -> Result<Resolved, CoreError> {
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
		id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Arc<TrackInfo>, CoreError> {
		self.enter(is_aborting)?;
		Ok(Arc::new(track_info(id, "In Christ Alone")))
	}

	fn open_track(
		&self,
		_id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Box<dyn PcmTrack>, CoreError> {
		self.0.open_calls.fetch_add(1, Ordering::SeqCst);
		self.enter(is_aborting)?;
		Ok(Box::new(FixtureTrack(PcmDecoder::new(
			Box::new(Cursor::new(CHIRP)),
			Some("audio/ogg"),
		)?)))
	}

	fn cover(
		&self,
		_id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Option<Vec<u8>>, CoreError> {
		self.enter(is_aborting)?;
		Ok(self.0.cover.lock().unwrap_or_else(PoisonError::into_inner).clone())
	}
}

/// The fixture, decoded.
struct FixtureTrack(PcmDecoder);

impl PcmTrack for FixtureTrack {
	fn read(&mut self, out: &mut Vec<f64>, max_frames: usize) -> Result<usize, CoreError> {
		self.0.read(out, max_frames)
	}

	fn seek(&mut self, frame: u64) -> Result<(), CoreError> {
		self.0.seek(frame)
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
