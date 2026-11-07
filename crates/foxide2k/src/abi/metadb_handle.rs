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

//! `metadb_handle` (`SDK\metadb_handle.h:39-69`), of which only `get_location` and `get_info`
//! are used.

use std::ffi::c_void;

use crate::abi::file::FileStats;
use crate::abi::file_info::FileInfo as RawFileInfo;
use crate::abi::pfc_list::ListBaseConst;
use crate::abi::playlist_loader::PlayableLocation;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;

/// `metadb_handle_list_cref`: a `pfc::list_base_const_t<metadb_handle_ptr>`. A `metadb_handle_ptr`
/// is a single owning pointer.
pub type HandleListRaw = ListBaseConst<*mut ServiceBase>;

/// `metadb_handle`'s slots up to `get_info`, in declaration order.
#[repr(C)]
pub struct MetadbHandleVtbl {
	pub base: ServiceBaseVtbl,
	/// `const playable_location & get_location() const`.
	pub get_location:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> *const PlayableLocation,
	/// `bool format_title(titleformat_hook * p_hook, pfc::string_base & p_out,
	/// const service_ptr_t<titleformat_object> & p_script, titleformat_text_filter * p_filter)`.
	/// The hook and the filter are opaque here. Not called.
	pub format_title: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		hook: *mut c_void,
		out: *mut StringBase,
		script: *const *mut ServiceBase,
		filter: *mut c_void,
	) -> bool,
	/// `void metadb_lock()`, obsolete. Not called.
	pub metadb_lock: unsafe extern "C-unwind" fn(this: *mut ServiceBase),
	/// `void metadb_unlock()`, obsolete. Not called.
	pub metadb_unlock: unsafe extern "C-unwind" fn(this: *mut ServiceBase),
	/// `t_filestats get_filestats() const`. Returns `t_filestats` by value, through the hidden
	/// return pointer after `this`. Not called.
	pub get_filestats:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut FileStats) -> *mut FileStats,
	/// `bool is_info_loaded() const`. Not called.
	pub is_info_loaded: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
	/// `bool get_info(file_info & p_info) const`: copies the cached info into `info`. Returns
	/// false when the info is not known yet.
	pub get_info:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, info: *mut RawFileInfo) -> bool,
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::MetadbHandleVtbl;

	#[test]
	fn get_location_follows_the_service_base_slots() {
		assert_eq!(offset_of!(MetadbHandleVtbl, get_location), 3 * 8);
	}

	#[test]
	fn get_info_is_the_tenth_slot() {
		assert_eq!(offset_of!(MetadbHandleVtbl, get_info), 9 * 8);
	}
}
