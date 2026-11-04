//! Reference-counted service objects implemented in Rust, and owning pointers to any service.

use std::ptr::NonNull;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::guid::Guid;

/// An owning pointer to a foobar2000 service, like the SDK's `service_ptr_t`. Cloning adds a
/// reference and dropping releases one.
pub struct ServicePtr {
	raw: NonNull<ServiceBase>,
}

// Service reference counts are atomic on both sides of the boundary, and foobar2000 services
// are designed to be handed between threads.
unsafe impl Send for ServicePtr {}
unsafe impl Sync for ServicePtr {}

impl ServicePtr {
	/// Takes over a reference the caller already owns. Returns `None` for null.
	///
	/// # Safety
	/// `raw` must be null or point to a live service object whose reference the caller owns.
	pub unsafe fn from_raw_owned(raw: *mut ServiceBase) -> Option<Self> {
		NonNull::new(raw).map(|raw| Self { raw })
	}

	/// Adds a reference to a service the caller only borrows. Returns `None` for null.
	///
	/// # Safety
	/// `raw` must be null or point to a live service object.
	pub unsafe fn from_raw_add_ref(raw: *mut ServiceBase) -> Option<Self> {
		let raw = NonNull::new(raw)?;
		unsafe { (vtbl(raw.as_ptr()).service_add_ref)(raw.as_ptr()) };
		Some(Self { raw })
	}

	/// The raw pointer, still owned by `self`.
	pub fn as_raw(&self) -> *mut ServiceBase {
		self.raw.as_ptr()
	}

	/// Gives up ownership of the reference without releasing it.
	pub fn into_raw(self) -> *mut ServiceBase {
		let raw = self.raw.as_ptr();
		std::mem::forget(self);
		raw
	}

	/// Asks the service for another interface it implements, like `service_query_t`.
	pub fn query(&self, iid: &Guid) -> Option<Self> {
		let mut out: *mut ServiceBase = std::ptr::null_mut();
		let found =
			unsafe { (vtbl(self.as_raw()).service_query)(self.as_raw(), &raw mut out, iid) };
		if found { unsafe { Self::from_raw_owned(out) } } else { None }
	}
}

impl Clone for ServicePtr {
	fn clone(&self) -> Self {
		unsafe { (vtbl(self.as_raw()).service_add_ref)(self.as_raw()) };
		Self { raw: self.raw }
	}
}

impl Drop for ServicePtr {
	fn drop(&mut self) {
		unsafe { (vtbl(self.as_raw()).service_release)(self.as_raw()) };
	}
}

impl std::fmt::Debug for ServicePtr {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_tuple("ServicePtr").field(&self.raw).finish()
	}
}

unsafe fn vtbl<'a>(raw: *mut ServiceBase) -> &'a ServiceBaseVtbl {
	unsafe { &*(*raw).vtbl }
}

/// A Rust type that foobar2000 can use as a service.
///
/// # Safety
/// `Vtbl` must be `#[repr(C)]` and begin with a [`ServiceBaseVtbl`] built by
/// [`ServiceBaseVtbl::new::<Self>`], followed by the slots of every interface in `IIDS`, in SDK
/// order. Every slot must treat `this` as a [`ServiceObject<Self>`].
pub unsafe trait ServiceImpl: Send + Sync + Sized + 'static {
	type Vtbl: 'static;
	/// The interfaces `service_query` answers, most-derived first.
	const IIDS: &'static [Guid];
	const VTBL: &'static Self::Vtbl;
}

/// The memory layout of a Rust-implemented service. The vtable pointer comes first, so a pointer
/// to the object is also a pointer to its [`ServiceBase`].
#[repr(C)]
pub struct ServiceObject<T: ServiceImpl> {
	vtbl: *const T::Vtbl,
	refs: AtomicI32,
	singleton: bool,
	value: T,
}

impl<T: ServiceImpl> ServiceObject<T> {
	/// The Rust value behind a service pointer that came from one of `T`'s vtable slots.
	///
	/// # Safety
	/// `this` must point to a live `ServiceObject<T>`.
	pub unsafe fn value<'a>(this: *mut ServiceBase) -> &'a T {
		unsafe { &(*this.cast::<Self>()).value }
	}
}

impl ServiceBaseVtbl {
	/// The `service_base` slots for a Rust service of type `T`.
	pub const fn new<T: ServiceImpl>() -> Self {
		Self {
			service_release: release::<T>,
			service_add_ref: add_ref::<T>,
			service_query: query::<T>,
		}
	}
}

