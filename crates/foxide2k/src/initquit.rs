//! Startup and shutdown callbacks (`initquit`).

use crate::abi::initquit::{IID, InitQuitVtbl};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::boundary::guard;
use crate::guid::Guid;
use crate::service::{ServiceImpl, ServiceObject};

/// Something to run when foobar2000 starts up or shuts down.
pub trait InitQuit: Send + Sync + 'static {
	/// Called after foobar2000's main window has been created.
	fn on_init(&self) {}
	/// Called before foobar2000's main window is destroyed.
	fn on_quit(&self) {}
}

/// The service that delivers [`InitQuit`] callbacks to `T`.
pub struct InitQuitService<T>(T);

impl<T: InitQuit> InitQuitService<T> {
	pub const fn new(value: T) -> Self {
		Self(value)
	}
}

unsafe impl<T: InitQuit> ServiceImpl for InitQuitService<T> {
	type Vtbl = InitQuitVtbl;
	const IIDS: &'static [Guid] = &[IID];
	const VTBL: &'static InitQuitVtbl = &InitQuitVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		on_init: on_init::<T>,
		on_quit: on_quit::<T>,
	};
}

unsafe extern "C-unwind" fn on_init<T: InitQuit>(this: *mut ServiceBase) {
	unsafe {
		guard(|| {
			ServiceObject::<InitQuitService<T>>::value(this).0.on_init();
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn on_quit<T: InitQuit>(this: *mut ServiceBase) {
	unsafe {
		guard(|| {
			ServiceObject::<InitQuitService<T>>::value(this).0.on_quit();
			Ok(())
		});
	}
}
