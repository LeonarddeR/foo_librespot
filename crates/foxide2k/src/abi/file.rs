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

//! File structures from `SDK\file.h`.

use std::ffi::c_void;

use crate::abi::abort_callback::AbortCallback;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::guid::Guid;

/// `t_filestats` (`SDK\file.h:25-30`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStats {
	/// `filesize_invalid` (`u64::MAX`) when unknown.
	pub size: u64,
	/// A Windows FILETIME; 0 when unknown.
	pub timestamp: u64,
}

impl FileStats {
	/// `filestats_invalid`.
	pub const INVALID: Self = Self { size: u64::MAX, timestamp: 0 };
}

/// `foobar2000_io::file` (`SDK\file.h:263`).
pub const FILE_IID: Guid = Guid::from_u128(0xb2f9fc40_3e55_4b23_a2c9_22baad8795b1);

/// `filesystem::t_open_mode` (`SDK\filesystem.h:49-59`). Compare after masking with
/// [`OPEN_MODE_MASK`]: foobar2000 may add [`OPEN_SHAREABLE`].
pub const OPEN_MODE_READ: u32 = 0;
pub const OPEN_MODE_WRITE_EXISTING: u32 = 1;
pub const OPEN_MODE_WRITE_NEW: u32 = 2;
pub const OPEN_MODE_MASK: u32 = 0xFF;
pub const OPEN_SHAREABLE: u32 = 0x100;

/// `file` derives from `service_base`, `stream_reader` and `stream_writer`, so an object has
/// three vtable pointers: this one at offset 0, [`StreamReaderVtbl`] at 8 and
/// [`StreamWriterVtbl`] at 16. Calls through the second and third arrive with `this` at that
/// offset. Slots from `SDK\file.h:279-332`.
#[repr(C)]
pub struct FileVtbl {
	pub base: ServiceBaseVtbl,
	pub get_size:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, abort: *mut AbortCallback) -> u64,
	pub get_position:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, abort: *mut AbortCallback) -> u64,
	pub resize:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, size: u64, abort: *mut AbortCallback),
	pub seek: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		position: u64,
		abort: *mut AbortCallback,
	),
	/// `mode` is `t_seek_mode`: 0 from the start, 1 from the current position, 2 from the end.
	pub seek_ex: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		position: i64,
		mode: i32,
		abort: *mut AbortCallback,
	),
	pub can_seek: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
	pub get_content_type:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, out: *mut StringBase) -> bool,
	pub is_in_memory: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
	pub on_idle: unsafe extern "C-unwind" fn(this: *mut ServiceBase, abort: *mut AbortCallback),
	pub get_timestamp:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, abort: *mut AbortCallback) -> u64,
	pub reopen: unsafe extern "C-unwind" fn(this: *mut ServiceBase, abort: *mut AbortCallback),
	pub is_remote: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> bool,
}

/// `stream_reader` (`SDK\file.h:142-163`), the vtable at offset 8 of a `file`.
#[repr(C)]
pub struct StreamReaderVtbl {
	pub read: unsafe extern "C-unwind" fn(
		this: *mut c_void,
		buffer: *mut c_void,
		bytes: usize,
		abort: *mut AbortCallback,
	) -> usize,
	pub read_object: unsafe extern "C-unwind" fn(
		this: *mut c_void,
		buffer: *mut c_void,
		bytes: usize,
		abort: *mut AbortCallback,
	),
	pub skip: unsafe extern "C-unwind" fn(
		this: *mut c_void,
		bytes: u64,
		abort: *mut AbortCallback,
	) -> u64,
	pub skip_object:
		unsafe extern "C-unwind" fn(this: *mut c_void, bytes: u64, abort: *mut AbortCallback),
}

/// `stream_writer` (`SDK\file.h:221-227`), the vtable at offset 16 of a `file`.
#[repr(C)]
pub struct StreamWriterVtbl {
	pub write: unsafe extern "C-unwind" fn(
		this: *mut c_void,
		buffer: *const c_void,
		bytes: usize,
		abort: *mut AbortCallback,
	),
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::{FileVtbl, StreamReaderVtbl, StreamWriterVtbl};

	#[test]
	fn file_vtbls_have_15_4_and_1_slots() {
		assert_eq!(size_of::<FileVtbl>(), 15 * 8);
		assert_eq!(offset_of!(FileVtbl, get_size), 3 * 8);
		assert_eq!(offset_of!(FileVtbl, get_content_type), 9 * 8);
		assert_eq!(offset_of!(FileVtbl, is_remote), 14 * 8);
		assert_eq!(size_of::<StreamReaderVtbl>(), 4 * 8);
		assert_eq!(offset_of!(StreamReaderVtbl, skip_object), 3 * 8);
		assert_eq!(size_of::<StreamWriterVtbl>(), 8);
	}
}
