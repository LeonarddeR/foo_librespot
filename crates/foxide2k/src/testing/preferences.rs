//! Driving a preferences page through its vtables, as foobar2000's Preferences dialog does.

use std::ffi::{CStr, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Counter, FakeString};
use crate::abi::preferences::{
	CALLBACK_IID, PageCallbackVtbl, PageInstanceVtbl, PreferencesPageVtbl,
};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::cxx::try_host;
use crate::guid::Guid;
use crate::service::{ByValueServicePtr, ServiceImpl, ServiceObject, ServicePtr, new_service};

/// A `preferences_page_callback` that counts `on_state_changed` calls.
pub struct FakePageCallback {
	service: ServicePtr,
	calls: Arc<AtomicUsize>,
}

struct CallbackService {
	calls: Arc<AtomicUsize>,
	destroyed: Arc<AtomicUsize>,
}

impl Drop for CallbackService {
	fn drop(&mut self) {
		self.destroyed.fetch_add(1, Ordering::SeqCst);
	}
}

unsafe impl ServiceImpl for CallbackService {
	type Vtbl = PageCallbackVtbl;
	const IIDS: &'static [Guid] = &[CALLBACK_IID];
	const VTBL: &'static PageCallbackVtbl =
		&PageCallbackVtbl { base: ServiceBaseVtbl::new::<Self>(), on_state_changed };
}

unsafe extern "C-unwind" fn on_state_changed(this: *mut ServiceBase) {
	unsafe { ServiceObject::<CallbackService>::value(this) }.calls.fetch_add(1, Ordering::SeqCst);
}

impl FakePageCallback {
	/// A callback, and a counter of how often its service was destroyed.
	#[allow(clippy::new_ret_no_self)]
	pub fn new() -> (Self, Counter) {
		let calls = Arc::new(AtomicUsize::new(0));
		let destroyed = Arc::new(AtomicUsize::new(0));
		let service = new_service(CallbackService {
			calls: Arc::clone(&calls),
			destroyed: Arc::clone(&destroyed),
		});
		(Self { service, calls }, Counter(destroyed))
	}

	pub fn calls(&self) -> usize {
		self.calls.load(Ordering::SeqCst)
	}
}

/// A `preferences_page_v3` service, called through its vtable.
pub struct PreferencesHandle(ServicePtr);

impl PreferencesHandle {
	pub const fn new(service: ServicePtr) -> Self {
		Self(service)
	}

	fn vtbl(&self) -> &PreferencesPageVtbl {
		unsafe { &*(*self.0.as_raw()).vtbl.cast::<PreferencesPageVtbl>() }
	}

	pub fn answers(&self, iid: &Guid) -> bool {
		self.0.query(iid).is_some()
	}

	pub fn name(&self) -> String {
		let name = unsafe { (self.vtbl().get_name)(self.0.as_raw()) };
		unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned()
	}

	pub fn guid(&self) -> Guid {
		let mut guid = Guid::from_u128(0);
		unsafe { (self.vtbl().get_guid)(self.0.as_raw(), &raw mut guid) };
		guid
	}

	pub fn parent_guid(&self) -> Guid {
		let mut guid = Guid::from_u128(0);
		unsafe { (self.vtbl().get_parent_guid)(self.0.as_raw(), &raw mut guid) };
		guid
	}

	pub fn sort_priority(&self) -> f64 {
		unsafe { (self.vtbl().get_sort_priority)(self.0.as_raw()) }
	}

	pub fn reset_query(&self) -> bool {
		unsafe { (self.vtbl().reset_query)(self.0.as_raw()) }
	}

	pub fn help_url(&self) -> Option<String> {
		let mut url = FakeString::new();
		let written =
			try_host(|| unsafe { (self.vtbl().get_help_url)(self.0.as_raw(), url.as_raw()) })
				.expect("get_help_url threw");
		written.then(|| url.value())
	}

	/// Opens the page inside `parent`, passing `callback` by value as foobar2000 does.
	pub fn instantiate(&self, parent: isize, callback: &FakePageCallback) -> PageInstanceHandle {
		let mut temporary = callback.service.clone().into_raw();
		let mut out: *mut ServiceBase = std::ptr::null_mut();
		try_host(|| unsafe {
			(self.vtbl().instantiate)(
				self.0.as_raw(),
				&raw mut out,
				parent as *mut c_void,
				ByValueServicePtr::for_callee(&raw mut temporary),
			);
		})
		.expect("instantiate threw");
		PageInstanceHandle(unsafe { ServicePtr::from_raw_owned(out) }.expect("no instance"))
	}
}

/// A `preferences_page_instance`, called through its vtable.
pub struct PageInstanceHandle(ServicePtr);

impl PageInstanceHandle {
	fn vtbl(&self) -> &PageInstanceVtbl {
		unsafe { &*(*self.0.as_raw()).vtbl.cast::<PageInstanceVtbl>() }
	}

	pub fn state(&self) -> u32 {
		try_host(|| unsafe { (self.vtbl().get_state)(self.0.as_raw()) }).expect("get_state threw")
	}

	pub fn hwnd(&self) -> isize {
		try_host(|| unsafe { (self.vtbl().get_wnd)(self.0.as_raw()) }).expect("get_wnd threw")
			as isize
	}

	pub fn apply(&self) {
		try_host(|| unsafe { (self.vtbl().apply)(self.0.as_raw()) }).expect("apply threw");
	}

	pub fn reset(&self) {
		try_host(|| unsafe { (self.vtbl().reset)(self.0.as_raw()) }).expect("reset threw");
	}
}
