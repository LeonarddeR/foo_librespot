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

//! The component entry point: `foobar2000_client`, `foobar2000_api` (`SDK\component.h:14-54`) and
//! `service_factory_base` (`SDK\service.h:375-396`).

use std::ffi::{c_char, c_void};

use crate::abi::service::ServiceBase;
use crate::guid::Guid;

/// `foobar2000_client`: what the component's entry point returns. No data members.
#[repr(C)]
pub struct Client {
	pub vtbl: *const ClientVtbl,
}

#[repr(C)]
pub struct ClientVtbl {
	pub get_version: unsafe extern "C-unwind" fn(this: *mut Client) -> u32,
	pub get_service_list: unsafe extern "C-unwind" fn(this: *mut Client) -> *mut FactoryBase,
	/// `get_config(stream_writer *, abort_callback &)`.
	pub get_config:
		unsafe extern "C-unwind" fn(this: *mut Client, stream: *mut c_void, abort: *mut c_void),
	/// `set_config(stream_reader *, abort_callback &)`.
	pub set_config:
		unsafe extern "C-unwind" fn(this: *mut Client, stream: *mut c_void, abort: *mut c_void),
	pub set_library_path:
		unsafe extern "C-unwind" fn(this: *mut Client, path: *const c_char, name: *const c_char),
	pub services_init: unsafe extern "C-unwind" fn(this: *mut Client, value: bool),
	pub is_debug: unsafe extern "C-unwind" fn(this: *mut Client) -> bool,
}

/// `service_class_ref`: an opaque handle to one class of services.
pub type ServiceClassRef = *const c_void;

/// `foobar2000_api`: what foobar2000 passes to the entry point. No data members.
#[repr(C)]
pub struct Api {
	pub vtbl: *const ApiVtbl,
}

#[repr(C)]
pub struct ApiVtbl {
	pub service_enum_find_class:
		unsafe extern "C-unwind" fn(this: *mut Api, guid: *const Guid) -> ServiceClassRef,
	/// `service_enum_create(service_ptr_t<service_base> & p_out, service_class_ref, t_size)`.
	pub service_enum_create: unsafe extern "C-unwind" fn(
		this: *mut Api,
		out: *mut *mut ServiceBase,
		class: ServiceClassRef,
		index: usize,
	) -> bool,
	pub service_enum_get_count:
		unsafe extern "C-unwind" fn(this: *mut Api, class: ServiceClassRef) -> usize,
	pub get_main_window: unsafe extern "C-unwind" fn(this: *mut Api) -> *mut c_void,
	pub assert_main_thread: unsafe extern "C-unwind" fn(this: *mut Api) -> bool,
	pub is_main_thread: unsafe extern "C-unwind" fn(this: *mut Api) -> bool,
	pub is_shutting_down: unsafe extern "C-unwind" fn(this: *mut Api) -> bool,
	pub get_profile_path: unsafe extern "C-unwind" fn(this: *mut Api) -> *const c_char,
	pub is_initializing: unsafe extern "C-unwind" fn(this: *mut Api) -> bool,
	pub is_portable_mode_enabled: unsafe extern "C-unwind" fn(this: *mut Api) -> bool,
	pub is_quiet_mode_enabled: unsafe extern "C-unwind" fn(this: *mut Api) -> bool,
}

/// `service_factory_base`: one entry of the component's linked list of service factories.
#[repr(C)]
pub struct FactoryBase {
	pub vtbl: *const FactoryVtbl,
	pub next: *mut Self,
	/// A reference in C++ (`const GUID & m_guid`), stored as a pointer.
	pub guid: *const Guid,
}

#[repr(C)]
pub struct FactoryVtbl {
	/// `instance_create(service_ptr_t<service_base> & p_out)`.
	pub instance_create:
		unsafe extern "C-unwind" fn(this: *mut FactoryBase, out: *mut *mut ServiceBase),
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::{ApiVtbl, ClientVtbl, FactoryBase};

	#[test]
	fn client_vtable_has_7_slots() {
		assert_eq!(size_of::<ClientVtbl>(), 7 * 8);
	}

	#[test]
	fn api_vtable_has_11_slots() {
		assert_eq!(size_of::<ApiVtbl>(), 11 * 8);
	}

	#[test]
	fn factory_layout_is_vptr_next_guid() {
		assert_eq!(offset_of!(FactoryBase, vtbl), 0);
		assert_eq!(offset_of!(FactoryBase, next), 8);
		assert_eq!(offset_of!(FactoryBase, guid), 16);
		assert_eq!(offset_of!(crate::component::Factory, base), 0);
	}
}
