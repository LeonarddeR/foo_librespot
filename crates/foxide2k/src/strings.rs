//! Writing to foobar2000's `pfc::string_base` out-parameters.

use std::ffi::{CStr, c_char};

use crate::abi::string_base::{StringBase, StringBaseVtbl};
use crate::boundary::guard;
use crate::cxx::{ExceptionKind, try_host};
use crate::error::{Error, Result};

/// A `pfc::string_base` implemented in Rust, to pass where foobar2000 writes a string out.
#[repr(C)]
pub struct OwnedString {
	base: StringBase,
	/// Always NUL-terminated.
	bytes: Vec<u8>,
}

static OWNED_STRING_VTBL: StringBaseVtbl = StringBaseVtbl {
	add_string: string_add,
	get_ptr: string_get_ptr,
	set_string: string_set,
	truncate: string_truncate,
	get_length: string_get_length,
	lock_buffer: string_lock_buffer,
	unlock_buffer: string_unlock_buffer,
};

impl OwnedString {
	/// An empty string. Boxed, because foobar2000 keeps the address while it writes.
	pub fn new() -> Box<Self> {
		Box::new(Self { base: StringBase { vtbl: &raw const OWNED_STRING_VTBL }, bytes: vec![0] })
	}

	/// The `pfc::string_base &` to pass to a slot.
	pub fn as_raw(&mut self) -> *mut StringBase {
		(&raw mut self.base).cast()
	}

	/// The contents, with invalid UTF-8 replaced.
	pub fn value(&self) -> String {
		String::from_utf8_lossy(&self.bytes[..self.bytes.len() - 1]).into_owned()
	}
}

/// `text` as a NUL-terminated UTF-16 string, for Win32.
pub(crate) fn wide(text: &str) -> Vec<u16> {
	text.encode_utf16().chain([0]).collect()
}

/// Bytes up to the NUL terminator, or up to `length` if that comes first, as pfc reads strings.
pub(crate) unsafe fn bytes_of<'a>(text: *const c_char, length: usize) -> &'a [u8] {
	if text.is_null() {
		return &[];
	}
	let mut len = 0;
	while len < length && unsafe { *text.add(len) } != 0 {
		len += 1;
	}
	unsafe { std::slice::from_raw_parts(text.cast(), len) }
}

unsafe fn owned<'a>(this: *const StringBase) -> &'a mut OwnedString {
	unsafe { &mut *this.cast::<OwnedString>().cast_mut() }
}