unsafe extern "C-unwind" fn release<T: ServiceImpl>(this: *mut ServiceBase) -> i32 {
	let object = this.cast::<ServiceObject<T>>();
	unsafe {
		if (*object).singleton {
			return 1;
		}
		let remaining = (*object).refs.fetch_sub(1, Ordering::AcqRel) - 1;
		if remaining == 0 {
			free_from_release(object);
		}
		remaining
	}
}

/// Frees an object from a `service_release` slot. foobar2000 declares that slot noexcept, so
/// it can't run inside [`crate::boundary::guard`], which reports a panic by throwing; a panicking
/// destructor aborts the process instead of unwinding into foobar2000.
///
/// # Safety
/// `object` must come from `Box::into_raw` and must not be used afterwards.
pub(crate) unsafe fn free_from_release<T>(object: *mut T) {
	let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
		drop(unsafe { Box::from_raw(object) });
	}));
	if dropped.is_err() {
		std::process::abort();
	}
}

/// A `service_ptr_t` passed by value. MSVC passes a pointer to a temporary the caller owns, and the
/// callee destroys it, which here means releasing the reference it holds. Dropping this does that,
/// so a slot that takes one releases it on every path, and [`Self::into_ptr`] takes the reference
/// over instead. A caller moves it into foobar2000's call, and foobar2000 destroys it.
#[repr(transparent)]
pub struct ByValueServicePtr(*mut *mut ServiceBase);

impl ByValueServicePtr {
	/// # Safety
	/// `temporary` must point to a `service_ptr_t` holding one reference or null, which the callee
	/// is meant to destroy.
	pub const unsafe fn for_callee(temporary: *mut *mut ServiceBase) -> Self {
		Self(temporary)
	}

	/// The reference, which the caller now owns; `None` when the pointer is null.
	pub fn into_ptr(self) -> Option<ServicePtr> {
		let this = std::mem::ManuallyDrop::new(self);
		this.take()
	}

	fn take(&self) -> Option<ServicePtr> {
		if self.0.is_null() {
			return None;
		}
		// SAFETY: `for_callee`'s contract; the temporary is left null, so it is released once.
		unsafe { ServicePtr::from_raw_owned(std::mem::replace(&mut *self.0, std::ptr::null_mut())) }
	}
}

impl Drop for ByValueServicePtr {
	fn drop(&mut self) {
		drop(self.take());
	}
}

unsafe extern "C-unwind" fn add_ref<T: ServiceImpl>(this: *mut ServiceBase) -> i32 {
	let object = this.cast::<ServiceObject<T>>();
	unsafe {
		if (*object).singleton {
			return 1;
		}
		(*object).refs.fetch_add(1, Ordering::Relaxed) + 1
	}
}

unsafe extern "C-unwind" fn query<T: ServiceImpl>(
	this: *mut ServiceBase,
	out: *mut *mut ServiceBase,
	guid: *const Guid,
) -> bool {
	unsafe {
		if !T::IIDS.contains(&*guid) {
			return false;
		}
		// Same order as service_ptr_t::copy: add the new reference before releasing the old one.
		add_ref::<T>(this);
		let old = *out;
		*out = this;
		if !old.is_null() {
			(vtbl(old).service_release)(old);
		}
		true
	}
}

/// Creates a heap-allocated service with one reference, owned by the returned pointer.
pub fn new_service<T: ServiceImpl>(value: T) -> ServicePtr {
	let object =
		Box::new(ServiceObject { vtbl: T::VTBL, refs: AtomicI32::new(1), singleton: false, value });
	let raw = Box::into_raw(object).cast::<ServiceBase>();
	unsafe { ServicePtr::from_raw_owned(raw) }.expect("Box::into_raw is never null")
}

/// A service that lives for the whole process, like the SDK's `service_impl_single_t`: adding
/// and releasing references does nothing.
pub struct Singleton<T: ServiceImpl>(ServiceObject<T>);

// The object is only ever shared, and `T: Sync` makes sharing its value safe.
unsafe impl<T: ServiceImpl> Sync for Singleton<T> {}

impl<T: ServiceImpl> Singleton<T> {
	pub const fn new(value: T) -> Self {
		Self(ServiceObject { vtbl: T::VTBL, refs: AtomicI32::new(1), singleton: true, value })
	}

