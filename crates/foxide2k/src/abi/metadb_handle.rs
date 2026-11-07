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

//! `metadb_handle` (`SDK\metadb_handle.h:39-43`), of which only `get_location` is used.

use crate::abi::pfc_list::ListBaseConst;
use crate::abi::playlist_loader::PlayableLocation;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};

/// `metadb_handle_list_cref`: a `pfc::list_base_const_t<metadb_handle_ptr>`. A `metadb_handle_ptr`
/// is a single owning pointer.
pub type HandleListRaw = ListBaseConst<*mut ServiceBase>;

/// `metadb_handle`'s first slot; the others are never called.
#[repr(C)]
pub struct MetadbHandleVtbl {
	pub base: ServiceBaseVtbl,
	/// `const playable_location & get_location() const`.
	pub get_location:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> *const PlayableLocation,
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::MetadbHandleVtbl;

	#[test]
	fn get_location_follows_the_service_base_slots() {
		assert_eq!(offset_of!(MetadbHandleVtbl, get_location), 3 * 8);
	}
}
