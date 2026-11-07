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

//! `contextmenu_item` and the node tree it instantiates (`SDK\contextmenu.h:9-154`).

use std::ffi::c_void;

use crate::abi::metadb_handle::HandleListRaw;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::guid::Guid;

/// `contextmenu_item_v2`.
pub const ITEM_V2_IID: Guid = Guid::from_u128(0x4bd830ca_e3e6_404e_9544_c9b7d15a3f49);
/// `contextmenu_item`.
pub const ITEM_IID: Guid = Guid::from_u128(0x1c0802f7_cf24_49ef_b914_8b9866f19779);

/// `contextmenu_groups::root`.
pub const GROUP_ROOT: Guid = Guid::from_u128(0xe6e7dc71_114b_4848_928c_2adc3f86beb4);
/// `contextmenu_item::caller_keyboard_shortcut_list`.
pub const CALLER_KEYBOARD_SHORTCUT_LIST: Guid =
	Guid::from_u128(0xfabee3e9_8901_4df4_a2d7_b9898d86c39b);

/// `contextmenu_item_node::t_flags`.
pub const FLAG_CHECKED: u32 = 1;
pub const FLAG_DISABLED: u32 = 2;
pub const FLAG_GRAYED: u32 = 4;

/// `contextmenu_item_node::t_type`.
pub const TYPE_GROUP: i32 = 0;
pub const TYPE_COMMAND: i32 = 1;

/// `contextmenu_item::DEFAULT_ON`.
pub const ENABLED_DEFAULT_ON: i32 = 2;

/// A `contextmenu_item_node`, which is not a service: it has no `service_base` slots.
#[repr(C)]
pub struct Node {
	pub vtbl: *const NodeVtbl,
}

/// `contextmenu_item_node`'s nine slots, then the virtual destructor `contextmenu_item_node_root`
/// adds, which MSVC compiles to a scalar deleting destructor.
#[repr(C)]
pub struct NodeVtbl {
	pub get_display_data: unsafe extern "C-unwind" fn(
		this: *mut Node,
		out: *mut StringBase,
		flags: *mut u32,
		data: *const HandleListRaw,
		caller: *const Guid,
	) -> bool,
	pub get_type: unsafe extern "C-unwind" fn(this: *mut Node) -> i32,
	pub execute: unsafe extern "C-unwind" fn(
		this: *mut Node,
		data: *const HandleListRaw,
		caller: *const Guid,
	),
	pub get_glyph: unsafe extern "C-unwind" fn(
		this: *mut Node,
		data: *const HandleListRaw,
		caller: *const Guid,
	) -> *mut c_void,
	pub get_children_count: unsafe extern "C-unwind" fn(this: *mut Node) -> usize,
	pub get_child: unsafe extern "C-unwind" fn(this: *mut Node, index: usize) -> *mut Node,
	pub get_description: unsafe extern "C-unwind" fn(this: *mut Node, out: *mut StringBase) -> bool,
	pub get_guid: unsafe extern "C-unwind" fn(this: *mut Node, ret: *mut Guid) -> *mut Guid,
	pub is_mappable_shortcut: unsafe extern "C-unwind" fn(this: *mut Node) -> bool,
	/// Frees the object when `flags & 1`; returns `this`.
	pub delete: unsafe extern "C-unwind" fn(this: *mut Node, flags: u32) -> *mut c_void,
}

/// `contextmenu_item_v2`: `contextmenu_item`'s eight slots, then `get_sort_priority` and
/// `get_parent`.
#[repr(C)]
pub struct ItemVtbl {
	pub base: ServiceBaseVtbl,
	pub get_num_items: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> u32,
	pub instantiate_item: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		index: u32,
		data: *const HandleListRaw,
		caller: *const Guid,
	) -> *mut Node,
	pub get_item_guid: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		ret: *mut Guid,
		index: u32,
	) -> *mut Guid,
	pub get_item_name:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, index: u32, out: *mut StringBase),
	pub get_item_default_path:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, index: u32, out: *mut StringBase),
	pub get_item_description: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		index: u32,
		out: *mut StringBase,
	) -> bool,
	pub get_enabled_state: unsafe extern "C-unwind" fn(this: *mut ServiceBase, index: u32) -> i32,
	pub item_execute_simple: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		index: u32,
		node: *const Guid,
		data: *const HandleListRaw,
		caller: *const Guid,
	),
	pub get_sort_priority: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> f64,
	pub get_parent:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid,
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::{ItemVtbl, NodeVtbl};

	#[test]
	fn contextmenu_vtables_have_10_and_13_slots() {
		assert_eq!(size_of::<NodeVtbl>(), 10 * 8);
		assert_eq!(offset_of!(NodeVtbl, delete), 9 * 8);
		assert_eq!(size_of::<ItemVtbl>(), 13 * 8);
		assert_eq!(offset_of!(ItemVtbl, get_sort_priority), 11 * 8);
	}
}
