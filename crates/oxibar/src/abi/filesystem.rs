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

//! `foobar2000_io::filesystem` (`SDK\filesystem.h:44` onwards): the handler for one family of
//! paths, such as `file://`.

use std::ffi::c_char;

use crate::abi::abort_callback::AbortCallback;
use crate::abi::file::FileStats;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::guid::Guid;

pub const IID: Guid = Guid::from_u128(0x2fbce1e5_902e_49e0_b9cf_ce0fba765348);

/// The first slots of `filesystem`, up to `get_stats`. OxiBar only calls these, so the later
/// slots are not declared; do not implement a filesystem with this struct.
#[repr(C)]
pub struct FilesystemVtblPrefix {
	pub base: ServiceBaseVtbl,
	pub get_canonical_path: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		path: *const c_char,
		out: *mut StringBase,
	) -> bool,
	pub is_our_path:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, path: *const c_char) -> bool,
	pub get_display_path: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		path: *const c_char,
		out: *mut StringBase,
	) -> bool,
	/// `open(service_ptr_t<file> & p_out, const char * p_path, t_open_mode p_mode, abort_callback &)`.
	pub open: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		out: *mut *mut ServiceBase,
		path: *const c_char,
		mode: u32,
		abort: *mut AbortCallback,
	),
	pub remove: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		path: *const c_char,
		abort: *mut AbortCallback,
	),
	pub move_file: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		source: *const c_char,
		destination: *const c_char,
		abort: *mut AbortCallback,
	),
	pub is_remote: unsafe extern "C-unwind" fn(this: *mut ServiceBase, path: *const c_char) -> bool,
	/// Throws `exception_io_not_found` when nothing exists at `path`.
	pub get_stats: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		path: *const c_char,
		stats: *mut FileStats,
		is_writeable: *mut bool,
		abort: *mut AbortCallback,
	),
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::FilesystemVtblPrefix;

	#[test]
	fn get_stats_is_slot_10() {
		assert_eq!(offset_of!(FilesystemVtblPrefix, is_our_path), 4 * 8);
		assert_eq!(offset_of!(FilesystemVtblPrefix, get_stats), 10 * 8);
	}
}
