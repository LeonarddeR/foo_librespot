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

//! `file_info` (`SDK\file_info.h:107-370`): track metadata, technical info, length and
//! ReplayGain. foobar2000 passes its own implementation; no virtual destructor.

use std::ffi::c_char;
use std::ffi::c_void;

/// `replaygain_info` (`SDK\file_info.h:6-11`). Gains of -1000 and peaks of -1 mean "not set".
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplayGainInfo {
	pub album_gain: f32,
	pub track_gain: f32,
	pub album_peak: f32,
	pub track_peak: f32,
}

#[repr(C)]
pub struct FileInfo {
	pub vtbl: *const FileInfoVtbl,
}

/// In declaration order; the two protected `*_nocheck_ex` virtuals are declared last. Lengths of
/// `usize::MAX` mean NUL-terminated. `bit_array` arguments are opaque here.
#[repr(C)]
pub struct FileInfoVtbl {
	pub get_length: unsafe extern "C-unwind" fn(this: *const FileInfo) -> f64,
	pub set_length: unsafe extern "C-unwind" fn(this: *mut FileInfo, length: f64),
	pub set_replaygain:
		unsafe extern "C-unwind" fn(this: *mut FileInfo, info: *const ReplayGainInfo),
	/// Returns `replaygain_info` by value, through the hidden return pointer after `this`.
	pub get_replaygain: unsafe extern "C-unwind" fn(
		this: *const FileInfo,
		ret: *mut ReplayGainInfo,
	) -> *mut ReplayGainInfo,
	pub meta_get_count: unsafe extern "C-unwind" fn(this: *const FileInfo) -> usize,
	pub meta_enum_name:
		unsafe extern "C-unwind" fn(this: *const FileInfo, index: usize) -> *const c_char,
	pub meta_enum_value_count:
		unsafe extern "C-unwind" fn(this: *const FileInfo, index: usize) -> usize,
	pub meta_enum_value: unsafe extern "C-unwind" fn(
		this: *const FileInfo,
		index: usize,
		value_index: usize,
	) -> *const c_char,
	/// Returns `usize::MAX` when the field does not exist.
	pub meta_find_ex: unsafe extern "C-unwind" fn(
		this: *const FileInfo,
		name: *const c_char,
		name_length: usize,
	) -> usize,
	pub meta_set_ex: unsafe extern "C-unwind" fn(
		this: *mut FileInfo,
		name: *const c_char,
		name_length: usize,
		value: *const c_char,
		value_length: usize,
	) -> usize,
	pub meta_insert_value_ex: unsafe extern "C-unwind" fn(
		this: *mut FileInfo,
		index: usize,
		value_index: usize,
		value: *const c_char,
		value_length: usize,
	),
	pub meta_remove_mask: unsafe extern "C-unwind" fn(this: *mut FileInfo, mask: *const c_void),
	pub meta_reorder: unsafe extern "C-unwind" fn(this: *mut FileInfo, order: *const usize),
	pub meta_remove_values:
		unsafe extern "C-unwind" fn(this: *mut FileInfo, index: usize, mask: *const c_void),
	pub meta_modify_value_ex: unsafe extern "C-unwind" fn(
		this: *mut FileInfo,
		index: usize,
		value_index: usize,
		value: *const c_char,
		value_length: usize,
	),
	pub info_get_count: unsafe extern "C-unwind" fn(this: *const FileInfo) -> usize,
	pub info_enum_name:
		unsafe extern "C-unwind" fn(this: *const FileInfo, index: usize) -> *const c_char,
	pub info_enum_value:
		unsafe extern "C-unwind" fn(this: *const FileInfo, index: usize) -> *const c_char,
	pub info_set_ex: unsafe extern "C-unwind" fn(
		this: *mut FileInfo,
		name: *const c_char,
		name_length: usize,
		value: *const c_char,
		value_length: usize,
	) -> usize,
	pub info_remove_mask: unsafe extern "C-unwind" fn(this: *mut FileInfo, mask: *const c_void),
	pub info_find_ex: unsafe extern "C-unwind" fn(
		this: *const FileInfo,
		name: *const c_char,
		name_length: usize,
	) -> usize,
	pub copy: unsafe extern "C-unwind" fn(this: *mut FileInfo, source: *const FileInfo),
	pub copy_meta: unsafe extern "C-unwind" fn(this: *mut FileInfo, source: *const FileInfo),
	pub copy_info: unsafe extern "C-unwind" fn(this: *mut FileInfo, source: *const FileInfo),
	pub meta_set_nocheck_ex: unsafe extern "C-unwind" fn(
		this: *mut FileInfo,
		name: *const c_char,
		name_length: usize,
		value: *const c_char,
		value_length: usize,
	) -> usize,
	pub info_set_nocheck_ex: unsafe extern "C-unwind" fn(
		this: *mut FileInfo,
		name: *const c_char,
		name_length: usize,
		value: *const c_char,
		value_length: usize,
	) -> usize,
}

#[cfg(test)]
mod tests {
	use super::{FileInfoVtbl, ReplayGainInfo};

	#[test]
	fn file_info_vtable_has_26_slots() {
		assert_eq!(size_of::<FileInfoVtbl>(), 26 * 8);
	}

	#[test]
	fn replaygain_info_is_16_bytes() {
		assert_eq!(size_of::<ReplayGainInfo>(), 16);
	}
}
