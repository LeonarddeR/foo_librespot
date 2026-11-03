//! Filling foobar2000's `audio_chunk` with decoded PCM.

use std::marker::PhantomData;

use crate::abi::audio_chunk::AudioChunk as RawAudioChunk;
use crate::cxx::try_host;
use crate::error::Result;

/// `channel_front_center`: the WAVEFORMATEXTENSIBLE mask for mono.
pub const CHANNEL_MASK_MONO: u32 = 4;
/// `channel_front_left | channel_front_right`: the mask for stereo.
pub const CHANNEL_MASK_STEREO: u32 = 3;

/// A borrowed `audio_chunk`.
pub struct AudioChunk<'a> {
	raw: *mut RawAudioChunk,
	_borrow: PhantomData<&'a mut RawAudioChunk>,
}

impl AudioChunk<'_> {
	/// # Safety
	/// `raw` must point to an `audio_chunk` that outlives the returned value.
	pub unsafe fn from_raw(raw: *mut RawAudioChunk) -> Self {
		Self { raw, _borrow: PhantomData }
	}

	/// Replaces the chunk's contents with `frames` interleaved frames of `channels` samples each,
	/// as the SDK's `set_data` does (`SDK\audio_chunk.cpp:15-25`).
	///
	/// # Panics
	/// If `samples` holds fewer than `frames * channels` samples.
	pub fn set_data(
		&mut self,
		samples: &[f64],
		frames: usize,
		sample_rate: u32,
		channels: u32,
		mask: u32,
	) -> Result<()> {
		let count = frames * channels as usize;
		assert!(
			samples.len() >= count,
			"{} samples for {frames} frames of {channels} channels",
			samples.len()
		);
		let raw = self.raw;
		try_host(|| unsafe {
			let vtbl = &*(*raw).vtbl;
			(vtbl.set_data_size)(raw, count);
			let data = (vtbl.get_data_overload_1)(raw);
			std::ptr::copy_nonoverlapping(samples.as_ptr(), data, count);
			(vtbl.set_sample_count)(raw, frames);
			(vtbl.set_srate)(raw, sample_rate);
			(vtbl.set_channels)(raw, channels, mask);
		})?;
		Ok(())
	}
}
