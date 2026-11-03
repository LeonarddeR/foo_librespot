//! Writing track metadata and technical info into foobar2000's `file_info`.

use std::marker::PhantomData;

use crate::abi::file_info::FileInfo as RawFileInfo;
use crate::cxx::try_host;
use crate::error::Result;

/// A borrowed `file_info`.
pub struct FileInfo<'a> {
	raw: *mut RawFileInfo,
	_borrow: PhantomData<&'a mut RawFileInfo>,
}

impl FileInfo<'_> {
	/// # Safety
	/// `raw` must point to a `file_info` that outlives the returned value.
	pub unsafe fn from_raw(raw: *mut RawFileInfo) -> Self {
		Self { raw, _borrow: PhantomData }
	}

	/// Sets the length in seconds; 0 means unknown.
	pub fn set_length(&mut self, seconds: f64) -> Result<()> {
		let raw = self.raw;
		try_host(|| unsafe { ((*(*raw).vtbl).set_length)(raw, seconds) })?;
		Ok(())
	}

	/// Replaces all values of the metadata field `name` with `value`.
	pub fn meta_set(&mut self, name: &str, value: &str) -> Result<()> {
		let raw = self.raw;
		try_host(|| unsafe {
			((*(*raw).vtbl).meta_set_ex)(
				raw,
				name.as_ptr().cast(),
				name.len(),
				value.as_ptr().cast(),
				value.len(),
			);
		})?;
		Ok(())
	}

	/// Adds `value` to the metadata field `name`, keeping its existing values, as the SDK's
	/// `meta_add_ex` does (`SDK\file_info.cpp:405-419`).
	pub fn meta_add(&mut self, name: &str, value: &str) -> Result<()> {
		let raw = self.raw;
		try_host(|| unsafe {
			let vtbl = &*(*raw).vtbl;
			let index = (vtbl.meta_find_ex)(raw, name.as_ptr().cast(), name.len());
			if index == usize::MAX {
				(vtbl.meta_set_nocheck_ex)(
					raw,
					name.as_ptr().cast(),
					name.len(),
					value.as_ptr().cast(),
					value.len(),
				);
			} else {
				let next = (vtbl.meta_enum_value_count)(raw, index);
				(vtbl.meta_insert_value_ex)(raw, index, next, value.as_ptr().cast(), value.len());
			}
		})?;
		Ok(())
	}

	/// Sets the technical info field `name` (shown in Properties > Details).
	pub fn info_set(&mut self, name: &str, value: &str) -> Result<()> {
		let raw = self.raw;
		try_host(|| unsafe {
			((*(*raw).vtbl).info_set_ex)(
				raw,
				name.as_ptr().cast(),
				name.len(),
				value.as_ptr().cast(),
				value.len(),
			);
		})?;
		Ok(())
	}

	/// Sets a numeric technical info field, written in decimal as the SDK's `info_set_int` does.
	pub fn info_set_int(&mut self, name: &str, value: i64) -> Result<()> {
		self.info_set(name, &value.to_string())
	}
}
