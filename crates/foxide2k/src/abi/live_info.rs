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

//! `input_live_info_t` and `input_decoder_v5::poll_live_info` (`SDK\input.h:152-176`), with
//! `file_info_filter` (`SDK\file_info_filter.h:5-16`), the payload that carries a change of
//! technical info such as the bitrate.

use crate::abi::file::FileStats;
use crate::abi::file_info::FileInfo;
use crate::abi::input::ByValueServicePtr;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::guid::Guid;
use crate::service::ServicePtr;

/// `file_info_filter` (`SDK\guids.cpp:1164`).
pub const FILE_INFO_FILTER_IID: Guid = Guid::from_u128(0x45d0b800_de83_4a77_ad34_3f842d40e795);

/// `input_live_info_t::scope_info`: technical info changed mid-track, such as a VBR bitrate.
pub const SCOPE_INFO: u32 = 1;

/// `input_live_info_t`. `payload` is a `service_ptr`: whoever holds the struct owns a reference.
#[repr(C)]
#[derive(Debug)]
pub struct InputLiveInfo {
	pub payload: *mut ServiceBase,
	/// `scope_none` (0) means "no change".
	pub scope: u32,
	/// When the change shows, in seconds from the start of the last chunk returned.
	pub delta_time: f64,
}

impl Default for InputLiveInfo {
	fn default() -> Self {
		Self { payload: std::ptr::null_mut(), scope: 0, delta_time: 0.0 }
	}
}

impl InputLiveInfo {
	/// Copy assignment, as `input_live_info_t`'s: takes a reference to `from`'s payload and
	/// releases the one held.
	///
	/// # Safety
	/// Both payloads must be null or point to live services.
	pub unsafe fn assign_from(&mut self, from: &Self) {
		let added = unsafe { ServicePtr::from_raw_add_ref(from.payload) }.map(ServicePtr::into_raw);
		let old = std::mem::replace(&mut self.payload, added.unwrap_or(std::ptr::null_mut()));
		drop(unsafe { ServicePtr::from_raw_owned(old) });
		self.scope = from.scope;
		self.delta_time = from.delta_time;
	}
}

/// `file_info_filter`. `apply_filter` takes `trackRef` (a `metadb_handle_ptr`, which may be
/// null) and `t_filestats` by value: the handle as a pointer to the caller's temporary, which
/// the callee releases, and the 16-byte stats as a pointer to the caller's copy. It returns
/// true when it changed `info`.
#[repr(C)]
pub struct FileInfoFilterVtbl {
	pub base: ServiceBaseVtbl,
	pub apply_filter: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		location: ByValueServicePtr,
		stats: *const FileStats,
		info: *mut FileInfo,
	) -> bool,
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::{FileInfoFilterVtbl, InputLiveInfo, SCOPE_INFO};

	#[test]
	fn live_info_matches_the_sdk_layout() {
		assert_eq!(size_of::<InputLiveInfo>(), 24);
		assert_eq!(offset_of!(InputLiveInfo, payload), 0);
		assert_eq!(offset_of!(InputLiveInfo, scope), 8);
		assert_eq!(offset_of!(InputLiveInfo, delta_time), 16);
		assert_eq!(SCOPE_INFO, 1);
		assert_eq!(size_of::<FileInfoFilterVtbl>(), 4 * 8);
	}
}
