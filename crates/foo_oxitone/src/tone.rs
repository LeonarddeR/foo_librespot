//! `oxitone://` paths and the sine wave they play.

use std::f64::consts::PI;

use foxide2k::strings::strip_prefix_ignore_case;

pub const SCHEME: &str = "oxitone://";
pub const SAMPLE_RATE: u32 = 44_100;
/// Stereo: every frame holds the same sample for left and right.
pub const CHANNELS: u32 = 2;
pub const DURATION_SECONDS: u32 = 10;
pub const TOTAL_FRAMES: usize = (SAMPLE_RATE * DURATION_SECONDS) as usize;
/// -12 dBFS: clearly audible, never loud.
pub const AMPLITUDE: f64 = 0.25;
/// A tenth of a second per `decode_run`.
pub const CHUNK_FRAMES: usize = (SAMPLE_RATE / 10) as usize;
/// The highest frequency below the Nyquist limit of 44.1 kHz.
pub const MAX_HZ: u32 = SAMPLE_RATE / 2 - 1;

/// What an `oxitone://` path asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToneSpec {
	/// `oxitone://<Hz>`: a stereo sine wave.
	Sine { hz: u32 },
	/// `oxitone://hostcheck`: make foobar2000 throw an exception, catch it and throw it back.
	HostCheck,
	/// `oxitone://panic`, only with the `test-panic` feature: panic while opening.
	#[cfg(feature = "test-panic")]
	Panic,
}

/// Whether `path` uses the `oxitone://` scheme, case-insensitively, whatever follows it.
pub fn has_scheme(path: &str) -> bool {
	strip_prefix_ignore_case(path, SCHEME).is_some()
}

/// Parses an `oxitone://` path. Only plain decimal frequencies from 1 to [`MAX_HZ`] are valid.
pub fn parse(path: &str) -> Option<ToneSpec> {
	let rest = strip_prefix_ignore_case(path, SCHEME)?;
	if rest.eq_ignore_ascii_case("hostcheck") {
		return Some(ToneSpec::HostCheck);
	}
	#[cfg(feature = "test-panic")]
	if rest.eq_ignore_ascii_case("panic") {
		return Some(ToneSpec::Panic);
	}
	if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
		return None;
	}
	let hz: u32 = rest.parse().ok()?;
	(1..=MAX_HZ).contains(&hz).then_some(ToneSpec::Sine { hz })
}

/// A ten-second stereo sine wave, read frame by frame.
pub struct Tone {
	hz: u32,
	next_frame: usize,
}

impl Tone {
	pub fn new(hz: u32) -> Self {
		Self { hz, next_frame: 0 }
	}

	pub fn hz(&self) -> u32 {
		self.hz
	}

	/// Replaces `out` with up to `max_frames` interleaved stereo frames and returns how many it
	/// wrote; 0 at the end.
	pub fn fill(&mut self, out: &mut Vec<f64>, max_frames: usize) -> usize {
		let frames = max_frames.min(TOTAL_FRAMES - self.next_frame);
		out.clear();
		let step = 2.0 * PI * f64::from(self.hz) / f64::from(SAMPLE_RATE);
		for frame in self.next_frame..self.next_frame + frames {
			#[allow(clippy::cast_precision_loss)]
			let sample = AMPLITUDE * (step * frame as f64).sin();
			out.extend(std::iter::repeat_n(sample, CHANNELS as usize));
		}
		self.next_frame += frames;
		frames
	}

	/// Moves to the frame nearest `seconds`. Past the end means "at the end"; negative or NaN
	/// means "at the start".
	pub fn seek(&mut self, seconds: f64) {
		let frame = (seconds * f64::from(SAMPLE_RATE)).round();
		#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
		let frame = if frame.is_nan() || frame <= 0.0 { 0 } else { (frame as usize).min(TOTAL_FRAMES) };
		self.next_frame = frame;
	}
}

