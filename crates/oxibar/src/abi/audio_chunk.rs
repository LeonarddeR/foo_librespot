// Interface definitions in this file are transcribed from the foobar2000 2.x SDK.
//
// foobar2000 2.x SDK
// Copyright (c), Peter Pawlowski
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without modification, are permitted
// provided that the following conditions are met:
//
// Redistributions of source code must retain the above copyright notice, this list of conditions
// and the following disclaimer.
// Neither the name of the author nor the names of its contributors may be used to endorse or
// promote products derived from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
// IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
// FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE AUTHORS OR CONTRIBUTORS BE
// LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
// (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
// PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
// CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF
// THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! `audio_chunk` (`SDK\audio_chunk.h:94-125`): a block of decoded PCM. On x64 an
//! `audio_sample` is a `double` (`shared\audio_math.h:4-15`); no virtual destructor.

#[repr(C)]
pub struct AudioChunk {
	pub vtbl: *const AudioChunkVtbl,
}

/// `get_data()` is overloaded (`const` and non-`const`), and MSVC's order for overloads is not
/// declaration order. foobar2000's `audio_chunk_impl_t` returns the same buffer from both, so
/// either slot gives the sample buffer.
#[repr(C)]
pub struct AudioChunkVtbl {
	pub get_data_overload_0: unsafe extern "C-unwind" fn(this: *mut AudioChunk) -> *mut f64,
	pub get_data_overload_1: unsafe extern "C-unwind" fn(this: *mut AudioChunk) -> *mut f64,
	/// In samples (frames × channels), not bytes.
	pub get_data_size: unsafe extern "C-unwind" fn(this: *const AudioChunk) -> usize,
	pub set_data_size: unsafe extern "C-unwind" fn(this: *mut AudioChunk, size: usize),
	pub get_srate: unsafe extern "C-unwind" fn(this: *const AudioChunk) -> u32,
	pub set_srate: unsafe extern "C-unwind" fn(this: *mut AudioChunk, rate: u32),
	pub get_channels: unsafe extern "C-unwind" fn(this: *const AudioChunk) -> u32,
	pub get_channel_config: unsafe extern "C-unwind" fn(this: *const AudioChunk) -> u32,
	/// `config` is a WAVEFORMATEXTENSIBLE channel mask.
	pub set_channels: unsafe extern "C-unwind" fn(this: *mut AudioChunk, count: u32, config: u32),
	/// In frames (samples per channel).
	pub get_sample_count: unsafe extern "C-unwind" fn(this: *const AudioChunk) -> usize,
	pub set_sample_count: unsafe extern "C-unwind" fn(this: *mut AudioChunk, count: usize),
}

#[cfg(test)]
mod tests {
	use super::AudioChunkVtbl;

	#[test]
	fn audio_chunk_vtable_has_11_slots() {
		assert_eq!(size_of::<AudioChunkVtbl>(), 11 * 8);
	}
}
