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

//! `mainmenu_group`, `mainmenu_group_popup` and `mainmenu_commands` (`SDK\menu.h:4-66`), and
//! `mainmenu_node` and `mainmenu_commands_v2`, whose entries are built as the menu opens
//! (`SDK\menu.h:170-238`).

use crate::abi::input::ByValueServicePtr;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::guid::Guid;

pub const GROUP_IID: Guid = Guid::from_u128(0x21656ab9_0255_4f8c_8fb9_1a7748bce93a);
pub const GROUP_POPUP_IID: Guid = Guid::from_u128(0x2e673a2e_a4ee_419c_94c8_9c838660414c);
pub const COMMANDS_IID: Guid = Guid::from_u128(0x35077b8c_6e70_47ba_b9dd_d51500e12f2e);
/// `mainmenu_commands_v2`.
pub const COMMANDS_V2_IID: Guid = Guid::from_u128(0xe682e810_41f3_434d_b0c7_d49690e65f87);
/// `mainmenu_node`.
pub const NODE_IID: Guid = Guid::from_u128(0xabba6512_377d_414f_813e_6806c22d4db1);

/// `mainmenu_groups::file`, the File menu.
pub const GROUP_FILE: Guid = Guid::from_u128(0x8f487f1f_419f_47a7_8ecf_ec44af4449a3);
/// `mainmenu_groups::view`, the View menu.
pub const GROUP_VIEW: Guid = Guid::from_u128(0xf8be5604_165f_4bb9_b6a9_15e55e0e0d3a);

/// `mainmenu_commands::sort_priority_base`: components sort at this and above.
pub const SORT_PRIORITY_BASE: u32 = 0x10000;

/// `mainmenu_commands::flag_disabled`: greyed out, and it can't be picked. Nodes use the same
/// flags.
pub const FLAG_DISABLED: u32 = 1;

/// `mainmenu_node::type_group`.
pub const TYPE_GROUP: u32 = 0;
/// `mainmenu_node::type_command`.
pub const TYPE_COMMAND: u32 = 1;

/// `mainmenu_group_popup`, which adds `get_display_string` to `mainmenu_group`'s three slots.
#[repr(C)]
pub struct GroupPopupVtbl {
	pub base: ServiceBaseVtbl,
	pub get_guid: unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid,
	pub get_parent:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid,
	pub get_sort_priority: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> u32,
	pub get_display_string:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, out: *mut StringBase),
}

/// `mainmenu_commands`.
#[repr(C)]
pub struct CommandsVtbl {
	pub base: ServiceBaseVtbl,
	pub get_command_count: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> u32,
	pub get_command: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		ret: *mut Guid,
		index: u32,
	) -> *mut Guid,
	pub get_name:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, index: u32, out: *mut StringBase),
	pub get_description: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		index: u32,
		out: *mut StringBase,
	) -> bool,
	pub get_parent:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid,
	pub get_sort_priority: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> u32,
	pub get_display: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		index: u32,
		text: *mut StringBase,
		flags: *mut u32,
	) -> bool,
	/// `execute(index, service_ptr callback)`, the pointer passed by value.
	pub execute: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		index: u32,
		callback: ByValueServicePtr,
	),
}

/// `mainmenu_commands_v2`: `mainmenu_commands`' eleven slots, then three for entries built as the
/// menu opens.
#[repr(C)]
pub struct CommandsV2Vtbl {
	pub base: CommandsVtbl,
	pub is_command_dynamic: unsafe extern "C-unwind" fn(this: *mut ServiceBase, index: u32) -> bool,
	/// Returns `mainmenu_node::ptr` through a hidden pointer.
	pub dynamic_instantiate: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		ret: *mut *mut ServiceBase,
		index: u32,
	) -> *mut *mut ServiceBase,
	/// `dynamic_execute(index, const GUID & subID, service_ptr callback)`, the pointer passed by
	/// value. Returns false when no command in the entry has `subID`.
	pub dynamic_execute: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		index: u32,
		sub: *const Guid,
		callback: ByValueServicePtr,
	) -> bool,
}

/// `mainmenu_node`: `service_base`'s three slots, then seven.
#[repr(C)]
pub struct NodeVtbl {
	pub base: ServiceBaseVtbl,
	pub get_type: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> u32,
	pub get_display:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, text: *mut StringBase, flags: *mut u32),
	/// Only for a group.
	pub get_children_count: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> usize,
	/// Only for a group. Returns `mainmenu_node::ptr` through a hidden pointer.
	pub get_child: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		ret: *mut *mut ServiceBase,
		index: usize,
	) -> *mut *mut ServiceBase,
	/// Only for a command. `execute(service_ptr callback)`, the pointer passed by value.
	pub execute: unsafe extern "C-unwind" fn(this: *mut ServiceBase, callback: ByValueServicePtr),
	/// Only for a command.
	pub get_guid: unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid,
	/// Only for a command.
	pub get_description:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, out: *mut StringBase) -> bool,
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::{CommandsV2Vtbl, CommandsVtbl, GROUP_VIEW, GroupPopupVtbl, NodeVtbl};
	use crate::guid::Guid;

	#[test]
	fn group_view_is_the_sdks_view_menu() {
		// `mainmenu_groups::view` as `SDK\guids.cpp:966` writes it.
		let view = Guid {
			data1: 0xf8be5604,
			data2: 0x165f,
			data3: 0x4bb9,
			data4: [0xb6, 0xa9, 0x15, 0xe5, 0x5e, 0xe, 0xd, 0x3a],
		};
		assert_eq!(GROUP_VIEW, view);
	}

	#[test]
	fn mainmenu_vtables_have_7_and_11_slots() {
		assert_eq!(size_of::<GroupPopupVtbl>(), 7 * 8);
		assert_eq!(size_of::<CommandsVtbl>(), 11 * 8);
		assert_eq!(offset_of!(CommandsVtbl, execute), 10 * 8);
	}

	#[test]
	fn mainmenu_v2_and_node_vtables_have_14_and_10_slots() {
		assert_eq!(size_of::<CommandsV2Vtbl>(), 14 * 8);
		assert_eq!(offset_of!(CommandsV2Vtbl, is_command_dynamic), 11 * 8);
		assert_eq!(offset_of!(CommandsV2Vtbl, dynamic_instantiate), 12 * 8);
		assert_eq!(offset_of!(CommandsV2Vtbl, dynamic_execute), 13 * 8);
		assert_eq!(size_of::<NodeVtbl>(), 10 * 8);
		assert_eq!(offset_of!(NodeVtbl, get_type), 3 * 8);
		assert_eq!(offset_of!(NodeVtbl, get_child), 6 * 8);
		assert_eq!(offset_of!(NodeVtbl, get_guid), 8 * 8);
		assert_eq!(offset_of!(NodeVtbl, get_description), 9 * 8);
	}
}
