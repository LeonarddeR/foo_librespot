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

//! `playlist_manager` and `playlist_manager_v2` (`SDK\playlist.h:81-245, 406-452`),
//! `playlist_incoming_item_filter_v2` and `process_locations_notify` (`SDK\playlist.h:775-860`),
//! implemented by foobar2000. Main thread only.

use std::ffi::{c_char, c_void};

use crate::abi::abort_callback::AbortCallback;
use crate::abi::input::ByValueServicePtr;
use crate::abi::metadb_handle::HandleListRaw;
use crate::abi::pfc_list::{BitArray, ListBaseConst};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl, Unused};
use crate::guid::Guid;

pub const MANAGER_IID: Guid = Guid::from_u128(0xd8d51855_d79d_49b8_97a8_065785fa2426);
pub const MANAGER_V2_IID: Guid = Guid::from_u128(0x7fe9052a_8ec2_4054_a8cf_77efb52fe408);
pub const INCOMING_IID: Guid = Guid::from_u128(0xeb8fa333_f365_46ad_8621_345671567baa);
pub const INCOMING_V2_IID: Guid = Guid::from_u128(0xf335802b_a522_434d_be89_e86d59561648);
pub const NOTIFY_IID: Guid = Guid::from_u128(0x7d7eddac_f93e_4707_969b_cf44a9e1fb78);

/// `playlist_manager`'s 54 slots after `service_base`, then `playlist_manager_v2`'s first two.
/// Only the slots used are typed; the others are [`Unused`].
#[repr(C)]
pub struct PlaylistManagerVtbl {
	pub base: ServiceBaseVtbl,
	pub get_playlist_count: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> usize,
	/// `get_active_playlist`.
	pub unused_4: Unused<1>,
	pub set_active_playlist: unsafe extern "C-unwind" fn(this: *mut ServiceBase, index: usize),
	/// `get_playing_playlist`, `set_playing_playlist` and `remove_playlists`.
	pub unused_6_8: Unused<3>,
	/// Returns the index of the new playlist, infinite on failure. A `length` of infinite reads
	/// `name` as null-terminated.
	pub create_playlist: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		name: *const c_char,
		length: usize,
		index: usize,
	) -> usize,
	/// `reorder`, `playlist_get_item_count`, `playlist_enum_items`, `playlist_get_focus_item`,
	/// `playlist_get_name`, `playlist_reorder_items`, `playlist_set_selection`,
	/// `playlist_remove_items`, `playlist_replace_item` and `playlist_set_focus_item`.
	pub unused_10_19: Unused<10>,
	/// Inserts `data` at `base`, infinite for the end. Bit `n` of `selection` selects item `n` of
	/// `data`.
	pub playlist_insert_items: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		playlist: usize,
		base: usize,
		data: *const HandleListRaw,
		selection: *const BitArray,
	) -> usize,
	/// `playlist_ensure_visible` through `playlist_execute_default_action`: renaming, undo,
	/// title formatting, sorting, locks, playback order, the queue and the callbacks.
	pub unused_21_56: Unused<36>,
	/// `playlist_manager_v2::playlist_set_property`. `stream` is a `stream_reader *`.
	pub playlist_set_property: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		playlist: usize,
		property: *const Guid,
		stream: *mut c_void,
		size_hint: usize,
		abort: *mut AbortCallback,
	),
	/// `playlist_manager_v2::playlist_get_property`. `stream` is a `stream_writer *`. Returns
	/// whether the property exists.
	pub playlist_get_property: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		playlist: usize,
		property: *const Guid,
		stream: *mut c_void,
		abort: *mut AbortCallback,
	) -> bool,
}

/// `playlist_incoming_item_filter_v2`: `playlist_incoming_item_filter`'s eight slots, then
/// `process_locations_async`.
#[repr(C)]
pub struct IncomingItemFilterV2Vtbl {
	pub base: ServiceBaseVtbl,
	/// `filter_items`, `process_locations`, `process_dropped_files`,
	/// `process_dropped_files_check`, `process_dropped_files_check_if_native`,
	/// `create_dataobject`, `process_dropped_files_check_ex` and `process_dropped_files_delayed`.
	pub unused_3_10: Unused<8>,
	/// Returns at once; `notify` hears the outcome. `notify` is passed by value.
	pub process_locations_async: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		urls: *const ListBaseConst<*const c_char>,
		op_flags: u32,
		restrict: *const c_char,
		exclude: *const c_char,
		parent: *mut c_void,
		notify: ByValueServicePtr,
	),
}

/// `process_locations_notify`, implemented by the component.
#[repr(C)]
pub struct ProcessLocationsNotifyVtbl {
	pub base: ServiceBaseVtbl,
	pub on_completion:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, items: *const HandleListRaw),
	pub on_aborted: unsafe extern "C-unwind" fn(this: *mut ServiceBase),
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::{IncomingItemFilterV2Vtbl, PlaylistManagerVtbl, ProcessLocationsNotifyVtbl};

	#[test]
	fn playlist_vtbls_have_their_sdk_slots() {
		assert_eq!(size_of::<PlaylistManagerVtbl>(), 59 * 8);
		assert_eq!(offset_of!(PlaylistManagerVtbl, set_active_playlist), 5 * 8);
		assert_eq!(offset_of!(PlaylistManagerVtbl, create_playlist), 9 * 8);
		assert_eq!(offset_of!(PlaylistManagerVtbl, playlist_insert_items), 20 * 8);
		assert_eq!(offset_of!(PlaylistManagerVtbl, playlist_set_property), 57 * 8);
		assert_eq!(offset_of!(PlaylistManagerVtbl, playlist_get_property), 58 * 8);
		assert_eq!(offset_of!(IncomingItemFilterV2Vtbl, process_locations_async), 11 * 8);
		assert_eq!(size_of::<ProcessLocationsNotifyVtbl>(), 5 * 8);
	}
}
