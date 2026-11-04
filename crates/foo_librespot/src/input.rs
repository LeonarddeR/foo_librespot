//! The input that plays `sptf://spotify:track:<id>` entries.

use std::ffi::CStr;

use foo_librespot_core::audio::decoder::{CHANNELS, SAMPLE_RATE};
use foo_librespot_core::link::parse_track_path;
use foo_librespot_core::{CoreError, SpotifyId};
use oxibar::abort::Abort;
use oxibar::audio_chunk::{AudioChunk, CHANNEL_MASK_STEREO};
use oxibar::cxx::ExceptionKind;
use oxibar::error::{Error, Result};
use oxibar::file_info::FileInfo;
use oxibar::guid::Guid;
use oxibar::input::{Input, OpenReason};

use crate::backend::backend;
use crate::errors::{PLAY_LOGGED_OUT, host_error};
use crate::fields::fill_info;
use crate::first_use::prompt_login;
use crate::worker::AbortableTrack;

/// A tenth of a second per `decode_run`, as foobar2000's own inputs deliver.
const CHUNK_FRAMES: usize = 4410;

/// The Spotify track a path names: `sptf://spotify:track:<id>`, which foo_librespot and
/// foo_spotify store, or a bare `spotify:track:<id>`.
pub fn track_id(path: &str) -> Option<SpotifyId> {
	if let Some(id) = parse_track_path(path) {
		return Some(id);
	}
	let path = path.trim();
	let prefix = path.get(.."spotify:track:".len())?;
	if !prefix.eq_ignore_ascii_case("spotify:track:") {
		return None;
	}
	SpotifyId::from_base62(&path[prefix.len()..]).ok()
}

/// One opened Spotify track.
pub struct SpotifyInput {
	id: SpotifyId,
	track: Option<AbortableTrack>,
	buffer: Vec<f64>,
}

impl SpotifyInput {
	fn track(&mut self) -> Result<&mut AbortableTrack> {
		self.track
			.as_mut()
			.ok_or_else(|| Error::new(ExceptionKind::BugCheck, "decoding before decode_initialize"))
	}
}

impl Input for SpotifyInput {
	const GUID: Guid = Guid::from_u128(0x4b0cd358_f0b2_469c_8bc0_c2ac81d3b91c);
	const NAME: &'static CStr = c"Spotify (foo_librespot)";

	fn is_our_path(path: &str) -> bool {
		track_id(path).is_some()
	}

	fn open(path: &str, _reason: OpenReason, _abort: &Abort) -> Result<Self> {
		// Nothing to fetch yet: info reads and decoding each ask Spotify for what they need.
		let id = track_id(path)
			.ok_or_else(|| Error::unsupported_format(format!("not a Spotify track: {path}")))?;
		Ok(Self { id, track: None, buffer: Vec::with_capacity(CHUNK_FRAMES * CHANNELS) })
	}

	fn get_info(&mut self, info: &mut FileInfo<'_>, abort: &Abort) -> Result<()> {
		let track = backend().track_info(self.id, &|| abort.is_aborting()).map_err(host_error)?;
		fill_info(info, &track)
	}

	fn decode_initialize(&mut self, _flags: u32, abort: &Abort) -> Result<()> {
		let track = backend().open_track(self.id, &|| abort.is_aborting()).map_err(|error| {
			if matches!(error, CoreError::NotLoggedIn) {
				// The user pressed Play; info reads in the background never ask.
				prompt_login();
				return Error::new(ExceptionKind::Io, PLAY_LOGGED_OUT);
			}
			host_error(error)
		})?;
		self.track = Some(AbortableTrack::new(track).map_err(host_error)?);
		Ok(())
	}

	fn decode_run(&mut self, chunk: &mut AudioChunk<'_>, abort: &Abort) -> Result<bool> {
		abort.check()?;
		let mut buffer = std::mem::take(&mut self.buffer);
		let frames = self
			.track()?
			.read(&mut buffer, CHUNK_FRAMES, &|| abort.is_aborting())
			.map_err(host_error);
		self.buffer = buffer;
		let frames = frames?;
		if frames == 0 {
			return Ok(false);
		}
		let channels = u32::try_from(CHANNELS).unwrap_or(2);
		chunk.set_data(&self.buffer, frames, SAMPLE_RATE, channels, CHANNEL_MASK_STEREO)?;
		Ok(true)
	}

	fn decode_seek(&mut self, seconds: f64, abort: &Abort) -> Result<()> {
		abort.check()?;
		#[allow(
			clippy::cast_possible_truncation,
			clippy::cast_sign_loss,
			reason = "clamped to 0 and up"
		)]
		let frame = (seconds.max(0.0) * f64::from(SAMPLE_RATE)).round() as u64;
		self.track()?.seek(frame, &|| abort.is_aborting()).map_err(host_error)
	}

	fn can_seek(&self) -> bool {
		true
	}
}

#[cfg(test)]
mod tests {
	use std::time::{Duration, Instant};

	use foo_librespot_core::CoreError;
	use oxibar::abi::input::INFO_READER;
	use oxibar::audio_chunk::CHANNEL_MASK_STEREO;
	use oxibar::cxx::ExceptionKind;
	use oxibar::file_info_impl::FileInfoImpl;
	use oxibar::input::entry_service;
	use oxibar::testing::{FakeAbort, FakeAudioChunk, InputEntryHandle};

	use super::SpotifyInput;
	use crate::backend;
	use crate::testing::{FakeBackend, TRACK_ID, fixture_samples, installed_prompt, stop_after};

	fn path() -> String {
		format!("sptf://spotify:track:{TRACK_ID}")
	}

	fn entry() -> InputEntryHandle {
		InputEntryHandle::new(entry_service::<SpotifyInput>())
	}

