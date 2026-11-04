//! Registering a page in foobar2000's Preferences, through `preferences_page_v3`.
//!
//! foobar2000 calls a page and its instances only on its main thread, and calls back into an
//! instance from inside `on_state_changed`. So instances take `&self`: a page that reports a
//! change while applying is asked for its state before `apply` returns.

use std::ffi::{CStr, c_char, c_void};
use std::marker::PhantomData;

use crate::abi::input::ByValueServicePtr;
use crate::abi::preferences::{
	INSTANCE_IID, PAGE_IID, PAGE_V2_IID, PAGE_V3_IID, PageCallbackVtbl, PageInstanceVtbl,
	PreferencesPageVtbl,
};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::bindings::Windows::Win32::{DestroyWindow, HWND, IsWindow};
use crate::boundary::guard;
use crate::component::Factory;
use crate::cxx::try_host;
use crate::error::Result;
use crate::guid::Guid;
use crate::service::{ServiceImpl, ServiceObject, ServicePtr, new_service};
use crate::strings::set_string;

/// A page in foobar2000's Preferences.
pub trait PreferencesPage: 'static {
	const GUID: Guid;
	/// The branch the page sits under, such as `abi::preferences::GUID_TOOLS`.
	const PARENT: Guid;
	const NAME: &'static CStr;
	const HELP_URL: &'static str;
	/// Creates the page's window inside `parent`. `changed` tells foobar2000 to ask for the
	/// page's state again.
	fn create(parent: isize, changed: PageChanged) -> Result<Box<dyn PageInstance>>;
}

/// One open page.
pub trait PageInstance: 'static {
	fn hwnd(&self) -> isize;
	/// `preferences_state` flags, such as `STATE_CHANGED | STATE_RESETTABLE`.
	fn state(&self) -> u32;
	fn apply(&self);
	/// Puts the defaults in the page without applying them.
	fn reset(&self);
}

/// foobar2000's `preferences_page_callback` for one open page.
#[derive(Clone)]
pub struct PageChanged(ServicePtr);

impl PageChanged {
	/// Tells foobar2000 the page's state changed, so it updates the Apply button.
	pub fn on_state_changed(&self) {
		let raw = self.0.as_raw();
		let _ = try_host(|| unsafe {
			let vtbl = &*(*raw).vtbl.cast::<PageCallbackVtbl>();
			(vtbl.on_state_changed)(raw);
		});
	}
}

/// The factory that registers `T` with foobar2000.
pub const fn preferences_page_factory<T: PreferencesPage>() -> Factory {
	Factory::per_instance(&PAGE_IID, preferences_page_service::<T>)
}

/// A new `preferences_page_v3` service for `T`.
pub fn preferences_page_service<T: PreferencesPage>() -> ServicePtr {
	new_service(Page::<T>(PhantomData))
}

struct Page<T>(PhantomData<fn() -> T>);

unsafe impl<T: PreferencesPage> ServiceImpl for Page<T> {
	type Vtbl = PreferencesPageVtbl;
	const IIDS: &'static [Guid] = &[PAGE_V3_IID, PAGE_V2_IID, PAGE_IID];
	const VTBL: &'static PreferencesPageVtbl = &PreferencesPageVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		create: obsolete_create,
		get_name: get_name::<T>,
		get_guid: get_guid::<T>,
		get_parent_guid: get_parent_guid::<T>,
		reset_query,
		reset: obsolete_reset,
		get_help_url: get_help_url::<T>,
		get_sort_priority,
		instantiate: instantiate::<T>,
	};
}

unsafe extern "C-unwind" fn obsolete_create(
	_this: *mut ServiceBase,
	_parent: *mut c_void,
) -> *mut c_void {
	std::ptr::null_mut()
}

unsafe extern "C-unwind" fn get_name<T: PreferencesPage>(_this: *mut ServiceBase) -> *const c_char {
	T::NAME.as_ptr()
}

unsafe extern "C-unwind" fn get_guid<T: PreferencesPage>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
) -> *mut Guid {
	unsafe { ret.write(T::GUID) };
	ret
}

