//! The foobar2000 that loaded this component: its `foobar2000_api` and what it told us.

use std::ffi::{CStr, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::abi::component::Api;
use crate::abi::componentversion::{self, ComponentVersionVtbl};
use crate::abi::service::ServiceBase;
use crate::cxx::{ExceptionKind, try_host};
use crate::error::{Error, Result};
use crate::guid::Guid;
use crate::service::ServicePtr;
use crate::strings::OwnedString;

static API: AtomicPtr<Api> = AtomicPtr::new(std::ptr::null_mut());
static INSTANCE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static SERVICES_AVAILABLE: AtomicBool = AtomicBool::new(false);
static LIBRARY: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Access to foobar2000's services, through the `foobar2000_api` it passed to the entry point.
pub struct Host(());

static HOST: Host = Host(());

/// The foobar2000 that loaded this component.
pub fn host() -> &'static Host {
	&HOST
}

pub(crate) fn set_api(api: *mut Api, instance: *mut c_void) {
	API.store(api, Ordering::Release);
	INSTANCE.store(instance, Ordering::Release);
}

pub(crate) fn set_library(path: String, name: String) {
	*LIBRARY.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((path, name));
}

pub(crate) fn set_services_available(available: bool) {
	SERVICES_AVAILABLE.store(available, Ordering::Release);
}

impl Host {
	fn api() -> Option<*mut Api> {
		let api = API.load(Ordering::Acquire);
		(!api.is_null()).then_some(api)
	}

	/// Whether foobar2000 has finished registering services, so they can be used.
	pub fn is_available(&self) -> bool {
		Self::api().is_some() && SERVICES_AVAILABLE.load(Ordering::Acquire)
	}

	/// Every registered service of the class `iid`, like `service_enum_t`.
	pub fn enumerate(&self, iid: &Guid) -> Vec<ServicePtr> {
		let Some(api) = Self::api() else { return Vec::new() };
		try_host(|| unsafe {
			let vtbl = &*(*api).vtbl;
			let class = (vtbl.service_enum_find_class)(api, iid);
			let count = (vtbl.service_enum_get_count)(api, class);
			let mut services = Vec::with_capacity(count);
			for index in 0..count {
				let mut out: *mut ServiceBase = std::ptr::null_mut();
				if (vtbl.service_enum_create)(api, &raw mut out, class, index)
					&& let Some(service) = ServicePtr::from_raw_owned(out)
				{
					services.push(service);
				}
			}
			services
		})
		.unwrap_or_default()
	}

	/// The one service of a core API class such as `popup_message`, like `standard_api_create_t`.
	pub fn core_service(&self, iid: &Guid) -> Option<ServicePtr> {
		if !self.is_available() {
			return None;
		}
		self.enumerate(iid).into_iter().next()
	}

	/// [`core_service`](Self::core_service), or a bug-check error saying foobar2000 has no
	/// `name`.
	pub fn require(&self, iid: &Guid, name: &str) -> Result<ServicePtr> {
		self.core_service(iid)
			.ok_or_else(|| Error::new(ExceptionKind::BugCheck, format!("foobar2000 has no {name}")))
	}

	/// foobar2000's main window, or 0 when there is none.
	pub fn main_window(&self) -> isize {
		Self::api().map_or(0, |api| {
			try_host(|| unsafe { ((*(*api).vtbl).get_main_window)(api) })
				.map_or(0, |window| window as isize)
		})
	}

	/// foobar2000's profile folder, as the URL-style path it reports.
	pub fn profile_path(&self) -> String {
		let Some(api) = Self::api() else { return String::new() };
		try_host(|| unsafe {
			let path = ((*(*api).vtbl).get_profile_path)(api);
			if path.is_null() {
				String::new()
			} else {
				CStr::from_ptr(path).to_string_lossy().into_owned()
			}
		})
		.unwrap_or_default()
	}

	/// Whether the calling thread is foobar2000's main thread.
	pub fn is_main_thread(&self) -> bool {
		Self::api().is_some_and(|api| {
			try_host(|| unsafe { ((*(*api).vtbl).is_main_thread)(api) }).unwrap_or(false)
		})
	}

	/// The file name, without extension, of every loaded component, such as `"foo_youtube"`.
	pub fn loaded_components(&self) -> Vec<String> {
		self.enumerate(&componentversion::IID)
			.iter()
			.filter_map(|service| {
				let raw = service.as_raw();
				let mut name = OwnedString::new();
				try_host(|| unsafe {
					let vtbl = &*(*raw).vtbl.cast::<ComponentVersionVtbl>();
					(vtbl.get_file_name)(raw, name.as_raw());
				})
				.ok()?;
				Some(name.value())
			})
			.collect()
	}

	/// The component's file name without extension, as foobar2000 reported it.
	pub fn library_name(&self) -> Option<String> {
		LIBRARY
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.as_ref()
			.map(|(_, name)| name.clone())
	}

	/// The component DLL's full path, as foobar2000 reported it.
	pub fn library_path(&self) -> Option<String> {
		LIBRARY
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.as_ref()
			.map(|(path, _)| path.clone())
	}
}

#[cfg(test)]
mod tests {
	use super::host;
	use crate::abi::componentversion;
	use crate::component::ComponentVersionService;
	use crate::cxx::ExceptionKind;
	use crate::guid::Guid;
	use crate::service::new_service;
	use crate::testing::FakeApi;

	#[test]
	fn require_gives_the_core_service_or_names_the_missing_one() {
		let api = FakeApi::new();
		let version = ComponentVersionService::new("foo_librespot", "1.0", "");
		api.add_service(componentversion::IID, new_service(version));
		api.install();
		assert!(host().require(&componentversion::IID, "componentversion").is_ok());
		let missing = Guid::from_u128(0x5f2c_7e10_3a4b_4c8d_9e6f_1a2b_3c4d_5e6f);
		let error = host().require(&missing, "popup_message").unwrap_err();
		assert_eq!(error.kind, ExceptionKind::BugCheck);
		assert_eq!(error.message, "foobar2000 has no popup_message");
	}

	#[test]
	fn loaded_components_lists_every_componentversion_file_name() {
		let api = FakeApi::new();
		for name in ["foo_youtube", "foo_librespot"] {
			api.add_service(
				componentversion::IID,
				new_service(ComponentVersionService::new(name, "1.0", "")),
			);
		}
		api.install();
		assert_eq!(host().loaded_components(), ["foo_youtube", "foo_librespot"]);
	}

	#[test]
	fn main_window_is_the_one_the_fake_host_was_given() {
		let api = FakeApi::new();
		api.install();
		assert_eq!(host().main_window(), 0);
		api.set_main_window(0x4321);
		assert_eq!(host().main_window(), 0x4321);
	}
}
