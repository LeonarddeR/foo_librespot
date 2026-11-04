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

//! `album_art_extractor`, `album_art_extractor_instance` (`SDK\album_art.h`) and
//! `album_art_data`, which is `fb2k::memBlock` (`SDK\commonObjects.h:171-175`).

use std::ffi::{c_char, c_void};

use crate::abi::abort_callback::AbortCallback;
use crate::abi::input::ByValueServicePtr;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::guid::Guid;

pub const EXTRACTOR_IID: Guid = Guid::from_u128(0xe19ca3d3_6267_4271_b789_6a3f87fb3ebf);
pub const EXTRACTOR_V2_IID: Guid = Guid::from_u128(0x3aa31001_af5b_497a_bddc_a93f23b21bc2);
pub const INSTANCE_IID: Guid = Guid::from_u128(0xf673700e_3b6e_4f70_a106_ab745c202060);
/// `fb2k::memBlock`, which `album_art_data` is a typedef of.
pub const DATA_IID: Guid = Guid::from_u128(0x9ddce05c_aa3f_4565_b33a_bd6adcdd9037);

/// The kinds of picture a track can have (`SDK\album_art.h:15-23`).
pub const COVER_FRONT: Guid = Guid::from_u128(0xf1e66f4e_fe09_4b94_91a3_67c23ed1445e);
pub const COVER_BACK: Guid = Guid::from_u128(0xcb552d19_86d5_434c_ac77_bb24ed567ee4);
pub const DISC: Guid = Guid::from_u128(0x3dba9f36_f928_4fa4_879c_d3404759587e);
pub const ICON: Guid = Guid::from_u128(0x74cdf5b4_7053_4b3d_9a3c_5469f5826eec);
pub const ARTIST: Guid = Guid::from_u128(0x9a654042_acd1_43f7_bfcf_d3ec0ffe40fa);

/// `album_art_extractor_v2`, which adds `get_guid` to `album_art_extractor`. foobar2000 pairs an
/// extractor with the input whose GUID it returns. Both by-value returns go through a hidden
/// pointer after `this`.
#[repr(C)]
pub struct AlbumArtExtractorVtbl {
	pub base: ServiceBaseVtbl,
	pub is_our_path: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		path: *const c_char,
		extension: *const c_char,
	) -> bool,
	/// Returns `album_art_extractor_instance_ptr`; `file` is a `file_ptr` passed by value.
	pub open: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		ret: *mut *mut ServiceBase,
		file: ByValueServicePtr,
		path: *const c_char,
		abort: *mut AbortCallback,
	) -> *mut *mut ServiceBase,
	pub get_guid: unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid,
}

#[repr(C)]
pub struct AlbumArtExtractorInstanceVtbl {
	pub base: ServiceBaseVtbl,
	/// Returns `album_art_data_ptr`; throws `exception_album_art_not_found` when there is none.
	pub query: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		ret: *mut *mut ServiceBase,
		what: *const Guid,
		abort: *mut AbortCallback,
	) -> *mut *mut ServiceBase,
}

#[repr(C)]
pub struct MemBlockVtbl {
	pub base: ServiceBaseVtbl,
	pub data: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> *const c_void,
	pub size: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> usize,
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::{AlbumArtExtractorInstanceVtbl, AlbumArtExtractorVtbl, MemBlockVtbl};

	#[test]
	fn album_art_vtbls_have_the_sdk_slot_counts() {
		assert_eq!(size_of::<AlbumArtExtractorVtbl>(), 6 * 8);
		assert_eq!(offset_of!(AlbumArtExtractorVtbl, open), 4 * 8);
		assert_eq!(offset_of!(AlbumArtExtractorVtbl, get_guid), 5 * 8);
		assert_eq!(size_of::<AlbumArtExtractorInstanceVtbl>(), 4 * 8);
		assert_eq!(size_of::<MemBlockVtbl>(), 5 * 8);
	}
}