unsafe extern "C-unwind" fn get_parent_guid<T: PreferencesPage>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
) -> *mut Guid {
	unsafe { ret.write(T::PARENT) };
	ret
}

unsafe extern "C-unwind" fn reset_query(_this: *mut ServiceBase) -> bool {
	false
}

unsafe extern "C-unwind" fn obsolete_reset(_this: *mut ServiceBase) {}

unsafe extern "C-unwind" fn get_help_url<T: PreferencesPage>(
	_this: *mut ServiceBase,
	out: *mut StringBase,
) -> bool {
	unsafe {
		guard(|| {
			set_string(out, T::HELP_URL)?;
			Ok(true)
		})
	}
}

unsafe extern "C-unwind" fn get_sort_priority(_this: *mut ServiceBase) -> f64 {
	0.0
}

unsafe extern "C-unwind" fn instantiate<T: PreferencesPage>(
	_this: *mut ServiceBase,
	ret: *mut *mut ServiceBase,
	parent: *mut c_void,
	callback: ByValueServicePtr,
) -> *mut *mut ServiceBase {
	unsafe {
		guard(|| {
			// Passed by value: keep a reference of our own, then destroy the caller's temporary.
			let kept = ServicePtr::from_raw_add_ref(*callback);
			drop(ServicePtr::from_raw_owned(*callback));
			let changed = PageChanged(kept.ok_or_else(|| {
				crate::error::Error::new(
					crate::cxx::ExceptionKind::InvalidParams,
					"instantiate without a callback",
				)
			})?);
			let instance = T::create(parent as isize, changed)?;
			ret.write(new_service(Instance(MainThreadOnly(instance))).into_raw());
			Ok(ret)
		})
	}
}

/// Holds what only foobar2000's main thread touches. foobar2000 creates, calls and releases
/// page instances on that thread.
struct MainThreadOnly<T>(T);

#[allow(clippy::non_send_fields_in_send_ty, reason = "only foobar2000's main thread touches it")]
unsafe impl<T> Send for MainThreadOnly<T> {}
unsafe impl<T> Sync for MainThreadOnly<T> {}

struct Instance(MainThreadOnly<Box<dyn PageInstance>>);

impl Instance {
	unsafe fn of<'a>(this: *mut ServiceBase) -> &'a dyn PageInstance {
		&*unsafe { ServiceObject::<Self>::value(this) }.0.0
	}
}

impl Drop for Instance {
	/// foobar2000 destroys the window before the last release; this covers a host that didn't.
	fn drop(&mut self) {
		let window = HWND(self.0.0.hwnd() as _);
		unsafe {
			if IsWindow(Some(window)).as_bool() {
				let _ = DestroyWindow(window);
			}
		}
	}
}

unsafe impl ServiceImpl for Instance {
	type Vtbl = PageInstanceVtbl;
	const IIDS: &'static [Guid] = &[INSTANCE_IID];
	const VTBL: &'static PageInstanceVtbl = &PageInstanceVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_state,
		get_wnd,
		apply,
		reset: instance_reset,
	};
}

unsafe extern "C-unwind" fn get_state(this: *mut ServiceBase) -> u32 {
	unsafe { guard(|| Ok(Instance::of(this).state())) }
}

unsafe extern "C-unwind" fn get_wnd(this: *mut ServiceBase) -> *mut c_void {
	unsafe { guard(|| Ok(Instance::of(this).hwnd() as *mut c_void)) }
}

