//! foobar2000's settings store, for values a component keeps between sessions.

use crate::abi::config_store::{ConfigStoreVtbl, IID};
use crate::cxx::try_host;
use crate::error::Result;
use crate::host::host;
use crate::service::ServicePtr;
use crate::strings::c_string;

fn store() -> Result<ServicePtr> {
	host().require(&IID, "configStore")
}

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

#[cfg(test)]
mod tests {
	use super::{get_int, set_int};
	use crate::testing::FakeApi;

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
