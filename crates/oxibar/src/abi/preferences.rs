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

//! `preferences_page_v3` (`SDK\preferences_page.h:27-63, 66-71, 151-156`) and the instance and
//! callback it is created with (`:123-146`).

use std::ffi::{c_char, c_void};

use crate::abi::input::ByValueServicePtr;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::guid::Guid;

pub const PAGE_IID: Guid = Guid::from_u128(0x7255e8d0_3fcf_4781_b93b_d06cb88dfafa);
pub const PAGE_V2_IID: Guid = Guid::from_u128(0xce4ebc9e_ab20_46f9_925f_883b084f0569);
pub const PAGE_V3_IID: Guid = Guid::from_u128(0xd6d0f741_9f17_4df8_9d5c_87f28b1f0e64);
pub const INSTANCE_IID: Guid = Guid::from_u128(0x6893a996_a816_49fe_82ce_0cb804a4cfec);
pub const CALLBACK_IID: Guid = Guid::from_u128(0x3d26e08e_861c_4599_9c89_aaa719af5070);

/// `preferences_page::guid_tools`, the "Tools" branch.
pub const GUID_TOOLS: Guid = Guid::from_u128(0x627c0767_0793_44f8_8087_be4934311282);

/// `preferences_state::changed`: Apply is enabled.
pub const STATE_CHANGED: u32 = 1;
/// `preferences_state::resettable`: Reset is enabled.
pub const STATE_RESETTABLE: u32 = 8;

#[repr(C)]
pub struct PreferencesPageVtbl {
	pub base: ServiceBaseVtbl,
	/// Obsolete; foobar2000 2.x calls `instantiate`.
	pub create:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, parent: *mut c_void) -> *mut c_void,
	pub get_name: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> *const c_char,
	pub get_guid: unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid,
	pub get_parent_guid:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid,
	/// Obsolete.
	pub reset_query: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
	/// Obsolete.
	pub reset: unsafe extern "C-unwind" fn(this: *mut ServiceBase),
	pub get_help_url:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, out: *mut StringBase) -> bool,
	/// `preferences_page_v2`.
	pub get_sort_priority: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> f64,
	/// `preferences_page_v3`: `instantiate(HWND parent, preferences_page_callback::ptr)`, the
	/// instance returned through a hidden pointer and the callback passed by value.
	pub instantiate: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		ret: *mut *mut ServiceBase,
		parent: *mut c_void,
		callback: ByValueServicePtr,
	) -> *mut *mut ServiceBase,
}

#[repr(C)]
pub struct PageInstanceVtbl {
	pub base: ServiceBaseVtbl,
	pub get_state: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> u32,
	pub get_wnd: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> *mut c_void,
	pub apply: unsafe extern "C-unwind" fn(this: *mut ServiceBase),
	pub reset: unsafe extern "C-unwind" fn(this: *mut ServiceBase),
}

#[repr(C)]
pub struct PageCallbackVtbl {
	pub base: ServiceBaseVtbl,
	pub on_state_changed: unsafe extern "C-unwind" fn(this: *mut ServiceBase),
}
