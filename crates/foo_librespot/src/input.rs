//! The input that plays `sptf://spotify:track:<id>` and `sptf://spotify:episode:<id>` entries.

use std::ffi::CStr;

use foo_librespot_core::CoreError;
use foo_librespot_core::audio::decoder::{CHANNELS, SAMPLE_RATE};
use foo_librespot_core::link::{ItemId, parse_item};
use foxide2k::abi::input::INPUT_FLAG_PLAYBACK;
use foxide2k::abort::Abort;
use foxide2k::audio_chunk::{AudioChunk, CHANNEL_MASK_STEREO};
use foxide2k::cxx::ExceptionKind;
use foxide2k::error::{Error, Result};
use foxide2k::file_info::FileInfo;
use foxide2k::guid::Guid;
use foxide2k::input::{Input, OpenReason};
use foxide2k::popup::{self, PopupIcon};
use foxide2k::{main_thread, playback};

use crate::backend::backend;
use crate::errors::host_error;
use crate::fields::fill_info;
use crate::first_use::login_to_play;
use crate::worker::AbortableTrack;

/// Why anything but playback is refused.
const NOT_FOR_PLAYBACK: &str = "Spotify audio can only be played: foo_librespot doesn't hand it to the Converter or other tools.";

/// A tenth of a second per `decode_run`, as foobar2000's own inputs deliver.
const CHUNK_FRAMES: usize = 4410;

/// The title of the popup after Spotify refused a track's key; a screen reader reads it first.
const REFUSED_KEY_TITLE: &str = "Spotify refused to play a track";

/// Why playback stopped after Spotify refused the key with `code`.
fn refused_key_message(code: u16) -> String {
	// librespot asks again for about a minute before it passes on a code 2, which Spotify sends
	// when many keys are asked for at once.
	if code == 2 {
		"Spotify refused the key that decrypts this track for about a minute, so playback stopped. Spotify does this when many tracks are asked for in a short time. Wait a few minutes before you press Play again.".to_owned()
	} else {
		format!(
			"Spotify refused the key that decrypts this track (code {code}), so playback stopped. Spotify doesn't say why. If it refuses every track, it may be refusing this account."
		)
	}
}

/// Stops playback after Spotify refused a track's key, and says why. Otherwise foobar2000 opens
/// the next track at once, Spotify refuses its key too, and so on through the playlist: a burst
/// of requests that keeps the refusals going.
fn stop_after_refused_key(code: u16) {
	let message = refused_key_message(code);
	let queued = main_thread::run(Box::new(move || {
		if let Err(error) = playback::stop() {
			log::warn!("couldn't stop playback after a refused key: {}", error.message);
		}
		if let Err(error) = popup::show(REFUSED_KEY_TITLE, &message, PopupIcon::Error) {
			log::warn!("couldn't say why playback stopped: {}", error.message);
		}
	}));
	if let Err(error) = queued {
		log::warn!("couldn't stop playback after a refused key: {}", error.message);
	}
}

/// The track or episode `path` names (see [`parse_item`]), or the error foobar2000 expects for a
/// path that isn't ours.
pub fn item_or_error(path: &str) -> Result<ItemId> {
	parse_item(path)
		.ok_or_else(|| Error::unsupported_format(format!("not a Spotify track or episode: {path}")))
}

