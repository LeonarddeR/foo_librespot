//! foobar2000's settings store, for values a component keeps between sessions.

use std::ffi::{CStr, c_char};

use crate::abi::config_store::{ConfigStoreVtbl, IID};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string::{self, StringVtbl};
use crate::cxx::try_host;
use crate::error::Result;
use crate::guid::Guid;
use crate::host::host;
use crate::service::{ByValueServicePtr, ServiceImpl, ServicePtr, Singleton};
use crate::strings::c_string;

fn store() -> Result<ServicePtr> {
	host().require(&IID, "configStore")
}

/// The `fb2k::string` holding "", which [`get_string`] passes as the default.
struct EmptyString;

unsafe impl ServiceImpl for EmptyString {
	type Vtbl = StringVtbl;
	const IIDS: &'static [Guid] = &[string::IID];
	const VTBL: &'static StringVtbl =
		&StringVtbl { base: ServiceBaseVtbl::new::<Self>(), c_str: empty_c_str };
}

unsafe extern "C-unwind" fn empty_c_str(_this: *mut ServiceBase) -> *const c_char {
	c"".as_ptr()
}

static EMPTY: Singleton<EmptyString> = Singleton::new(EmptyString);

/// The integer stored under `name`, or `default` when there is none, no host, or the host
/// fails.
pub fn get_int(name: &str, default: i64) -> i64 {
	let (Ok(store), Ok(name)) = (store(), c_string(name, "a setting name")) else { return default };
	let raw = store.as_raw();
	try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<ConfigStoreVtbl>();
		(vtbl.get_config_int)(raw, name.as_ptr(), default)
	})
	.unwrap_or(default)
}

/// Stores `value` under `name`.
pub fn set_int(name: &str, value: i64) -> Result<()> {
	let store = store()?;
	let name = c_string(name, "a setting name")?;
	let raw = store.as_raw();
	try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<ConfigStoreVtbl>();
		(vtbl.set_config_int)(raw, name.as_ptr(), value);
	})?;
	Ok(())
}

/// The string stored under `name`; `None` when there is none, no host, or the host fails. A
/// stored empty string reads as `None` too.
pub fn get_string(name: &str) -> Option<String> {
	let store = store().ok()?;
	let name = c_string(name, "a setting name").ok()?;
	let raw = store.as_raw();
	// The by-value default: a temporary holding one reference, which the callee releases.
	// Releasing the singleton does nothing, so a throw can't leak or free it whoever releases it.
	let mut default = EMPTY.service().into_raw();
	let mut answer: *mut ServiceBase = std::ptr::null_mut();
	let answer_slot = &raw mut answer;
	try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<ConfigStoreVtbl>();
		(vtbl.get_config_string)(
			raw,
			answer_slot,
			name.as_ptr(),
			ByValueServicePtr::for_callee(&raw mut default),
		);
	})
	.ok()?;
	// Once the call has returned, the hidden return value holds a reference of ours; a call that
	// threw built none for us to release.
	let answer = unsafe { ServicePtr::from_raw_owned(answer) }?;
	let raw = answer.as_raw();
	let text = try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<StringVtbl>();
		let text = (vtbl.c_str)(raw);
		// The text belongs to `answer`, which is alive until it is copied.
		(!text.is_null()).then(|| CStr::from_ptr(text).to_string_lossy().into_owned())
	})
	.ok()??;
	Some(text).filter(|text| !text.is_empty())
}

/// Stores `value` under `name`.
pub fn set_string(name: &str, value: &str) -> Result<()> {
	let store = store()?;
	let name = c_string(name, "a setting name")?;
	let value = c_string(value, "a setting value")?;
	let raw = store.as_raw();
	try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<ConfigStoreVtbl>();
		(vtbl.set_config_string)(raw, name.as_ptr(), value.as_ptr());
	})?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::{get_int, get_string, set_int, set_string};
	use crate::testing::FakeApi;

	const HISTORY: &str = "foo_librespot.search_history";

	#[test]
	fn config_string_round_trips_and_is_none_when_absent() {
		let api = FakeApi::new();
		api.install();
		assert_eq!(get_string(HISTORY), None);
		set_string(HISTORY, "in christ alone\nbach cantatas").unwrap();
		assert_eq!(get_string(HISTORY).as_deref(), Some("in christ alone\nbach cantatas"));
		assert_eq!(api.config_string(HISTORY).as_deref(), Some("in christ alone\nbach cantatas"));
	}

	#[test]
	fn a_stored_empty_config_string_reads_as_none() {
		let api = FakeApi::new();
		api.install();
		set_string(HISTORY, "").unwrap();
		assert_eq!(api.config_string(HISTORY).as_deref(), Some(""));
		assert_eq!(get_string(HISTORY), None);
	}

	#[test]
	fn reading_a_config_string_releases_the_answer() {
		let api = FakeApi::new();
		api.install();
		set_string(HISTORY, "in christ alone").unwrap();
		assert_eq!(get_string(HISTORY).as_deref(), Some("in christ alone"));
		assert_eq!(api.live_config_strings(), 0);
	}

	#[test]
	fn config_string_without_host_is_none_and_setting_fails() {
		assert_eq!(get_string(HISTORY), None);
		assert!(set_string(HISTORY, "in christ alone").is_err());
	}

	#[test]
	fn config_int_round_trips_and_defaults() {
		let api = FakeApi::new();
		api.install();
		assert_eq!(get_int("foo_librespot.bitrate", 320), 320);
		set_int("foo_librespot.bitrate", 160).unwrap();
		assert_eq!(get_int("foo_librespot.bitrate", 320), 160);
		assert_eq!(api.config_int("foo_librespot.bitrate"), Some(160));
	}

	#[test]
	fn config_without_host_gives_the_default() {
		assert_eq!(get_int("foo_librespot.bitrate", 320), 320);
		assert!(set_int("foo_librespot.bitrate", 160).is_err());
	}
}