unsafe extern "C-unwind" fn apply(this: *mut ServiceBase) {
	unsafe {
		guard(|| {
			Instance::of(this).apply();
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn instance_reset(this: *mut ServiceBase) {
	unsafe {
		guard(|| {
			Instance::of(this).reset();
			Ok(())
		});
	}
}

#[cfg(test)]
mod tests {
	use std::ffi::CStr;
	use std::sync::atomic::{AtomicUsize, Ordering};

	use super::{PageChanged, PageInstance, PreferencesPage, preferences_page_service};
	use crate::abi::preferences::{
		GUID_TOOLS, INSTANCE_IID, PAGE_IID, PAGE_V2_IID, PAGE_V3_IID, STATE_CHANGED,
		STATE_RESETTABLE,
	};
	use crate::bindings::Windows::Win32::{HWND, IsWindow};
	use crate::dialog::host::tests::hidden_parent;
	use crate::error::Result;
	use crate::guid::Guid;
	use crate::testing::{FakePageCallback, PreferencesHandle};

	static APPLIES: AtomicUsize = AtomicUsize::new(0);
	static RESETS: AtomicUsize = AtomicUsize::new(0);

	struct TestPage;

	struct TestInstance {
		window: isize,
		changed: PageChanged,
	}

	impl PreferencesPage for TestPage {
		const GUID: Guid = Guid::from_u128(0xcbd58b0a_b483_4deb_a487_38dfdb3e106e);
		const PARENT: Guid = GUID_TOOLS;
		const NAME: &'static CStr = c"foo_librespot";
		const HELP_URL: &'static str = "https://github.com/LeonarddeR/foo_librespot";

		fn create(parent: isize, changed: PageChanged) -> Result<Box<dyn PageInstance>> {
			Ok(Box::new(TestInstance { window: parent, changed }))
		}
	}

	impl PageInstance for TestInstance {
		fn hwnd(&self) -> isize {
			self.window
		}

		fn state(&self) -> u32 {
			STATE_RESETTABLE | if APPLIES.load(Ordering::SeqCst) == 0 { STATE_CHANGED } else { 0 }
		}

		fn apply(&self) {
			APPLIES.fetch_add(1, Ordering::SeqCst);
			self.changed.on_state_changed();
		}

		fn reset(&self) {
			RESETS.fetch_add(1, Ordering::SeqCst);
		}
	}

	fn page() -> PreferencesHandle {
		PreferencesHandle::new(preferences_page_service::<TestPage>())
	}

	#[test]
	fn page_answers_v1_v2_and_v3_queries() {
		let page = page();
		for iid in [PAGE_IID, PAGE_V2_IID, PAGE_V3_IID] {
			assert!(page.answers(&iid), "{iid:?}");
		}
		assert!(!page.answers(&INSTANCE_IID));
	}

	#[test]
	fn name_guid_and_parent_come_from_the_trait() {
		let page = page();
		assert_eq!(page.name(), "foo_librespot");
		assert_eq!(page.guid(), TestPage::GUID);
		assert_eq!(page.parent_guid(), GUID_TOOLS);
		assert!((page.sort_priority() - 0.0).abs() < f64::EPSILON);
		assert!(!page.reset_query());
	}

	#[test]
	fn help_url_is_written() {
		assert_eq!(
			page().help_url().as_deref(),
			Some("https://github.com/LeonarddeR/foo_librespot")
		);
	}

	#[test]
	fn instantiate_releases_the_by_value_callback_when_done() {
		let (callback, destroyed) = FakePageCallback::new();
		let instance = page().instantiate(0x1234, &callback);
		drop(instance);
		drop(callback);
		assert!(destroyed.get() == 1, "the instance kept the callback alive");
	}

	#[test]
	fn instance_state_apply_and_reset_reach_the_page() {
		let (callback, _destroyed) = FakePageCallback::new();
		let instance = page().instantiate(0x1234, &callback);
		assert_eq!(instance.hwnd(), 0x1234);
		assert_eq!(instance.state(), STATE_RESETTABLE | STATE_CHANGED);
		instance.apply();
		assert_eq!(APPLIES.load(Ordering::SeqCst), 1);
		assert_eq!(instance.state(), STATE_RESETTABLE);
		instance.reset();
		assert_eq!(RESETS.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn page_changed_calls_on_state_changed_on_the_host() {
		let (callback, _destroyed) = FakePageCallback::new();
		let instance = page().instantiate(0x1234, &callback);
		instance.apply();
		assert_eq!(callback.calls(), 1);
	}

	#[test]
	fn final_release_destroys_a_window_still_alive() {
		let window = hidden_parent();
		let (callback, _destroyed) = FakePageCallback::new();
		let instance = page().instantiate(window, &callback);
		assert!(unsafe { IsWindow(Some(HWND(window as _))) }.as_bool());
		drop(instance);
		assert!(!unsafe { IsWindow(Some(HWND(window as _))) }.as_bool());
	}
}
