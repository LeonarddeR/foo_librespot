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

//! `fb2k::configStore` (`SDK\configStore.h:22-39`): foobar2000's own settings store. Only the
//! slots up to `setConfigString` are declared; Foxide2k calls no later ones.

use std::ffi::c_char;

use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::guid::Guid;
use crate::service::ByValueServicePtr;

pub const IID: Guid = Guid::from_u128(0x97aad7fd_b216_4286_9eb3_188d35ee841c);

#[repr(C)]
pub struct ConfigStoreVtbl {
	pub base: ServiceBaseVtbl,
	/// `fb2k::objRef acquireTransactionScope()`, returned through a hidden pointer. Not called.
	pub acquire_transaction_scope: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		ret: *mut *mut ServiceBase,
	) -> *mut *mut ServiceBase,
	/// `void commitBlocking()`. Not called.
	pub commit_blocking: unsafe extern "C-unwind" fn(this: *mut ServiceBase),
	pub get_config_int: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		name: *const c_char,
		default: i64,
	) -> i64,
	pub set_config_int:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, name: *const c_char, value: i64),
	/// `void deleteConfigInt(const char * name)`. Not called.
	pub delete_config_int: unsafe extern "C-unwind" fn(this: *mut ServiceBase, name: *const c_char),
	/// `fb2k::stringRef getConfigString(const char * name, fb2k::stringRef defaVal)`: an
	/// `fb2k::string`, returned through a hidden pointer. The default is passed by value, so the
	/// callee releases it, and comes back when nothing is stored under `name`.
	pub get_config_string: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		ret: *mut *mut ServiceBase,
		name: *const c_char,
		default: ByValueServicePtr,
	) -> *mut *mut ServiceBase,
	pub set_config_string: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		name: *const c_char,
		value: *const c_char,
	),
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::ConfigStoreVtbl;

	#[test]
	fn the_settings_slots_are_in_sdk_order() {
		assert_eq!(offset_of!(ConfigStoreVtbl, get_config_int), 5 * 8);
		assert_eq!(offset_of!(ConfigStoreVtbl, set_config_int), 6 * 8);
		assert_eq!(offset_of!(ConfigStoreVtbl, get_config_string), 8 * 8);
		assert_eq!(offset_of!(ConfigStoreVtbl, set_config_string), 9 * 8);
	}
}