	/// A pointer to the singleton. Releasing it is a no-op.
	pub fn service(&'static self) -> ServicePtr {
		let raw = (&raw const self.0).cast_mut().cast::<ServiceBase>();
		unsafe { ServicePtr::from_raw_owned(raw) }.expect("a static is never null")
	}

	pub fn value(&'static self) -> &'static T {
		&self.0.value
	}

	/// The singleton as a raw service pointer, for building static tables.
	pub(crate) const fn as_raw(&'static self) -> *const ServiceBase {
		(&raw const self.0).cast()
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::sync::atomic::{AtomicUsize, Ordering};

	use super::{
		ByValueServicePtr, ServiceImpl, ServicePtr, Singleton, free_from_release, new_service,
	};
	use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
	use crate::guid::Guid;

	struct Counted(Arc<AtomicUsize>);

	impl Drop for Counted {
		fn drop(&mut self) {
			self.0.fetch_add(1, Ordering::SeqCst);
		}
	}

	#[test]
	fn free_from_release_drops_the_object() {
		let drops = Arc::new(AtomicUsize::new(0));
		unsafe { free_from_release(Box::into_raw(Box::new(Counted(Arc::clone(&drops))))) };
		assert_eq!(drops.load(Ordering::SeqCst), 1);
	}

	const PANIC_CHILD: &str = "FOXIDE2K_RELEASE_PANIC_CHILD";

	#[test]
	fn a_panicking_destructor_in_release_aborts_instead_of_unwinding() {
		if std::env::var_os(PANIC_CHILD).is_some() {
			struct Panics;
			impl Drop for Panics {
				fn drop(&mut self) {
					panic!("destructor panicked");
				}
			}
			unsafe { free_from_release(Box::into_raw(Box::new(Panics))) };
			return;
		}
		// Run this test again in a child process, where the abort can't take the runner down.
		let status = std::process::Command::new(std::env::current_exe().unwrap())
			.args([
				"--exact",
				"service::tests::a_panicking_destructor_in_release_aborts_instead_of_unwinding",
				"--nocapture",
			])
			.env(PANIC_CHILD, "1")
			.stdout(std::process::Stdio::null())
			.stderr(std::process::Stdio::null())
			.status()
			.unwrap();
		// An unwound panic fails the test and the harness exits with 101; an abort doesn't get
		// that far. Returning normally would mean the panic was swallowed.
		assert!(!status.success(), "{status:?}");
		assert_ne!(status.code(), Some(101), "{status:?}");
	}

	const IID_A: Guid = Guid::from_u128(0x11111111_0000_0000_0000_000000000001);
	const IID_B: Guid = Guid::from_u128(0x22222222_0000_0000_0000_000000000002);
	const IID_CHILD: Guid = Guid::from_u128(0x33333333_0000_0000_0000_000000000003);
	const IID_PARENT: Guid = Guid::from_u128(0x44444444_0000_0000_0000_000000000004);
	const IID_UNKNOWN: Guid = Guid::from_u128(0x55555555_0000_0000_0000_000000000005);

	struct ServiceA {
		drops: Arc<AtomicUsize>,
	}

	impl Drop for ServiceA {
		fn drop(&mut self) {
			self.drops.fetch_add(1, Ordering::SeqCst);
		}
	}

	unsafe impl ServiceImpl for ServiceA {
		type Vtbl = ServiceBaseVtbl;
		const IIDS: &'static [Guid] = &[IID_A];
		const VTBL: &'static ServiceBaseVtbl = &ServiceBaseVtbl::new::<Self>();
	}

	#[test]
	fn a_by_value_argument_releases_its_reference_when_dropped() {
		let drops = Arc::new(AtomicUsize::new(0));
		let mut temporary = new_service(ServiceA { drops: Arc::clone(&drops) }).into_raw();
		drop(unsafe { ByValueServicePtr::for_callee(&raw mut temporary) });
		assert_eq!(drops.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn into_ptr_hands_the_reference_to_the_slot() {
		let drops = Arc::new(AtomicUsize::new(0));
		let mut temporary = new_service(ServiceA { drops: Arc::clone(&drops) }).into_raw();
		let owned = unsafe { ByValueServicePtr::for_callee(&raw mut temporary) }.into_ptr();
		assert_eq!(drops.load(Ordering::SeqCst), 0);
		drop(owned);
		assert_eq!(drops.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn a_null_by_value_argument_holds_nothing() {
		let mut temporary = std::ptr::null_mut();
		assert!(unsafe { ByValueServicePtr::for_callee(&raw mut temporary) }.into_ptr().is_none());
	}

	struct ServiceB {
		drops: Arc<AtomicUsize>,
	}

	impl Drop for ServiceB {
		fn drop(&mut self) {
			self.drops.fetch_add(1, Ordering::SeqCst);
		}
	}

	unsafe impl ServiceImpl for ServiceB {
		type Vtbl = ServiceBaseVtbl;
		const IIDS: &'static [Guid] = &[IID_B];
		const VTBL: &'static ServiceBaseVtbl = &ServiceBaseVtbl::new::<Self>();
	}

	struct Chained;

	unsafe impl ServiceImpl for Chained {
		type Vtbl = ServiceBaseVtbl;
		const IIDS: &'static [Guid] = &[IID_CHILD, IID_PARENT];
		const VTBL: &'static ServiceBaseVtbl = &ServiceBaseVtbl::new::<Self>();
	}

	static SINGLETON: Singleton<Chained> = Singleton::new(Chained);

	unsafe fn vtbl(raw: *mut ServiceBase) -> &'static ServiceBaseVtbl {
		unsafe { &*(*raw).vtbl }
	}

	unsafe fn query_raw(raw: *mut ServiceBase, out: &mut *mut ServiceBase, iid: &Guid) -> bool {
		unsafe { (vtbl(raw).service_query)(raw, out, iid) }
	}

	#[test]
	fn new_service_has_one_reference() {
		let drops = Arc::new(AtomicUsize::new(0));
		let service = new_service(ServiceA { drops: drops.clone() });
		let raw = service.as_raw();
		unsafe {
			assert_eq!((vtbl(raw).service_add_ref)(raw), 2);
			assert_eq!((vtbl(raw).service_release)(raw), 1);
		}
		drop(service);
		assert_eq!(drops.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn last_release_drops_value() {
		let drops = Arc::new(AtomicUsize::new(0));
		let first = new_service(ServiceA { drops: drops.clone() });
		let second = first.clone();
		drop(first);
		assert_eq!(drops.load(Ordering::SeqCst), 0);
		drop(second);
		assert_eq!(drops.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn query_known_guid_add_refs_and_replaces_out() {
		let a_drops = Arc::new(AtomicUsize::new(0));
		let b_drops = Arc::new(AtomicUsize::new(0));
		let a = new_service(ServiceA { drops: a_drops.clone() });
		let b = new_service(ServiceB { drops: b_drops.clone() });
		let a_raw = a.as_raw();
		// `out` owns the only reference to B, as a service_ptr_t would.
		let mut out = b.into_raw();
		unsafe {
			assert!(query_raw(a_raw, &mut out, &IID_A));
			assert_eq!(out, a_raw);
			assert_eq!(b_drops.load(Ordering::SeqCst), 1, "previous value of out was released");
			// A now has two references: `a` and `out`.
			assert_eq!((vtbl(a_raw).service_release)(out), 1);
		}
		drop(a);
		assert_eq!(a_drops.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn query_unknown_guid_leaves_out_untouched() {
		let a = new_service(ServiceA { drops: Arc::new(AtomicUsize::new(0)) });
		let sentinel = 0x1234 as *mut ServiceBase;
		let mut out = sentinel;
		unsafe {
			assert!(!query_raw(a.as_raw(), &mut out, &IID_UNKNOWN));
		}
		assert_eq!(out, sentinel);
	}

	#[test]
	fn query_matches_parent_guids_in_chain() {
		let chained = new_service(Chained);
		assert!(chained.query(&IID_CHILD).is_some());
		assert!(chained.query(&IID_PARENT).is_some());
		assert!(chained.query(&IID_UNKNOWN).is_none());
	}

	#[test]
	fn singleton_reference_count_is_constant() {
		let ptr: ServicePtr = SINGLETON.service();
		let raw = ptr.as_raw();
		unsafe {
			assert_eq!((vtbl(raw).service_add_ref)(raw), 1);
			assert_eq!((vtbl(raw).service_release)(raw), 1);
			assert_eq!((vtbl(raw).service_release)(raw), 1);
		}
		drop(ptr);
		assert!(SINGLETON.service().query(&IID_PARENT).is_some());
	}
}
