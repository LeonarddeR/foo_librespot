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

//! `advconfig_entry`, `advconfig_branch`, `advconfig_entry_checkbox` and
//! `advconfig_entry_checkbox_v2` (`SDK\advconfig.h:6-58`).

use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::guid::Guid;

pub const ENTRY_IID: Guid = Guid::from_u128(0x7e84602e_dc49_4047_aaee_63718bbc5a1f);
pub const BRANCH_IID: Guid = Guid::from_u128(0x6a60b472_91ac_4681_9dc2_76c994005b63);
pub const CHECKBOX_IID: Guid = Guid::from_u128(0x5243aba4_2a3d_4627_88ef_cee3761c079c);
pub const CHECKBOX_V2_IID: Guid = Guid::from_u128(0xe29b37d0_a957_4a85_8240_1e96c729b669);

/// `advconfig_entry::guid_branch_tools`, the "Tools" branch.
pub const GUID_BRANCH_TOOLS: Guid = Guid::from_u128(0x35365484_cc58_4926_97e1_5e63f3abb9e2);

/// `advconfig_entry`. A branch uses it as is: `advconfig_branch` adds no slots.
#[repr(C)]
pub struct EntryVtbl {
	pub base: ServiceBaseVtbl,
	pub get_name: unsafe extern "C-unwind" fn(this: *mut ServiceBase, out: *mut StringBase),
	pub get_guid: unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid,
	pub get_parent:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid,
	pub reset: unsafe extern "C-unwind" fn(this: *mut ServiceBase),
	pub get_sort_priority: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> f64,
}

/// `advconfig_entry_checkbox`, then `advconfig_entry_checkbox_v2`'s two slots.
#[repr(C)]
pub struct CheckboxVtbl {
	pub entry: EntryVtbl,
	pub get_state: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
	pub set_state: unsafe extern "C-unwind" fn(this: *mut ServiceBase, state: bool),
	pub is_radio: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
	/// `advconfig_entry_checkbox_v2`.
	pub get_default_state: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
	/// `advconfig_entry_checkbox_v2`: `preferences_state` flags asking for a restart; 0 for none.
	pub get_preferences_flags: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> u32,
}