unsafe extern "C-unwind" fn string_add(this: *mut StringBase, text: *const c_char, length: usize) {
	unsafe {
		guard(|| {
			// Copied first: the text may point into this string's own buffer.
			let added = bytes_of(text, length).to_vec();
			let string = owned(this);
			string.bytes.pop();
			string.bytes.extend_from_slice(&added);
			string.bytes.push(0);
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn string_get_ptr(this: *const StringBase) -> *const c_char {
	unsafe { guard(|| Ok(owned(this).bytes.as_ptr().cast())) }
}

unsafe extern "C-unwind" fn string_set(this: *mut StringBase, text: *const c_char, length: usize) {
	unsafe {
		guard(|| {
			let mut value = bytes_of(text, length).to_vec();
			value.push(0);
			owned(this).bytes = value;
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn string_truncate(this: *mut StringBase, length: usize) {
	unsafe {
		guard(|| {
			let string = owned(this);
			if length < string.bytes.len() - 1 {
				string.bytes.truncate(length);
				string.bytes.push(0);
			}
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn string_get_length(this: *const StringBase) -> usize {
	unsafe { guard(|| Ok(owned(this).bytes.len() - 1)) }
}

unsafe extern "C-unwind" fn string_lock_buffer(
	this: *mut StringBase,
	length: usize,
) -> *mut c_char {
	unsafe {
		guard(|| {
			let string = owned(this);
			let size = length.checked_add(1).ok_or_else(|| {
				Error::new(ExceptionKind::InvalidParams, "string buffer length overflows")
			})?;
			// pfc hands out a zeroed buffer, and callers may leave the terminator to it.
			string.bytes.clear();
			string.bytes.resize(size, 0);
			Ok(string.bytes.as_mut_ptr().cast())
		})
	}
}

unsafe extern "C-unwind" fn string_unlock_buffer(this: *mut StringBase) {
	unsafe {
		guard(|| {
			let string = owned(this);
			let end = string.bytes.iter().position(|&byte| byte == 0).unwrap_or(string.bytes.len());
			string.bytes.truncate(end);
			string.bytes.push(0);
			Ok(())
		});
	}
}

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
	use super::{OwnedString, get_string, set_string};
	use crate::cxx::try_host;
	use crate::testing::FakeString;

	#[test]
	fn string_out_sets_exact_bytes() {
		let mut out = FakeString::new();
		unsafe { set_string(out.as_raw(), "héllo") }.unwrap();
		assert_eq!(out.value(), "héllo");
	}

	#[test]
	fn owned_string_set_and_append_through_the_vtable() {
		let mut string = OwnedString::new();
		let raw = string.as_raw();
		try_host(|| unsafe {
			((*(*raw).vtbl).set_string)(raw, c"In Christ".as_ptr(), usize::MAX);
			((*(*raw).vtbl).add_string)(raw, c" Alone, and more".as_ptr(), 6);
		})
		.unwrap();
		assert_eq!(string.value(), "In Christ Alone");
		assert_eq!(unsafe { get_string(string.as_raw()) }.unwrap(), "In Christ Alone");
	}

	#[test]
	fn owned_string_truncate_and_length() {
		let mut string = OwnedString::new();
		let raw = string.as_raw();
		let length = try_host(|| unsafe {
			((*(*raw).vtbl).set_string)(raw, c"Getty".as_ptr(), usize::MAX);
			((*(*raw).vtbl).truncate)(raw, 3);
			((*(*raw).vtbl).truncate)(raw, 10);
			((*(*raw).vtbl).get_length)(raw)
		})
		.unwrap();
		assert_eq!(length, 3);
		assert_eq!(string.value(), "Get");
		let buffer_length = try_host(|| unsafe {
			let buffer = ((*(*raw).vtbl).lock_buffer)(raw, 8);
			buffer.copy_from_nonoverlapping(c"Keith".as_ptr(), 6);
			((*(*raw).vtbl).unlock_buffer)(raw);
			((*(*raw).vtbl).get_length)(raw)
		})
		.unwrap();
		assert_eq!((buffer_length, string.value().as_str()), (5, "Keith"));
	}

	#[test]
	fn lock_buffer_hands_out_a_zeroed_buffer_as_pfc_does() {
		let mut string = OwnedString::new();
		let raw = string.as_raw();
		try_host(|| unsafe {
			((*(*raw).vtbl).set_string)(raw, c"Hello World".as_ptr(), usize::MAX);
			// Writes three bytes without a terminator, relying on the zeroed buffer.
			let buffer = ((*(*raw).vtbl).lock_buffer)(raw, 20);
			buffer.copy_from_nonoverlapping(c"abc".as_ptr(), 3);
			((*(*raw).vtbl).unlock_buffer)(raw);
		})
		.unwrap();
		assert_eq!(string.value(), "abc");
	}

	#[test]
	fn lock_buffer_shorter_than_the_string_ends_it_at_the_new_length() {
		let mut string = OwnedString::new();
		let raw = string.as_raw();
		try_host(|| unsafe {
			((*(*raw).vtbl).set_string)(raw, c"Hello".as_ptr(), usize::MAX);
			((*(*raw).vtbl).lock_buffer)(raw, 3);
			((*(*raw).vtbl).unlock_buffer)(raw);
		})
		.unwrap();
		assert_eq!(string.value(), "");
	}

	#[test]
	fn lock_buffer_refuses_a_length_that_overflows() {
		let mut string = OwnedString::new();
		let raw = string.as_raw();
		let error =
			try_host(|| unsafe { ((*(*raw).vtbl).lock_buffer)(raw, usize::MAX) }).unwrap_err();
		assert_eq!(error.kind, crate::cxx::ExceptionKind::InvalidParams);
	}
}
