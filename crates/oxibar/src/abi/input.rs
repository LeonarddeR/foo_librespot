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

//! Inputs (`SDK\input.h`): `input_entry` up to v4, and the reader/decoder chain up to
//! `input_decoder_v5`.

use std::ffi::{c_char, c_void};

use crate::abi::abort_callback::AbortCallback;
use crate::abi::audio_chunk::AudioChunk;
use crate::abi::file::FileStats;
use crate::abi::file_info::FileInfo;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::guid::Guid;

pub const INPUT_ENTRY: Guid = Guid::from_u128(0x436547fc_c4ef_4322_b59e_e696a25fab2c);
pub const INPUT_ENTRY_V2: Guid = Guid::from_u128(0x7037df35_d246_4cdd_b224_08cf444ce4ce);
pub const INPUT_ENTRY_V3: Guid = Guid::from_u128(0x7099fac8_10ee_4906_a313_b98c6974e7b9);
pub const INPUT_ENTRY_V4: Guid = Guid::from_u128(0x09630c70_03c7_4a66_a936_217d804ec9d4);

pub const INFO_READER: Guid = Guid::from_u128(0x8e9bb1d4_a52b_4df6_a929_1aae4075388a);
pub const INFO_READER_V2: Guid = Guid::from_u128(0x42486bca_0d37_4e02_b772_4d30d6f850be);
pub const DECODER: Guid = Guid::from_u128(0x7eb442cd_fad7_4a26_ad7e_16f6fc89207b);
pub const DECODER_V2: Guid = Guid::from_u128(0x2c5489eb_d3c1_4ad4_bea3_cf408e161b9b);
pub const DECODER_V3: Guid = Guid::from_u128(0x953c71ed_d3a2_43f4_a33a_5594d67396e5);
pub const DECODER_V4: Guid = Guid::from_u128(0x7bbf10b5_2064_4da2_8a72_eb4ce9b3a0b4);
pub const DECODER_V5: Guid = Guid::from_u128(0xd07a8ef4_41c5_45ad_888e_db376d6c9bf0);
pub const INFO_WRITER: Guid = Guid::from_u128(0xfe40ff66_64c9_4234_b639_028dc8060cf7);

/// `service_ptr_t<T>` passed by value: MSVC passes a pointer to a temporary the caller owns, and
/// the callee destroys it, which here means releasing the pointer it holds.
pub type ByValueServicePtr = *mut *mut ServiceBase;

/// `input_entry_v4` (`SDK\input.h:232-337, 615`). `input_entry_v3` re-declares the three
/// `open_for_*` methods as overrides, which adds no slots.
#[repr(C)]
pub struct InputEntryVtbl {
	pub base: ServiceBaseVtbl,
	pub is_our_content_type:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, content_type: *const c_char) -> bool,
	pub is_our_path: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		full_path: *const c_char,
		extension: *const c_char,
	) -> bool,
	pub open_for_decoding: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		instance: *mut *mut ServiceBase,
		file_hint: ByValueServicePtr,
		path: *const c_char,
		abort: *mut AbortCallback,
	),
	pub open_for_info_read: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		instance: *mut *mut ServiceBase,
		file_hint: ByValueServicePtr,
		path: *const c_char,
		abort: *mut AbortCallback,
	),
	pub open_for_info_write: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		instance: *mut *mut ServiceBase,
		file_hint: ByValueServicePtr,
		path: *const c_char,
		abort: *mut AbortCallback,
	),
	/// Reserved by the SDK: "Do nothing". `location` is a `playable_location`, `out` a
	/// `mem_block_container`.
	pub get_extended_data: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		file_hint: ByValueServicePtr,
		location: *const c_void,
		guid: *const Guid,
		out: *mut c_void,
		abort: *mut AbortCallback,
	),
	pub get_flags: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> u32,
	/// v2. Returns a GUID by value, through the hidden return pointer after `this`.
	pub get_guid: unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid,
	pub get_name: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> *const c_char,
	pub get_preferences_guid:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid,
	pub is_low_merit: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
	/// v3. Returns a `service_ptr` by value: an owned pointer written to the uninitialised hidden
	/// return slot. Must throw `pfc::exception_not_implemented` for GUIDs it does not recognise.
	pub open_v3: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		ret: *mut *mut ServiceBase,
		what_for: *const Guid,
		hint: ByValueServicePtr,
		path: *const c_char,
		logger: ByValueServicePtr,
		abort: *mut AbortCallback,
	) -> *mut *mut ServiceBase,
	/// v4.
	pub fallback_is_our_payload: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		bytes: *const c_void,
		available: usize,
		whole: u64,
	) -> bool,
}

/// `input_decoder_v5` with everything it inherits (`SDK\input.h:36-175`).
#[repr(C)]
pub struct DecoderVtbl {
	pub base: ServiceBaseVtbl,
	// input_info_reader
	pub get_subsong_count: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> u32,
	pub get_subsong: unsafe extern "C-unwind" fn(this: *mut ServiceBase, index: u32) -> u32,
	pub get_info: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		subsong: u32,
		info: *mut FileInfo,
		abort: *mut AbortCallback,
	),
	/// Returns `t_filestats` by value, through the hidden return pointer after `this`.
	pub get_file_stats: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		ret: *mut FileStats,
		abort: *mut AbortCallback,
	) -> *mut FileStats,
	// input_decoder
	pub initialize: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		subsong: u32,
		flags: u32,
		abort: *mut AbortCallback,
	),
	/// Returns false at the end of the stream.
	pub run: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		chunk: *mut AudioChunk,
		abort: *mut AbortCallback,
	) -> bool,
	pub seek: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		seconds: f64,
		abort: *mut AbortCallback,
	),
	pub can_seek: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
	pub get_dynamic_info: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		out: *mut FileInfo,
		timestamp_delta: *mut f64,
	) -> bool,
	pub get_dynamic_info_track: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		out: *mut FileInfo,
		timestamp_delta: *mut f64,
	) -> bool,
	pub on_idle: unsafe extern "C-unwind" fn(this: *mut ServiceBase, abort: *mut AbortCallback),
	// input_decoder_v2; `raw` is a mem_block_container.
	pub run_raw: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		chunk: *mut AudioChunk,
		raw: *mut c_void,
		abort: *mut AbortCallback,
	) -> bool,
	pub set_logger: unsafe extern "C-unwind" fn(this: *mut ServiceBase, logger: ByValueServicePtr),
	// input_decoder_v3
	pub set_pause: unsafe extern "C-unwind" fn(this: *mut ServiceBase, paused: bool),
	pub flush_on_pause: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
	// input_decoder_v4
	pub extended_param: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		param_type: *const Guid,
		arg1: usize,
		arg2: *mut c_void,
		arg2_size: usize,
	) -> usize,
	// input_decoder_v5; `ret` is a pfc::list_base_t<input_live_info_t>.
	pub poll_live_info: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		ret: *mut c_void,
		abort: *mut AbortCallback,
	),
}

#[cfg(test)]
mod tests {
	use super::{DecoderVtbl, InputEntryVtbl};

	#[test]
	fn input_entry_vtable_has_16_slots() {
		assert_eq!(size_of::<InputEntryVtbl>(), 16 * 8);
	}

	#[test]
	fn decoder_vtable_has_20_slots() {
		assert_eq!(size_of::<DecoderVtbl>(), 20 * 8);
	}
}
