//! The component entry point and the list of services a component registers.

use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::abi::component::{Api, Client, ClientVtbl, FactoryBase, FactoryVtbl};
use crate::abi::componentversion::{self, ComponentVersionVtbl};
use crate::abi::initquit;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::boundary::guard;
use crate::guid::Guid;
use crate::host::{self, host};
use crate::initquit::{InitQuit, InitQuitService};
use crate::service::{ServiceImpl, ServiceObject, ServicePtr, Singleton};
use crate::strings::set_string;

enum Instance {
	/// A static service; every `instance_create` hands out the same object.
	Singleton(*const ServiceBase),
	/// A fresh service per `instance_create`.
	New(fn() -> ServicePtr),
}

/// One service registration: an SDK `service_factory_base` followed by how to create the
/// service. foobar2000 only reads the `service_factory_base` part.
#[repr(C)]
pub struct Factory {
	pub(crate) base: UnsafeCell<FactoryBase>,
	instance: Instance,
}

// foobar2000 reads factories from any thread; OxiBar only writes `next` while linking the list
// in the entry point, before foobar2000 has seen it.
unsafe impl Sync for Factory {}

static FACTORY_VTBL: FactoryVtbl = FactoryVtbl { instance_create };

impl Factory {
	const fn new(entrypoint: &'static Guid, instance: Instance) -> Self {
		Self {
			base: UnsafeCell::new(FactoryBase {
				vtbl: &raw const FACTORY_VTBL,
				next: std::ptr::null_mut(),
				guid: entrypoint,
			}),
			instance,
		}
	}

	/// Registers a static service under its entrypoint interface, like `service_factory_single_t`.
	pub const fn singleton<T: ServiceImpl>(
		service: &'static Singleton<T>,
		entrypoint: &'static Guid,
	) -> Self {
		Self::new(entrypoint, Instance::Singleton(service.as_raw()))
	}

	/// Registers a service that is created anew for every request, like `service_factory_t`.
	pub const fn per_instance(entrypoint: &'static Guid, create: fn() -> ServicePtr) -> Self {
		Self::new(entrypoint, Instance::New(create))
	}

	/// Registers startup and shutdown callbacks.
	pub const fn init_quit<T: InitQuit>(service: &'static Singleton<InitQuitService<T>>) -> Self {
		Self::singleton(service, &initquit::IID)
	}

	fn create(&self) -> ServicePtr {
		match self.instance {
			Instance::Singleton(raw) => unsafe { ServicePtr::from_raw_add_ref(raw.cast_mut()) }
				.expect("a static is never null"),
			Instance::New(create) => create(),
		}
	}
}

unsafe extern "C-unwind" fn instance_create(this: *mut FactoryBase, out: *mut *mut ServiceBase) {
	unsafe {
		guard(|| {
			let factory = &*this.cast::<Factory>();
			let service = factory.create();
			// Like the SDK's pass_instance: hand out the entrypoint interface, releasing whatever
			// `out` held before.
			let entrypoint = service.query(&*(*this).guid).unwrap_or(service);
			let previous = std::mem::replace(&mut *out, entrypoint.into_raw());
			drop(ServicePtr::from_raw_owned(previous));
			Ok(())
		});
	}
}

/// Everything a component registers, built by [`component!`](crate::component!).
pub struct Component {
	pub factories: &'static [Factory],
}

/// The `componentversion` service every component registers.
pub struct ComponentVersionService {
	name: &'static str,
	version: &'static str,
	about: &'static str,
}

impl ComponentVersionService {
	pub const fn new(name: &'static str, version: &'static str, about: &'static str) -> Self {
		Self { name, version, about }
	}
}

unsafe impl ServiceImpl for ComponentVersionService {
	type Vtbl = ComponentVersionVtbl;
	const IIDS: &'static [Guid] = &[componentversion::IID];
	const VTBL: &'static ComponentVersionVtbl = &ComponentVersionVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_file_name: version_string::<0>,
		get_component_name: version_string::<1>,
		get_component_version: version_string::<2>,
		get_about_message: version_string::<3>,
	};
}

unsafe extern "C-unwind" fn version_string<const FIELD: u8>(
	this: *mut ServiceBase,
	out: *mut StringBase,
) {
	unsafe {
		guard(|| {
			let service = ServiceObject::<ComponentVersionService>::value(this);
			let file_name;
			let value = match FIELD {
				// The SDK reports core_api::get_my_file_name(), the name from set_library_path.
				0 => {
					file_name = host().library_name().unwrap_or_else(|| service.name.to_owned());
					file_name.as_str()
				}
				1 => service.name,
				2 => service.version,
				_ => service.about,
			};
			set_string(out, value)
		});
	}
}

