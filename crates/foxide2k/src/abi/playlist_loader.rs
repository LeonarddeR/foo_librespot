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

//! `playlist_loader`, `playlist_loader_callback` (`SDK\playlist_loader.h`) and
//! `playable_location` (`SDK\playable_location.h`).

use std::ffi::{c_char, c_void};

use crate::abi::abort_callback::AbortCallback;
use crate::abi::file::FileStats;
use crate::abi::file_info::FileInfo;
use crate::abi::input::ByValueServicePtr;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::guid::Guid;

/// `playlist_loader` (`SDK\playlist_loader.h:67-124`).
pub const IID: Guid = Guid::from_u128(0x7103cae9_091c_4d80_bc1d_284ac13f1e8c);
/// `playlist_loader_callback` (`SDK\playlist_loader.h:5-49`).
pub const CALLBACK_IID: Guid = Guid::from_u128(0x924470a0_1478_4141_a75a_c52f1ffaefea);

/// `playlist_loader_callback::t_entry_type`.
pub const ENTRY_USER_REQUESTED: u32 = 0;
pub const ENTRY_DIRECTORY_ENUMERATED: u32 = 1;
pub const ENTRY_FROM_PLAYLIST: u32 = 2;

#[repr(C)]
pub struct PlaylistLoaderVtbl {
	pub base: ServiceBaseVtbl,
	/// `file` is a `const service_ptr_t<file> &` (it may hold null); `callback` is passed by
	/// value, so the callee releases it.
	pub open: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		path: *const c_char,
		file: *const *mut ServiceBase,
		callback: ByValueServicePtr,
		abort: *mut AbortCallback,
	),
	/// `handles` is a `metadb_handle_list_cref`, opaque here.
	pub write: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		path: *const c_char,
		file: *const *mut ServiceBase,
		handles: *const c_void,
		abort: *mut AbortCallback,
	),
	pub get_extension: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> *const c_char,
	pub can_write: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
	pub is_our_content_type:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, content_type: *const c_char) -> bool,
	pub is_associatable: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
}

/// Every `metadb_handle_ptr` argument is a `const metadb_handle_ptr &`: a pointer to the
/// caller's pointer, which keeps its reference.
#[repr(C)]
pub struct PlaylistLoaderCallbackVtbl {
	pub base: ServiceBaseVtbl,
	pub on_progress: unsafe extern "C-unwind" fn(this: *mut ServiceBase, path: *const c_char),
	pub on_entry: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		handle: *const *mut ServiceBase,
		entry_type: u32,
		stats: *const FileStats,
		fresh: bool,
	),
	pub want_info: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		handle: *const *mut ServiceBase,
		entry_type: u32,
		stats: *const FileStats,
		fresh: bool,
	) -> bool,
	pub on_entry_info: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		handle: *const *mut ServiceBase,
		entry_type: u32,
		stats: *const FileStats,
		info: *const FileInfo,
		fresh: bool,
	),
	/// `out` is a `metadb_handle_ptr &`; the new handle's reference belongs to the caller.
	pub handle_create: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		out: *mut *mut ServiceBase,
		location: *const PlayableLocation,
	),
	pub is_path_wanted: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		path: *const c_char,
		entry_type: u32,
	) -> bool,
	pub want_browse_info: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		handle: *const *mut ServiceBase,
		entry_type: u32,
		timestamp: u64,
	) -> bool,
	pub on_browse_info: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		handle: *const *mut ServiceBase,
		entry_type: u32,
		info: *const FileInfo,
		timestamp: u64,
	),
}

/// `playable_location`: not a service, just an interface with four virtuals and no destructor
/// (`SDK\playable_location.h:9-15`). Passed as `const playable_location &`, like the SDK's
/// `make_playable_location`.
#[repr(C)]
pub struct PlayableLocation {
	pub vtbl: *const PlayableLocationVtbl,
	pub path: *const c_char,
	pub subsong: u32,
}

#[repr(C)]
pub struct PlayableLocationVtbl {
	pub get_path: unsafe extern "C-unwind" fn(this: *const PlayableLocation) -> *const c_char,
	pub set_path: unsafe extern "C-unwind" fn(this: *mut PlayableLocation, path: *const c_char),
	pub get_subsong: unsafe extern "C-unwind" fn(this: *const PlayableLocation) -> u32,
	pub set_subsong: unsafe extern "C-unwind" fn(this: *mut PlayableLocation, subsong: u32),
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::{PlayableLocationVtbl, PlaylistLoaderCallbackVtbl, PlaylistLoaderVtbl};

	#[test]
	fn loader_vtbls_have_the_sdk_slot_counts() {
		assert_eq!(size_of::<PlaylistLoaderVtbl>(), 9 * 8);
		assert_eq!(offset_of!(PlaylistLoaderVtbl, open), 3 * 8);
		assert_eq!(offset_of!(PlaylistLoaderVtbl, is_our_content_type), 7 * 8);
		assert_eq!(size_of::<PlaylistLoaderCallbackVtbl>(), 11 * 8);
		assert_eq!(offset_of!(PlaylistLoaderCallbackVtbl, on_entry_info), 6 * 8);
		assert_eq!(offset_of!(PlaylistLoaderCallbackVtbl, handle_create), 7 * 8);
		assert_eq!(size_of::<PlayableLocationVtbl>(), 4 * 8);
	}
}
