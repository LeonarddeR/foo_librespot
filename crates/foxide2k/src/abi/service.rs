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

//! `service_base` (`SDK\service.h:302-362`), the root of every foobar2000 service interface.

use crate::guid::Guid;

/// The start of every service object: a pointer to its vtable.
#[repr(C)]
pub struct ServiceBase {
	pub vtbl: *const ServiceBaseVtbl,
}

/// The three `service_base` slots that begin every service vtable. `service_base` has no
/// virtual destructor, so slot 0 is `service_release`.
#[repr(C)]
pub struct ServiceBaseVtbl {
	/// `int service_release() noexcept`: returns the new reference count.
	pub service_release: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> i32,
	/// `int service_add_ref() noexcept`: returns the new reference count.
	pub service_add_ref: unsafe extern "C-unwind" fn(this: *mut ServiceBase) -> i32,
	/// `bool service_query(service_ptr & p_out, const GUID & p_guid)`. `p_out` is a
	/// `service_ptr_t`, which is a single owning pointer.
	pub service_query: unsafe extern "C-unwind" fn(
		this: *mut ServiceBase,
		out: *mut *mut ServiceBase,
		guid: *const Guid,
	) -> bool,
}
