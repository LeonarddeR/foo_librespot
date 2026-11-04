//! The foobar2000 input that plays `oxitone://` paths.

use std::ffi::CStr;

use foxide2k::abort::Abort;
use foxide2k::audio_chunk::{AudioChunk, CHANNEL_MASK_STEREO};
use foxide2k::console;
use foxide2k::cxx::ExceptionKind;
use foxide2k::error::{Error, Result};
use foxide2k::file_info::FileInfo;
use foxide2k::filesystem::get_stats;
use foxide2k::guid::Guid;
use foxide2k::host::host;
use foxide2k::input::{Input, OpenReason};

use crate::tone::{CHUNK_FRAMES, DURATION_SECONDS, SAMPLE_RATE, Tone, ToneSpec, has_scheme, parse};

/// Makes foobar2000 throw one of its own C++ exceptions, by asking its filesystem about a file
/// that does not exist, and returns it as the error to throw back: proof that Foxide2k can catch
/// foobar2000's exceptions and rethrow them with their type intact.
fn host_check(abort: &Abort) -> Error {
	let profile = host().profile_path();
	let profile = if profile.contains("://") { profile } else { format!("file://{profile}") };
	let path = format!("{}\\oxitone-hostcheck-missing.wav", profile.trim_end_matches(['\\', '/']));
	match get_stats(&path, abort) {
		Err(error) => {
			let type_name = error.host_type.as_deref().unwrap_or("an exception");
			console::print(&format!(
				"foo_oxitone: caught {type_name} from foobar2000: {}",
				error.message
			));
			error
		}
		Ok(_) => Error::new(ExceptionKind::Io, format!("hostcheck: {path} unexpectedly exists")),
	}
}

/// The bitrate shown while a tone plays: that of CD audio, as the tone plays at 44.1 kHz stereo.
const KBPS: &str = "1411";

/// One opened `oxitone://` track.
pub struct ToneInput {
	tone: Tone,
	buffer: Vec<f64>,
	/// Whether foobar2000 has been told the bitrate since decoding started or last seeked.
	reported: bool,
}

impl Input for ToneInput {
	const GUID: Guid = Guid::from_u128(0x7d3f2a90_5c1e_4b8a_9f62_0a4e1c9d3b57);
	const NAME: &'static CStr = c"foo_oxitone test tone";

	fn is_our_path(path: &str) -> bool {
		// Claim every oxitone:// path, so an invalid one reaches open and gets a clear error.
		has_scheme(path)
	}

	fn open(path: &str, _reason: OpenReason, abort: &Abort) -> Result<Self> {
		match parse(path) {
			Some(ToneSpec::Sine { hz }) => Ok(Self {
				tone: Tone::new(hz),
				buffer: Vec::with_capacity(CHUNK_FRAMES * 2),
				reported: false,
			}),
			Some(ToneSpec::HostCheck) => Err(host_check(abort)),
			#[cfg(feature = "test-panic")]
			Some(ToneSpec::Panic) => panic!("oxitone://panic was opened"),
			None => Err(Error::unsupported_format(format!("not a valid oxitone:// path: {path}"))),
		}
	}

	fn get_info(&mut self, info: &mut FileInfo<'_>, _abort: &Abort) -> Result<()> {
		info.meta_set("title", &format!("Test tone {} Hz", self.tone.hz()))?;
		info.meta_set("artist", "foo_oxitone")?;
		info.set_length(f64::from(DURATION_SECONDS))?;
		info.info_set_int("samplerate", i64::from(SAMPLE_RATE))?;
		info.info_set_int("channels", 2)?;
		info.info_set("codec", "Sine")?;
		info.info_set("encoding", "lossless")
	}

	fn decode_initialize(&mut self, _flags: u32, _abort: &Abort) -> Result<()> {
		self.tone = Tone::new(self.tone.hz());
		Ok(())
	}

	fn decode_run(&mut self, chunk: &mut AudioChunk<'_>, abort: &Abort) -> Result<bool> {
		abort.check()?;
		let frames = self.tone.fill(&mut self.buffer, CHUNK_FRAMES);
		if frames == 0 {
			return Ok(false);
		}
		chunk.set_data(&self.buffer, frames, SAMPLE_RATE, 2, CHANNEL_MASK_STEREO)?;
		Ok(true)
	}

	fn decode_seek(&mut self, seconds: f64, abort: &Abort) -> Result<()> {
		abort.check()?;
		self.tone.seek(seconds);
		self.reported = false;
		Ok(())
	}

	fn can_seek(&self) -> bool {
		true
	}

	/// Proves live info in a real foobar2000: the status bar shows the bitrate while a tone
	/// plays, also after a seek.
	fn live_info(&mut self) -> Vec<(String, String)> {
		if std::mem::replace(&mut self.reported, true) {
			return Vec::new();
		}
		vec![("bitrate".to_owned(), KBPS.to_owned())]
	}
}
