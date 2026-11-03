//! Writing to foobar2000's `pfc::string_base` out-parameters.

use std::ffi::CStr;

use crate::abi::string_base::StringBase;
use crate::cxx::try_host;
use crate::error::Result;

/// Replaces the contents of a foobar2000 string with `value`.
///
/// # Safety
/// `out` must point to a live `pfc::string_base`.
pub unsafe fn set_string(out: *mut StringBase, value: &str) -> Result<()> {
	try_host(|| unsafe { ((*(*out).vtbl).set_string)(out, value.as_ptr().cast(), value.len()) })?;
	Ok(())
}

/// Reads a foobar2000 string.
///
/// # Safety
/// `string` must point to a live `pfc::string_base`.
pub unsafe fn get_string(string: *const StringBase) -> Result<String> {
	let value = try_host(|| unsafe {
		let pointer = ((*(*string).vtbl).get_ptr)(string);
		if pointer.is_null() {
			String::new()
		} else {
			CStr::from_ptr(pointer).to_string_lossy().into_owned()
		}
	})?;
	Ok(value)
}

#[cfg(test)]
mod tests {
	use super::set_string;
	use crate::testing::FakeString;

	#[test]
	fn string_out_sets_exact_bytes() {
		let mut out = FakeString::new();
		unsafe { set_string(out.as_raw(), "héllo") }.unwrap();
		assert_eq!(out.value(), "héllo");
	}
}
