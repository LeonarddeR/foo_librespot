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

//! `componentversion` (`SDK\componentversion.h:4-12`): the name, version and About text foobar2000
//! shows in Preferences > Components. Every component must register exactly one.

use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::guid::Guid;

pub const IID: Guid = Guid::from_u128(0x10bb3ebd_ddf7_4975_a3cc_23084829453e);

#[repr(C)]
pub struct ComponentVersionVtbl {
	pub base: ServiceBaseVtbl,
	pub get_file_name: unsafe extern "C-unwind" fn(this: *mut ServiceBase, out: *mut StringBase),
	pub get_component_name:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, out: *mut StringBase),
	pub get_component_version:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, out: *mut StringBase),
	/// Lines are separated by "\n".
	pub get_about_message:
		unsafe extern "C-unwind" fn(this: *mut ServiceBase, out: *mut StringBase),
}