	fn installed() -> FakeBackend {
		let fake = FakeBackend::default();
		backend::install(Box::new(fake.clone()));
		fake
	}

	#[test]
	fn claims_only_spotify_track_paths() {
		let entry = entry();
		assert!(entry.is_our_path(&path()));
		assert!(entry.is_our_path(&format!("spotify:track:{TRACK_ID}")));
		assert!(!entry.is_our_path("sptf://spotify:album:4wPtS24asgBAG4VOmYYB9O"));
		assert!(!entry.is_our_path("file://C:\\a.mp3"));
		assert!(!entry.is_our_path("oxitone://440"));
	}

	#[test]
	fn info_read_fills_file_info_without_decoding() {
		let fake = installed();
		let abort = FakeAbort::new(false);
		let decoder = entry().open_v3(&INFO_READER, &path(), None, None, &abort).unwrap();
		let mut info = FileInfoImpl::new();
		decoder.get_info(0, &mut info, &abort).unwrap();
		assert_eq!(info.meta("TITLE"), ["In Christ Alone"]);
		assert_eq!(fake.open_calls(), 0);
	}

	fn decode_all(chunk_frames: &mut Vec<usize>) -> Vec<f64> {
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		decoder.initialize(0, 0, &abort).unwrap();
		let mut samples = Vec::new();
		let mut chunk = FakeAudioChunk::new();
		while decoder.run(&mut chunk, &abort).unwrap() {
			assert_eq!((chunk.sample_rate(), chunk.channels()), (44_100, 2));
			assert_eq!(chunk.channel_mask(), CHANNEL_MASK_STEREO);
			chunk_frames.push(chunk.sample_count());
			samples.extend_from_slice(chunk.samples());
		}
		assert!(!decoder.run(&mut chunk, &abort).unwrap());
		samples
	}

	#[test]
	fn decoding_delivers_the_fixture_then_ends() {
		installed();
		let mut frames = Vec::new();
		let samples = decode_all(&mut frames);
		assert_eq!(samples, fixture_samples());
		assert!(frames.iter().all(|&count| count <= 4410), "{frames:?}");
	}

	#[test]
	fn seek_lands_on_the_frame() {
		installed();
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		decoder.initialize(0, 0, &abort).unwrap();
		decoder.seek(1.0, &abort).unwrap();
		let mut chunk = FakeAudioChunk::new();
		assert!(decoder.run(&mut chunk, &abort).unwrap());
		let expected = &fixture_samples()[2 * 44_100..2 * 44_100 + chunk.samples().len()];
		for (want, got) in expected.iter().zip(chunk.samples()) {
			assert!((want - got).abs() < 1e-4);
		}
	}

	#[test]
	fn not_logged_in_reaches_the_host_as_an_io_error_with_advice() {
		let fake = installed();
		fake.fail_with(|| CoreError::NotLoggedIn);
		let abort = FakeAbort::new(false);
		let decoder = entry().open_v3(&INFO_READER, &path(), None, None, &abort).unwrap();
		let error = decoder.get_info(0, &mut FileInfoImpl::new(), &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Io);
		assert!(error.message.contains("spotify_dev"), "{}", error.message);
	}

	#[test]
	fn decoding_while_logged_out_prompts_and_fails_with_advice() {
		let fake = installed();
		let prompt = installed_prompt();
		fake.fail_with(|| CoreError::NotLoggedIn);
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		let error = decoder.initialize(0, 0, &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Io);
		assert_eq!(
			error.message,
			"Not logged in to Spotify. Confirm the login in the window that just opened, then play again."
		);
		assert_eq!(prompt.shown(), 1);
		assert_eq!(fake.logins_started(), 1);
	}

	#[test]
	fn info_reads_while_logged_out_never_prompt() {
		let fake = installed();
		let prompt = installed_prompt();
		fake.fail_with(|| CoreError::NotLoggedIn);
		let abort = FakeAbort::new(false);
		let decoder = entry().open_v3(&INFO_READER, &path(), None, None, &abort).unwrap();
		assert!(decoder.get_info(0, &mut FileInfoImpl::new(), &abort).is_err());
		assert_eq!(prompt.shown(), 0);
		assert_eq!(fake.logins_started(), 0);
	}

	#[test]
	fn aborted_open_throws_aborted() {
		let fake = installed();
		fake.fail_with(|| CoreError::Aborted);
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		let error = decoder.initialize(0, 0, &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Aborted);
	}

	#[test]
	fn stop_during_a_stalled_read_returns_promptly() {
		let fake = installed();
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		decoder.initialize(0, 0, &abort).unwrap();
		fake.stall(true);
		let stopper = stop_after(&abort, Duration::from_millis(100));
		let started = Instant::now();
		let error = decoder.run(&mut FakeAudioChunk::new(), &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Aborted);
		assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
		stopper.join().unwrap();
		fake.stall(false);
	}

	#[test]
	fn stop_during_a_stalled_seek_returns_promptly() {
		let fake = installed();
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		decoder.initialize(0, 0, &abort).unwrap();
		fake.stall(true);
		let stopper = stop_after(&abort, Duration::from_millis(100));
		let started = Instant::now();
		let error = decoder.seek(1.0, &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Aborted);
		assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
		stopper.join().unwrap();
		fake.stall(false);
	}

	#[test]
	fn two_decoders_on_two_threads_play_independently() {
		installed();
		let expected = fixture_samples();
		let threads: Vec<_> =
			(0..2).map(|_| std::thread::spawn(|| decode_all(&mut Vec::new()))).collect();
		for thread in threads {
			assert_eq!(thread.join().unwrap(), expected);
		}
	}
}