/// One opened Spotify track or episode.
pub struct SpotifyInput {
	id: ItemId,
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
		parse_item(path).is_some()
	}

	fn open(path: &str, _reason: OpenReason, _abort: &Abort) -> Result<Self> {
		// Nothing to fetch yet: info reads and decoding each ask Spotify for what they need. The
		// buffer grows on the first read, so the info reads foobar2000 opens for every entry cost
		// nothing.
		let id = item_or_error(path)?;
		Ok(Self { id, track: None, buffer: Vec::new() })
	}

	fn get_info(&mut self, info: &mut FileInfo<'_>, abort: &Abort) -> Result<()> {
		let track = backend().track_info(self.id, &|| abort.is_aborting()).map_err(host_error)?;
		fill_info(info, &track)
	}

	fn decode_initialize(&mut self, flags: u32, abort: &Abort) -> Result<()> {
		// Decrypted Spotify audio never reaches a file: only foobar2000's player gets it, as with
		// foo_spotify. The Converter, the ReplayGain scanner and the like are refused.
		if flags & INPUT_FLAG_PLAYBACK == 0 {
			return Err(Error::new(ExceptionKind::IoDenied, NOT_FOR_PLAYBACK));
		}
		let is_aborting = || abort.is_aborting();
		let track = match backend().open_track(self.id, &is_aborting) {
			// The user pressed Play while logged out: log in, then try once more. Info reads in
			// the background never ask.
			Err(CoreError::NotLoggedIn) => login_to_play(&is_aborting)
				.and_then(|()| backend().open_track(self.id, &is_aborting)),
			opened => opened,
		}
		.map_err(|error| {
			if let CoreError::KeyRefused { code } = error {
				stop_after_refused_key(code);
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

	/// The stored info can't know which file plays: that depends on the preferred bitrate and
	/// on which files Spotify has, so the status bar learns it here.
	fn live_info(&self) -> Vec<(String, String)> {
		let Some(track) = &self.track else { return Vec::new() };
		let format = track.file_format();
		let mut fields = vec![("codec".to_owned(), format.codec.to_owned())];
		if let Some(kbps) = format.kbps {
			fields.push(("bitrate".to_owned(), kbps.to_string()));
		}
		fields
	}
}

#[cfg(test)]
mod tests {
	use std::time::{Duration, Instant};

	use foo_librespot_core::CoreError;
	use foxide2k::abi::input::{
		INFO_READER, INPUT_FLAG_PLAYBACK, INPUT_FLAG_SIMPLEDECODE, INPUT_FLAG_TESTING_INTEGRITY,
	};
	use foxide2k::audio_chunk::CHANNEL_MASK_STEREO;
	use foxide2k::cxx::{ExceptionKind, HostException};
	use foxide2k::file_info_impl::FileInfoImpl;
	use foxide2k::input::entry_service;
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::{DecoderHandle, FakeAbort, FakeApi, FakeAudioChunk, InputEntryHandle};

	use super::SpotifyInput;

	use crate::testing::{
		EPISODE_ID, TRACK_ID, cancel_login_when_started, confirm_login_when_started,
		fail_login_when_started, fixture_samples, installed_backend, installed_prompt, stop_after,
	};

	fn path() -> String {
		format!("sptf://spotify:track:{TRACK_ID}")
	}

	fn entry() -> InputEntryHandle {
		InputEntryHandle::new(entry_service::<SpotifyInput>())
	}

	#[test]
	fn claims_only_spotify_track_and_episode_paths() {
		let entry = entry();
		assert!(entry.is_our_path(&path()));
		assert!(entry.is_our_path(&format!("spotify:track:{TRACK_ID}")));
		assert!(entry.is_our_path(&format!("sptf://spotify:episode:{EPISODE_ID}")));
		assert!(entry.is_our_path(&format!("spotify:episode:{EPISODE_ID}")));
		assert!(!entry.is_our_path("sptf://spotify:album:4wPtS24asgBAG4VOmYYB9O"));
		assert!(!entry.is_our_path("sptf://spotify:show:5CfCWKI5pZ28U0uOzXkDHe"));
		assert!(!entry.is_our_path("file://C:\\a.mp3"));
		assert!(!entry.is_our_path("oxitone://440"));
	}

	#[test]
	fn info_read_fills_file_info_without_decoding() {
		let fake = installed_backend();
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
		decoder.initialize(0, INPUT_FLAG_PLAYBACK, &abort).unwrap();
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
		installed_backend();
		let mut frames = Vec::new();
		let samples = decode_all(&mut frames);
		assert_eq!(samples, fixture_samples());
		assert!(frames.iter().all(|&count| count <= 4410), "{frames:?}");
	}

	#[test]
	fn seek_lands_on_the_frame() {
		installed_backend();
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		decoder.initialize(0, INPUT_FLAG_PLAYBACK, &abort).unwrap();
		decoder.seek(1.0, &abort).unwrap();
		let mut chunk = FakeAudioChunk::new();
		assert!(decoder.run(&mut chunk, &abort).unwrap());
		let expected = &fixture_samples()[2 * 44_100..2 * 44_100 + chunk.samples().len()];
		for (want, got) in expected.iter().zip(chunk.samples()) {
			assert!((want - got).abs() < 1e-4);
		}
	}

	/// What foobar2000 shows after polling the decoder's live info once.
	fn shown_after_poll(decoder: &DecoderHandle) -> Option<(String, String)> {
		let info = decoder.live_info()?;
		Some((info.info("codec")?, info.info("bitrate")?))
	}

	fn shown(codec: &str, bitrate: &str) -> (String, String) {
		(codec.to_owned(), bitrate.to_owned())
	}

	#[test]
	fn playing_reports_the_codec_and_bitrate_of_the_file_spotify_sent_once() {
		installed_backend();
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		decoder.initialize(0, INPUT_FLAG_PLAYBACK, &abort).unwrap();
		decoder.run(&mut FakeAudioChunk::new(), &abort).unwrap();
		assert_eq!(shown_after_poll(&decoder), Some(shown("Vorbis", "160")));
		decoder.run(&mut FakeAudioChunk::new(), &abort).unwrap();
		assert_eq!(shown_after_poll(&decoder), None);
	}

	#[test]
	fn seeking_reports_the_codec_and_bitrate_again() {
		installed_backend();
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		decoder.initialize(0, INPUT_FLAG_PLAYBACK, &abort).unwrap();
		decoder.run(&mut FakeAudioChunk::new(), &abort).unwrap();
		assert!(shown_after_poll(&decoder).is_some());
		decoder.seek(1.0, &abort).unwrap();
		decoder.run(&mut FakeAudioChunk::new(), &abort).unwrap();
		assert_eq!(shown_after_poll(&decoder), Some(shown("Vorbis", "160")));
	}

	#[test]
	fn decoding_for_anything_but_playback_is_denied() {
		let fake = installed_backend();
		let abort = FakeAbort::new(false);
		for flags in [0, INPUT_FLAG_SIMPLEDECODE, INPUT_FLAG_TESTING_INTEGRITY] {
			let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
			let error = decoder.initialize(0, flags, &abort).unwrap_err();
			assert_eq!(error.kind, ExceptionKind::IoDenied, "flags {flags}");
			assert_eq!(
				error.message,
				"Spotify audio can only be played: foo_librespot doesn't hand it to the Converter or other tools."
			);
		}
		assert_eq!(fake.open_calls(), 0);
	}

	#[test]
	fn not_logged_in_reaches_the_host_as_an_io_error_with_advice() {
		let fake = installed_backend();
		fake.fail_with(|| CoreError::NotLoggedIn);
		let abort = FakeAbort::new(false);
		let decoder = entry().open_v3(&INFO_READER, &path(), None, None, &abort).unwrap();
		let error = decoder.get_info(0, &mut FileInfoImpl::new(), &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Io);
		assert!(error.message.contains("Preferences > Tools > Spotify"), "{}", error.message);
	}

	#[test]
	fn playing_while_logged_out_waits_for_the_login_then_plays() {
		let fake = installed_backend();
		let prompt = installed_prompt();
		fake.require_login();
		let user = confirm_login_when_started(&fake, 1);
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		decoder.initialize(0, INPUT_FLAG_PLAYBACK, &abort).unwrap();
		user.join().unwrap();
		assert!(decoder.run(&mut FakeAudioChunk::new(), &abort).unwrap());
		assert_eq!(prompt.shown(), 1);
		assert_eq!(fake.logins_started(), 1);
	}

	/// Plays the track while the backend fails with `error`, and returns the error foobar2000 got.
	fn play_failing_with(api: &'static FakeApi, error: fn() -> CoreError) -> HostException {
		api.install();
		let fake = installed_backend();
		fake.fail_with(error);
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		decoder.initialize(0, INPUT_FLAG_PLAYBACK, &abort).unwrap_err()
	}

	#[test]
	fn a_refused_key_stops_playback_and_says_why() {
		let api = FakeApi::new();
		let error = play_failing_with(api, || CoreError::KeyRefused { code: 1 });
		assert_eq!(error.kind, ExceptionKind::Io);
		assert_eq!(error.message, "Spotify refused the key that decrypts this track (code 1)");
		// Stopping is main-thread work: otherwise foobar2000 opens the next track, whose key
		// request Spotify refuses too, and so on through the playlist.
		assert_eq!(api.playback_stops(), 0);
		assert_eq!(api.run_main_thread(), 1);
		assert_eq!(api.playback_stops(), 1);
		assert_eq!(
			api.popups(),
			[(
				"Spotify refused to play a track".to_owned(),
				"Spotify refused the key that decrypts this track (code 1), so playback stopped. Spotify doesn't say why. If it refuses every track, it may be refusing this account.".to_owned(),
				PopupIcon::Error
			)]
		);
	}

	#[test]
	fn a_key_refused_for_a_minute_says_to_wait() {
		let api = FakeApi::new();
		play_failing_with(api, || CoreError::KeyRefused { code: 2 });
		assert_eq!(api.run_main_thread(), 1);
		assert_eq!(api.playback_stops(), 1);
		assert_eq!(
			api.popups(),
			[(
				"Spotify refused to play a track".to_owned(),
				"Spotify refused the key that decrypts this track for about a minute, so playback stopped. Spotify does this when many tracks are asked for in a short time. Wait a few minutes before you press Play again.".to_owned(),
				PopupIcon::Error
			)]
		);
	}

	#[test]
	fn other_failures_leave_playback_to_foobar2000() {
		let api = FakeApi::new();
		play_failing_with(api, || {
			CoreError::Unavailable("not available in your country (NL)".to_owned())
		});
		assert_eq!(api.run_main_thread(), 0);
		assert_eq!(api.playback_stops(), 0);
		assert!(api.popups().is_empty(), "{:?}", api.popups());
	}

	#[test]
	fn a_cancelled_login_fails_the_play_with_advice() {
		let fake = installed_backend();
		let _prompt = installed_prompt();
		fake.require_login();
		let user = cancel_login_when_started(&fake);
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		let error = decoder.initialize(0, INPUT_FLAG_PLAYBACK, &abort).unwrap_err();
		user.join().unwrap();
		assert_eq!(error.kind, ExceptionKind::Io);
		assert_eq!(
			error.message,
			"Not logged in to Spotify. Log in from File > Preferences > Tools > Spotify."
		);
	}

	#[test]
	fn after_a_cancelled_login_playing_fails_without_asking_again() {
		let fake = installed_backend();
		let prompt = installed_prompt();
		fake.require_login();
		let user = cancel_login_when_started(&fake);
		let abort = FakeAbort::new(false);
		let first = entry().open_for_decoding(&path(), &abort).unwrap();
		assert!(first.initialize(0, INPUT_FLAG_PLAYBACK, &abort).is_err());
		user.join().unwrap();
		let second = entry().open_for_decoding(&path(), &abort).unwrap();
		let error = second.initialize(0, INPUT_FLAG_PLAYBACK, &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Io);
		assert_eq!(prompt.shown(), 1);
		assert_eq!(fake.logins_started(), 1);
	}

	#[test]
	fn after_a_failed_login_playing_fails_without_asking_again() {
		let fake = installed_backend();
		let prompt = installed_prompt();
		fake.require_login();
		let spotify = fail_login_when_started(&fake);
		let abort = FakeAbort::new(false);
		let first = entry().open_for_decoding(&path(), &abort).unwrap();
		assert!(first.initialize(0, INPUT_FLAG_PLAYBACK, &abort).is_err());
		spotify.join().unwrap();
		let second = entry().open_for_decoding(&path(), &abort).unwrap();
		assert!(second.initialize(0, INPUT_FLAG_PLAYBACK, &abort).is_err());
		assert_eq!(prompt.shown(), 1);
		assert_eq!(fake.logins_started(), 1);
	}

	#[test]
	fn stop_while_waiting_for_the_login_returns_promptly() {
		let fake = installed_backend();
		let _prompt = installed_prompt();
		fake.require_login();
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		let stopper = stop_after(&abort, Duration::from_millis(100));
		let started = Instant::now();
		let error = decoder.initialize(0, INPUT_FLAG_PLAYBACK, &abort).unwrap_err();
		stopper.join().unwrap();
		assert_eq!(error.kind, ExceptionKind::Aborted);
		assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
	}

	#[test]
	fn info_reads_while_logged_out_never_prompt() {
		let fake = installed_backend();
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
		let fake = installed_backend();
		fake.fail_with(|| CoreError::Aborted);
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		let error = decoder.initialize(0, INPUT_FLAG_PLAYBACK, &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Aborted);
	}

	#[test]
	fn stop_during_a_stalled_read_returns_promptly() {
		let fake = installed_backend();
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		decoder.initialize(0, INPUT_FLAG_PLAYBACK, &abort).unwrap();
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
		let fake = installed_backend();
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding(&path(), &abort).unwrap();
		decoder.initialize(0, INPUT_FLAG_PLAYBACK, &abort).unwrap();
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
		installed_backend();
		let expected = fixture_samples();
		let threads: Vec<_> =
			(0..2).map(|_| std::thread::spawn(|| decode_all(&mut Vec::new()))).collect();
		for thread in threads {
			assert_eq!(thread.join().unwrap(), expected);
		}
	}
}
