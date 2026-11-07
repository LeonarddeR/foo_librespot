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

//! `popup_message` (`SDK\popup_message.h:11-20`): a non-modal message window. Safe to call from
//! worker threads; foobar2000 shows it on the main thread.

use std::ffi::{c_char, c_void};

use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::guid::Guid;

pub const IID: Guid = Guid::from_u128(0xd5cac75a_3023_4dce_8b0d_b502bd056a7c);

/// `popup_message::icon_information`.
pub const ICON_INFORMATION: u32 = 0;
/// `popup_message::icon_error`.
pub const ICON_ERROR: u32 = 1;

#[repr(C)]
pub struct PopupMessageVtbl {
	pub base: ServiceBaseVtbl,
	/// `show_ex(msg, msg_length, title, title_length, icon)`; lengths are in bytes.
	pub show_ex: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		message: *const c_char,
		message_length: usize,
		title: *const c_char,
		title_length: usize,
		icon: u32,
	),
}

pub const V3_IID: Guid = Guid::from_u128(0xef3c83fd_1144_4edb_b843_cfbac6e5e18b);

/// `popup_message_v3::buttonYes`.
pub const BUTTON_YES: u32 = 1 << 2;
/// `popup_message_v3::buttonNo`.
pub const BUTTON_NO: u32 = 1 << 3;
/// `popup_message_v3::iconQuestion`.
pub const ICON_QUESTION: u32 = 2;

/// `popup_message_v3::query_t` (`SDK\popup_message.h:117-128`). `reply` is a `completion_notify::ptr`,
/// one pointer wide; null when there is no reply.
#[repr(C)]
pub struct Query {
	pub title: *const c_char,
	pub message: *const c_char,
	pub buttons: u32,
	pub default_button: u32,
	pub icon: u32,
	pub reply: *mut ServiceBase,
	pub parent: *mut c_void,
	pub do_not_ask_again: *const c_char,
}

/// `popup_message_v3` (`SDK\popup_message.h:93-134`): a MessageBox-like query. Main thread only.
#[repr(C)]
pub struct PopupV3Vtbl {
	pub base: ServiceBaseVtbl,
	pub show_query: unsafe extern "C-unwind" fn(this: *mut ServiceBase, query: *const Query),
	/// Returns the button code the user pressed.
	pub show_query_modal:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, query: *const Query) -> u32,
}

#[cfg(test)]
mod tests {
	use std::mem::{offset_of, size_of};

	use super::Query;

	#[test]
	fn query_t_is_56_bytes_with_reply_at_32() {
		assert_eq!(size_of::<Query>(), 56);
		assert_eq!(offset_of!(Query, reply), 32);
	}
}