struct StaticClient(Client);

// The client has no state of its own; it is shared by design.
unsafe impl Sync for StaticClient {}

static CLIENT_VTBL: ClientVtbl = ClientVtbl {
	get_version,
	get_service_list,
	get_config,
	set_config,
	set_library_path,
	services_init,
	is_debug,
};
static CLIENT: StaticClient = StaticClient(Client { vtbl: &raw const CLIENT_VTBL });
static SERVICE_LIST: AtomicPtr<FactoryBase> = AtomicPtr::new(std::ptr::null_mut());

/// The body of the `foobar2000_get_interface` export that [`component!`](crate::component!)
/// generates.
///
/// # Safety
/// `api` must be foobar2000's `foobar2000_api`, valid for the life of the process.
pub unsafe fn get_interface(
	component: &'static Component,
	api: *mut Api,
	instance: *mut c_void,
) -> *mut Client {
	host::set_api(api, instance);
	let mut next: *mut FactoryBase = std::ptr::null_mut();
	for factory in component.factories.iter().rev() {
		unsafe { (*factory.base.get()).next = next };
		next = factory.base.get();
	}
	SERVICE_LIST.store(next, Ordering::Release);
	(&raw const CLIENT.0).cast_mut()
}

unsafe extern "C-unwind" fn get_version(_this: *mut Client) -> u32 {
	crate::SDK_TARGET_VERSION
}

unsafe extern "C-unwind" fn get_service_list(_this: *mut Client) -> *mut FactoryBase {
	SERVICE_LIST.load(Ordering::Acquire)
}

// OxiBar keeps no configuration in foobar2000's legacy cfg_var stream.
unsafe extern "C-unwind" fn get_config(
	_this: *mut Client,
	_stream: *mut c_void,
	_abort: *mut c_void,
) {
}

unsafe extern "C-unwind" fn set_config(
	_this: *mut Client,
	_stream: *mut c_void,
	_abort: *mut c_void,
) {
}

unsafe extern "C-unwind" fn set_library_path(
	_this: *mut Client,
	path: *const c_char,
	name: *const c_char,
) {
	unsafe {
		guard(|| {
			let read = |text: *const c_char| {
				if text.is_null() {
					String::new()
				} else {
					CStr::from_ptr(text).to_string_lossy().into_owned()
				}
			};
			host::set_library(read(path), read(name));
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn services_init(_this: *mut Client, value: bool) {
	host::set_services_available(value);
}

unsafe extern "C-unwind" fn is_debug(_this: *mut Client) -> bool {
	cfg!(debug_assertions)
}

/// Declares a foobar2000 component: exports `foobar2000_get_interface` and registers the
/// component's `componentversion` plus the given service factories.
///
/// ```ignore
/// oxibar::component! {
///     name: "foo_example",
///     version: env!("CARGO_PKG_VERSION"),
///     about: "What it does.",
///     services: [Factory::init_quit(&STARTUP)],
/// }
/// ```
#[macro_export]
macro_rules! component {
	(name: $name:expr, version: $version:expr, about: $about:expr, services: [$($factory:expr),* $(,)?] $(,)?) => {
		const _: () = {
			static VERSION: $crate::service::Singleton<$crate::component::ComponentVersionService> =
				$crate::service::Singleton::new($crate::component::ComponentVersionService::new($name, $version, $about));
			static FACTORIES: [$crate::component::Factory; 1 + $crate::__count!($($factory),*)] = [
				$crate::component::Factory::singleton(&VERSION, &$crate::abi::componentversion::IID),
				$($factory),*
			];
			static COMPONENT: $crate::component::Component = $crate::component::Component { factories: &FACTORIES };

			#[unsafe(no_mangle)]
			unsafe extern "C" fn foobar2000_get_interface(
				api: *mut $crate::abi::component::Api,
				instance: *mut ::core::ffi::c_void,
			) -> *mut $crate::abi::component::Client {
				unsafe { $crate::component::get_interface(&COMPONENT, api, instance) }
			}
		};
	};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __count {
	() => { 0usize };
	($head:expr $(, $tail:expr)*) => { 1usize + $crate::__count!($($tail),*) };
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::Factory;

	#[test]
	fn factory_starts_with_service_factory_base() {
		assert_eq!(offset_of!(Factory, base), 0);
	}
}
