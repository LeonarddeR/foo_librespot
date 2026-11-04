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

use crate::tone::{
	CHANNELS, CHUNK_FRAMES, DURATION_SECONDS, SAMPLE_RATE, Tone, ToneSpec, has_scheme, parse,
};

/// Makes foobar2000 throw one of its own C++ exceptions, by asking its filesystem about a file
/// that does not exist, and returns it as the error to throw back: proof that Foxide2k can catch
/// foobar2000's exceptions and rethrow them with their type intact.
fn host_check(abort: &Abort) -> Error {
	let Some(path) = missing_file(&host().profile_path()) else {
		return Error::new(ExceptionKind::Io, "hostcheck: foobar2000 has no profile folder");
	};
	match get_stats(&path, abort) {
		Err(error) => {
			let message = match &error.host_type {
				Some(type_name) => {
					format!("foo_oxitone: caught {type_name} from foobar2000: {}", error.message)
				}
				// Foxide2k's own error, such as no filesystem for the path: nothing was caught.
				None => format!(
					"foo_oxitone: hostcheck failed before foobar2000 threw anything: {}",
					error.message
				),
			};
			console::print(&message);
			error
		}
		Ok(_) => Error::new(ExceptionKind::Io, format!("hostcheck: {path} unexpectedly exists")),
	}
}

/// A file that doesn't exist, in the profile folder, as the URL foobar2000's filesystem takes;
/// `None` without a profile folder.
fn missing_file(profile: &str) -> Option<String> {
	if profile.is_empty() {
		return None;
	}
	let profile =
		if profile.contains("://") { profile.to_owned() } else { format!("file://{profile}") };
	Some(format!("{}\\oxitone-hostcheck-missing.wav", profile.trim_end_matches(['\\', '/'])))
}

/// The bitrate shown while a tone plays: that of CD audio, 16-bit samples at the tone's rate
/// and channels, 1411 kbit/s.
const KBPS: u32 = SAMPLE_RATE * CHANNELS * 16 / 1000;

/// One opened `oxitone://` track.
pub struct ToneInput {
	tone: Tone,
	buffer: Vec<f64>,
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
			// The buffer grows on the first decode; info reads never decode.
			Some(ToneSpec::Sine { hz }) => Ok(Self { tone: Tone::new(hz), buffer: Vec::new() }),
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
		info.info_set_int("channels", i64::from(CHANNELS))?;
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
		chunk.set_data(&self.buffer, frames, SAMPLE_RATE, CHANNELS, CHANNEL_MASK_STEREO)?;
		Ok(true)
	}

	fn decode_seek(&mut self, seconds: f64, abort: &Abort) -> Result<()> {
		abort.check()?;
		self.tone.seek(seconds);
		Ok(())
	}

	fn can_seek(&self) -> bool {
		true
	}

	/// Proves live info in a real foobar2000: the status bar shows the bitrate while a tone
	/// plays, also after a seek.
	fn live_info(&self) -> Vec<(String, String)> {
		vec![("bitrate".to_owned(), KBPS.to_string())]
	}
}

#[cfg(test)]
mod tests {
	use super::missing_file;

	#[test]
	fn the_missing_file_is_inside_the_profile_folder_as_a_file_url() {
		assert_eq!(
			missing_file(r"C:\foobar2000\profile\").as_deref(),
			Some(r"file://C:\foobar2000\profile\oxitone-hostcheck-missing.wav")
		);
		assert_eq!(
			missing_file(r"file://C:\p").as_deref(),
			Some(r"file://C:\p\oxitone-hostcheck-missing.wav")
		);
	}

	#[test]
	fn no_profile_folder_gives_no_path() {
		assert_eq!(missing_file(""), None);
	}
}
