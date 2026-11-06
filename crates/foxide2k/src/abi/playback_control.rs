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

//! `playback_control` (`SDK\playback_control.h:7-44`): foobar2000's player. Main thread only.

use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::guid::Guid;

pub const IID: Guid = Guid::from_u128(0xbf803668_2977_4c71_b9ab_5c77c338c970);

/// The start of `playback_control`'s vtable, up to the last slot used.
#[repr(C)]
pub struct PlaybackControlVtbl {
	pub base: ServiceBaseVtbl,
	/// `bool get_now_playing(metadb_handle_ptr & p_out)`. Not called.
	pub get_now_playing:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, out: *mut *mut ServiceBase) -> bool,
	/// `void start(t_track_command p_command, bool p_paused)`. Not called.
	pub start: unsafe extern "C-unwind" fn(this: *mut ServiceBase, command: i32, paused: bool),
	/// `void stop()`.
	pub stop: unsafe extern "C-unwind" fn(this: *mut ServiceBase),
}
