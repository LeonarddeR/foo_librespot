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

//! `fb2k::string` (`SDK\commonObjects.h:155-169`): a UTF-8 string held by a service, which
//! `fb2k::configStore` hands out and takes as a default.

use std::ffi::c_char;

use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::guid::Guid;

pub const IID: Guid = Guid::from_u128(0xb3572de3_dc77_4494_a390_b1ffd9173877);

#[repr(C)]
pub struct StringVtbl {
	pub base: ServiceBaseVtbl,
	/// `const char * c_str() const`: the NUL-terminated text, owned by the string.
	pub c_str: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> *const c_char,
}

#[cfg(test)]
mod tests {
	use std::mem::{offset_of, size_of};

	use super::StringVtbl;

	#[test]
	fn c_str_is_the_only_slot_after_service_base() {
		assert_eq!(offset_of!(StringVtbl, c_str), 3 * 8);
		assert_eq!(size_of::<StringVtbl>(), 4 * 8);
	}
}