#[cfg(test)]
mod tests {
	use super::{AMPLITUDE, CHUNK_FRAMES, SAMPLE_RATE, Tone, ToneSpec, parse};

	#[test]
	fn parses_plain_hz() {
		assert_eq!(parse("oxitone://440"), Some(ToneSpec::Sine { hz: 440 }));
	}

	#[test]
	fn scheme_is_case_insensitive() {
		assert_eq!(parse("OXITONE://440"), Some(ToneSpec::Sine { hz: 440 }));
	}

	#[test]
	fn rejects_non_numeric_and_suffix() {
		for path in
			["oxitone://abc", "oxitone://440abc", "oxitone://440/", "oxitone://", "oxitone://+440"]
		{
			assert_eq!(parse(path), None, "{path}");
		}
	}

	#[test]
	fn rejects_out_of_range() {
		assert_eq!(parse("oxitone://0"), None);
		assert_eq!(parse("oxitone://22050"), None);
		assert_eq!(parse("oxitone://99999999999999999999"), None);
		assert_eq!(parse("oxitone://22049"), Some(ToneSpec::Sine { hz: 22_049 }));
	}

	#[cfg(feature = "test-panic")]
	#[test]
	fn recognises_panic_in_any_case_like_the_rest_of_the_path() {
		for path in ["oxitone://panic", "OXITONE://PANIC", "oxitone://Panic"] {
			assert_eq!(parse(path), Some(ToneSpec::Panic), "{path}");
		}
	}

	#[test]
	fn recognises_hostcheck() {
		assert_eq!(parse("oxitone://hostcheck"), Some(ToneSpec::HostCheck));
	}

	#[test]
	fn rejects_other_schemes() {
		assert_eq!(parse("tone://440"), None);
		assert_eq!(parse("file://C:\\oxitone://440"), None);
	}

	#[test]
	fn sample_formula() {
		let mut tone = Tone::new(440);
		let mut samples = Vec::new();
		tone.fill(&mut samples, 2);
		let expected =
			AMPLITUDE * (2.0 * std::f64::consts::PI * 440.0 / f64::from(SAMPLE_RATE)).sin();
		// Frame 1, both channels.
		assert!((samples[2] - expected).abs() < 1e-12);
		assert!((samples[3] - expected).abs() < 1e-12);
	}

	#[test]
	fn total_frames_is_441000() {
		let mut tone = Tone::new(440);
		let mut samples = Vec::new();
		let mut total = 0;
		loop {
			let frames = tone.fill(&mut samples, CHUNK_FRAMES);
			if frames == 0 {
				break;
			}
			total += frames;
		}
		assert_eq!(total, 441_000);
	}

	#[test]
	fn seek_to_5s_starts_at_frame_220500() {
		let mut tone = Tone::new(440);
		tone.seek(5.0);
		let mut samples = Vec::new();
		tone.fill(&mut samples, 1);
		let expected = AMPLITUDE
			* (2.0 * std::f64::consts::PI * 440.0 * 220_500.0 / f64::from(SAMPLE_RATE)).sin();
		assert!((samples[0] - expected).abs() < 1e-9);
	}

	#[test]
	fn seek_past_end_then_run_returns_false() {
		let mut tone = Tone::new(440);
		tone.seek(11.0);
		let mut samples = Vec::new();
		assert_eq!(tone.fill(&mut samples, CHUNK_FRAMES), 0);
	}

	#[test]
	fn seek_to_negative_or_nan_restarts() {
		let mut tone = Tone::new(440);
		tone.seek(3.0);
		tone.seek(f64::NAN);
		let mut samples = Vec::new();
		tone.fill(&mut samples, 1);
		assert!(samples[0].abs() < 1e-12, "frame 0 of a sine is 0");
		tone.seek(-1.0);
		tone.fill(&mut samples, 1);
		assert!(samples[0].abs() < 1e-12);
	}
}
