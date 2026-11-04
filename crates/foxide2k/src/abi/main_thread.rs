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

//! Running code on foobar2000's main thread (`SDK\main_thread_callback.h:14-35`), and the list
//! of modeless windows its message loop gives `IsDialogMessage` (`SDK\modeless_dialog.h:6-12`).

use std::ffi::c_void;

use crate::abi::input::ByValueServicePtr;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::guid::Guid;

pub const CALLBACK_IID: Guid = Guid::from_u128(0x87d2c583_7afb_4e58_b21e_fbd3e6e8f5e7);
pub const MANAGER_IID: Guid = Guid::from_u128(0x1131d64b_04cb_43ee_95eb_24d18b753248);
pub const MODELESS_IID: Guid = Guid::from_u128(0xc71b99bd_12c5_48fe_a9c0_469f6fea88bf);

/// `main_thread_callback`, implemented by the component.
#[repr(C)]
pub struct MainThreadCallbackVtbl {
	pub base: ServiceBaseVtbl,
	pub callback_run: unsafe extern "C-unwind" fn(this: *mut ServiceBase),
}

/// `main_thread_callback_manager`, implemented by foobar2000.
#[repr(C)]
pub struct MainThreadCallbackManagerVtbl {
	pub base: ServiceBaseVtbl,
	/// `add_callback(service_ptr_t<main_thread_callback>)`, the pointer passed by value.
	pub add_callback:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, callback: ByValueServicePtr),
}

/// `modeless_dialog_manager`, implemented by foobar2000. Main thread only.
#[repr(C)]
pub struct ModelessDialogManagerVtbl {
	pub base: ServiceBaseVtbl,
	pub add: unsafe extern "C-unwind" fn(this: *mut ServiceBase, window: *mut c_void),
	pub remove: unsafe extern "C-unwind" fn(this: *mut ServiceBase, window: *mut c_void),
}
