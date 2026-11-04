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

//! `pfc::string_base` (`pfc\string-interface.h:6-28`), foobar2000's string out-parameter.

use std::ffi::c_char;

/// A `pfc::string_base` owned by someone else. No virtual destructor.
#[repr(C)]
pub struct StringBase {
	pub vtbl: *const StringBaseVtbl,
}

/// `string_receiver::add_string` comes first; `string_base` re-declares it without a new slot.
/// A length of `usize::MAX` means "up to the NUL terminator".
#[repr(C)]
pub struct StringBaseVtbl {
	pub add_string:
		unsafe extern "C-unwind" fn(this: *mut StringBase, string: *const c_char, length: usize),
	pub get_ptr: unsafe extern "C-unwind" fn(this: *const StringBase) -> *const c_char,
	pub set_string:
		unsafe extern "C-unwind" fn(this: *mut StringBase, string: *const c_char, length: usize),
	pub truncate: unsafe extern "C-unwind" fn(this: *mut StringBase, length: usize),
	pub get_length: unsafe extern "C-unwind" fn(this: *const StringBase) -> usize,
	pub lock_buffer:
		unsafe extern "C-unwind" fn(this: *mut StringBase, length: usize) -> *mut c_char,
	pub unlock_buffer: unsafe extern "C-unwind" fn(this: *mut StringBase),
}
