//! A `file_info` owned by Rust, for passing prefilled track info to foobar2000.
//!
//! `file_info` is not a service: foobar2000 only calls it through its vtable for as long as the
//! call that received it lasts, so a `Box<FileInfoImpl>` on the caller's side is enough. Its
//! behaviour follows the SDK's `file_info_impl` (`SDK\file_info_impl.cpp`).

use std::ffi::{CStr, CString, c_char, c_void};

use crate::abi::file_info::{FileInfo as RawFileInfo, FileInfoVtbl, ReplayGainInfo};
use crate::boundary::guard;
use crate::cxx::try_host;
use crate::error::Result;
use crate::file_info::FileInfo;
use crate::strings::bytes_of;

/// The SDK's "not set" ReplayGain: gains of `gain_invalid` (-1000) and peaks of `peak_invalid`
/// (-1), as in `SDK\file_info.h`.
pub const REPLAYGAIN_UNSET: ReplayGainInfo =
	ReplayGainInfo { album_gain: -1000.0, track_gain: -1000.0, album_peak: -1.0, track_peak: -1.0 };

/// Track metadata, technical info, length and ReplayGain. Field names match case-insensitively,
/// as in foobar2000.
#[repr(C)]
pub struct FileInfoImpl {
	base: RawFileInfo,
	length: f64,
	replaygain: ReplayGainInfo,
	meta: Vec<(CString, Vec<CString>)>,
	info: Vec<(CString, CString)>,
}

impl FileInfoImpl {
	pub fn new() -> Box<Self> {
		Box::new(Self {
			base: RawFileInfo { vtbl: &raw const VTBL },
			length: 0.0,
			replaygain: REPLAYGAIN_UNSET,
			meta: Vec::new(),
			info: Vec::new(),
		})
	}

	/// The `file_info &` to pass to foobar2000.
	pub fn as_raw(&mut self) -> *mut RawFileInfo {
		(&raw mut self.base).cast()
	}

	/// Writes through the same vtable foobar2000 uses.
	pub fn file_info(&mut self) -> FileInfo<'_> {
		// SAFETY: `self` outlives the returned borrow.
		unsafe { FileInfo::from_raw(self.as_raw()) }
	}

	pub fn length(&self) -> f64 {
		self.length
	}

	pub fn replaygain(&self) -> ReplayGainInfo {
		self.replaygain
	}

	/// All values of the metadata field `name`.
	pub fn meta(&self, name: &str) -> Vec<String> {
		self.meta_index(name.as_bytes())
			.map(|index| {
				self.meta[index]
					.1
					.iter()
					.map(|value| value.to_string_lossy().into_owned())
					.collect()
			})
			.unwrap_or_default()
	}

	/// The technical info field `name`.
	pub fn info(&self, name: &str) -> Option<String> {
		self.info_index(name.as_bytes())
			.map(|index| self.info[index].1.to_string_lossy().into_owned())
	}

	fn meta_index(&self, name: &[u8]) -> Option<usize> {
		self.meta.iter().position(|(field, _)| field.to_bytes().eq_ignore_ascii_case(name))
	}

	fn info_index(&self, name: &[u8]) -> Option<usize> {
		self.info.iter().position(|(field, _)| field.to_bytes().eq_ignore_ascii_case(name))
	}
}

static VTBL: FileInfoVtbl = FileInfoVtbl {
	get_length,
	set_length,
	set_replaygain,
	get_replaygain,
	meta_get_count,
	meta_enum_name,
	meta_enum_value_count,
	meta_enum_value,
	meta_find_ex,
	meta_set_ex: meta_set,
	meta_insert_value_ex,
	meta_remove_mask,
	meta_reorder,
	meta_remove_values,
	meta_modify_value_ex,
	info_get_count,
	info_enum_name,
	info_enum_value,
	info_set_ex: info_set,
	info_remove_mask,
	info_find_ex,
	copy,
	copy_meta,
	copy_info,
	meta_set_nocheck_ex: meta_set,
	info_set_nocheck_ex: info_set,
};

/// The one slot of `pfc::bit_array` used here: `virtual bool get(t_size n) const`, its first
/// virtual (`pfc\bit_array.h`).
#[repr(C)]
struct BitArrayVtbl {
	get: unsafe extern "C-unwind" fn(this: *const c_void, index: usize) -> bool,
}

/// Asks a `bit_array` whether `index` is set.
unsafe fn bit(mask: *const c_void, index: usize) -> Result<bool> {
	let vtbl = unsafe { &**mask.cast::<*const BitArrayVtbl>() };
	Ok(try_host(|| unsafe { (vtbl.get)(mask, index) })?)
}

unsafe fn state<'a>(this: *const RawFileInfo) -> &'a mut FileInfoImpl {
	// SAFETY: every `this` handed to these slots is the `base` at offset 0 of a `FileInfoImpl`.
	unsafe { &mut *this.cast::<FileInfoImpl>().cast_mut() }
}

/// Bytes up to the NUL terminator, or up to `length` if that comes first, as pfc reads strings.
unsafe fn owned(text: *const c_char, length: usize) -> CString {
	// `bytes_of` stops at the first NUL, so this cannot fail.
	CString::new(unsafe { bytes_of(text, length) }).unwrap_or_default()
}

fn pointer(text: Option<&CString>) -> *const c_char {
	text.map_or(std::ptr::null(), |text| text.as_ptr())
}

unsafe extern "C-unwind" fn get_length(this: *const RawFileInfo) -> f64 {
	unsafe { guard(|| Ok(state(this).length)) }
}

unsafe extern "C-unwind" fn set_length(this: *mut RawFileInfo, length: f64) {
	unsafe {
		guard(|| {
			state(this).length = length;
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn set_replaygain(this: *mut RawFileInfo, value: *const ReplayGainInfo) {
	unsafe {
		guard(|| {
			state(this).replaygain = *value;
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn get_replaygain(
	this: *const RawFileInfo,
	ret: *mut ReplayGainInfo,
) -> *mut ReplayGainInfo {
	unsafe {
		guard(|| {
			ret.write(state(this).replaygain);
			Ok(ret)
		})
	}
}

unsafe extern "C-unwind" fn meta_get_count(this: *const RawFileInfo) -> usize {
	unsafe { guard(|| Ok(state(this).meta.len())) }
}

unsafe extern "C-unwind" fn meta_enum_name(
	this: *const RawFileInfo,
	index: usize,
) -> *const c_char {
	unsafe { guard(|| Ok(pointer(state(this).meta.get(index).map(|(name, _)| name)))) }
}

unsafe extern "C-unwind" fn meta_enum_value_count(this: *const RawFileInfo, index: usize) -> usize {
	unsafe { guard(|| Ok(state(this).meta.get(index).map_or(0, |(_, values)| values.len()))) }
}

unsafe extern "C-unwind" fn meta_enum_value(
	this: *const RawFileInfo,
	index: usize,
	value: usize,
) -> *const c_char {
	unsafe {
		guard(|| Ok(pointer(state(this).meta.get(index).and_then(|(_, values)| values.get(value)))))
	}
}

unsafe extern "C-unwind" fn meta_find_ex(
	this: *const RawFileInfo,
	name: *const c_char,
	length: usize,
) -> usize {
	unsafe { guard(|| Ok(state(this).meta_index(bytes_of(name, length)).unwrap_or(usize::MAX))) }
}

unsafe extern "C-unwind" fn meta_set(
	this: *mut RawFileInfo,
	name: *const c_char,
	name_length: usize,
	value: *const c_char,
	value_length: usize,
) -> usize {
	unsafe {
		guard(|| {
			let info = state(this);
			let value = owned(value, value_length);
			if let Some(index) = info.meta_index(bytes_of(name, name_length)) {
				info.meta[index].1 = vec![value];
				return Ok(index);
			}
			info.meta.push((owned(name, name_length), vec![value]));
			Ok(info.meta.len() - 1)
		})
	}
}

unsafe extern "C-unwind" fn meta_insert_value_ex(
	this: *mut RawFileInfo,
	index: usize,
	value_index: usize,
	value: *const c_char,
	value_length: usize,
) {
	unsafe {
		guard(|| {
			let value = owned(value, value_length);
			if let Some((_, values)) = state(this).meta.get_mut(index) {
				values.insert(value_index.min(values.len()), value);
			}
			Ok(())
		});
	}
}

/// Keeps the items whose index `mask` doesn't mark.
unsafe fn retain_unmarked<T>(items: &mut Vec<T>, mask: *const c_void) -> Result<()> {
	// Every mark is read first, so a mask that throws leaves the items as they were.
	let marked =
		(0..items.len()).map(|index| unsafe { bit(mask, index) }).collect::<Result<Vec<bool>>>()?;
	let mut marks = marked.into_iter();
	items.retain(|_| !marks.next().unwrap_or(false));
	Ok(())
}

unsafe extern "C-unwind" fn meta_remove_mask(this: *mut RawFileInfo, mask: *const c_void) {
	unsafe { guard(|| retain_unmarked(&mut state(this).meta, mask)) }
}

unsafe extern "C-unwind" fn meta_reorder(this: *mut RawFileInfo, order: *const usize) {
	unsafe {
		guard(|| {
			let info = state(this);
			let order = std::slice::from_raw_parts(order, info.meta.len());
			let old = std::mem::take(&mut info.meta);
			info.meta = order.iter().filter_map(|&index| old.get(index).cloned()).collect();
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn meta_remove_values(
	this: *mut RawFileInfo,
	index: usize,
	mask: *const c_void,
) {
	unsafe {
		guard(|| {
			let info = state(this);
			let Some((_, values)) = info.meta.get_mut(index) else {
				return Ok(());
			};
			retain_unmarked(values, mask)?;
			// As file_info_impl: a field left without values is removed.
			if values.is_empty() {
				info.meta.remove(index);
			}
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn meta_modify_value_ex(
	this: *mut RawFileInfo,
	index: usize,
	value_index: usize,
	value: *const c_char,
	value_length: usize,
) {
	unsafe {
		guard(|| {
			let value = owned(value, value_length);
			if let Some(slot) =
				state(this).meta.get_mut(index).and_then(|(_, values)| values.get_mut(value_index))
			{
				*slot = value;
			}
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn info_get_count(this: *const RawFileInfo) -> usize {
	unsafe { guard(|| Ok(state(this).info.len())) }
}

unsafe extern "C-unwind" fn info_enum_name(
	this: *const RawFileInfo,
	index: usize,
) -> *const c_char {
	unsafe { guard(|| Ok(pointer(state(this).info.get(index).map(|(name, _)| name)))) }
}

unsafe extern "C-unwind" fn info_enum_value(
	this: *const RawFileInfo,
	index: usize,
) -> *const c_char {
	unsafe { guard(|| Ok(pointer(state(this).info.get(index).map(|(_, value)| value)))) }
}

unsafe extern "C-unwind" fn info_set(
	this: *mut RawFileInfo,
	name: *const c_char,
	name_length: usize,
	value: *const c_char,
	value_length: usize,
) -> usize {
	unsafe {
		guard(|| {
			let info = state(this);
			let value = owned(value, value_length);
			if let Some(index) = info.info_index(bytes_of(name, name_length)) {
				info.info[index].1 = value;
				return Ok(index);
			}
			info.info.push((owned(name, name_length), value));
			Ok(info.info.len() - 1)
		})
	}
}

unsafe extern "C-unwind" fn info_remove_mask(this: *mut RawFileInfo, mask: *const c_void) {
	unsafe { guard(|| retain_unmarked(&mut state(this).info, mask)) }
}

unsafe extern "C-unwind" fn info_find_ex(
	this: *const RawFileInfo,
	name: *const c_char,
	length: usize,
) -> usize {
	unsafe { guard(|| Ok(state(this).info_index(bytes_of(name, length)).unwrap_or(usize::MAX))) }
}

/// Copies a NUL-terminated string a `file_info` returned; null reads as empty.
unsafe fn copied(text: *const c_char) -> CString {
	if text.is_null() { CString::default() } else { unsafe { CStr::from_ptr(text) }.to_owned() }
}

/// The metadata of any `file_info`, read through its vtable.
pub(crate) unsafe fn read_meta(source: *const RawFileInfo) -> Result<Vec<(CString, Vec<CString>)>> {
	let vtbl = unsafe { &*(*source).vtbl };
	let count = try_host(|| unsafe { (vtbl.meta_get_count)(source) })?;
	let mut meta = Vec::with_capacity(count);
	for index in 0..count {
		let name = try_host(|| unsafe { copied((vtbl.meta_enum_name)(source, index)) })?;
		let value_count = try_host(|| unsafe { (vtbl.meta_enum_value_count)(source, index) })?;
		let mut values = Vec::with_capacity(value_count);
		for value in 0..value_count {
			values.push(try_host(|| unsafe {
				copied((vtbl.meta_enum_value)(source, index, value))
			})?);
		}
		meta.push((name, values));
	}
	Ok(meta)
}

/// The technical info of any `file_info`, read through its vtable.
pub(crate) unsafe fn read_info(source: *const RawFileInfo) -> Result<Vec<(CString, CString)>> {
	let vtbl = unsafe { &*(*source).vtbl };
	let count = try_host(|| unsafe { (vtbl.info_get_count)(source) })?;
	let mut info = Vec::with_capacity(count);
	for index in 0..count {
		let name = try_host(|| unsafe { copied((vtbl.info_enum_name)(source, index)) })?;
		let value = try_host(|| unsafe { copied((vtbl.info_enum_value)(source, index)) })?;
		info.push((name, value));
	}
	Ok(info)
}

unsafe extern "C-unwind" fn copy_meta(this: *mut RawFileInfo, source: *const RawFileInfo) {
	unsafe {
		guard(|| {
			if !std::ptr::eq(this, source) {
				state(this).meta = read_meta(source)?;
			}
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn copy_info(this: *mut RawFileInfo, source: *const RawFileInfo) {
	unsafe {
		guard(|| {
			if !std::ptr::eq(this, source) {
				state(this).info = read_info(source)?;
			}
			Ok(())
		});
	}
}

/// Everything, as the SDK's `file_info::copy`: metadata, info, length and ReplayGain.
unsafe extern "C-unwind" fn copy(this: *mut RawFileInfo, source: *const RawFileInfo) {
	unsafe {
		guard(|| {
			if std::ptr::eq(this, source) {
				return Ok(());
			}
			let vtbl = &*(*source).vtbl;
			let meta = read_meta(source)?;
			let info = read_info(source)?;
			let length = try_host(|| (vtbl.get_length)(source))?;
			let mut replaygain = REPLAYGAIN_UNSET;
			try_host(|| (vtbl.get_replaygain)(source, &raw mut replaygain))?;
			let target = state(this);
			target.meta = meta;
			target.info = info;
			target.length = length;
			target.replaygain = replaygain;
			Ok(())
		});
	}
}

#[cfg(test)]
mod tests {
	use std::ffi::c_void;

	use super::FileInfoImpl;
	use crate::abi::file_info::ReplayGainInfo;

	const GAIN_INVALID: f32 = -1000.0;
	const PEAK_INVALID: f32 = -1.0;

	#[test]
	fn meta_add_keeps_values_in_order() {
		let mut info = FileInfoImpl::new();
		info.file_info().meta_add("ARTIST", "Keith Getty").unwrap();
		info.file_info().meta_add("artist", "Kristyn Getty").unwrap();
		assert_eq!(info.meta("Artist"), ["Keith Getty", "Kristyn Getty"]);
	}

	#[test]
	fn meta_set_replaces_and_is_case_insensitive() {
		let mut info = FileInfoImpl::new();
		info.file_info().meta_set("TITLE", "In Christ").unwrap();
		info.file_info().meta_set("title", "In Christ Alone").unwrap();
		assert_eq!(info.meta("TITLE"), ["In Christ Alone"]);
	}

	#[test]
	fn info_and_length_round_trip() {
		let mut info = FileInfoImpl::new();
		info.file_info().info_set("codec", "Vorbis").unwrap();
		info.file_info().set_length(279.0).unwrap();
		assert_eq!(info.info("CODEC").as_deref(), Some("Vorbis"));
		assert!((info.length() - 279.0).abs() < f64::EPSILON);
	}

	#[test]
	fn replaygain_round_trips_through_the_hidden_return_slot() {
		let mut info = FileInfoImpl::new();
		let value = ReplayGainInfo {
			album_gain: -8.51,
			track_gain: -6.62,
			album_peak: 1.0724,
			track_peak: 1.0127,
		};
		info.file_info().set_replaygain(value).unwrap();
		let raw = info.as_raw();
		let mut read =
			ReplayGainInfo { album_gain: 0.0, track_gain: 0.0, album_peak: 0.0, track_peak: 0.0 };
		let returned = unsafe { ((*(*raw).vtbl).get_replaygain)(raw, &raw mut read) };
		assert_eq!(returned, &raw mut read);
		assert_eq!(read, value);
	}

	#[test]
	fn fresh_info_has_invalid_replaygain() {
		let info = FileInfoImpl::new();
		let value = info.replaygain();
		assert_eq!((value.track_gain, value.album_gain), (GAIN_INVALID, GAIN_INVALID));
		assert_eq!((value.track_peak, value.album_peak), (PEAK_INVALID, PEAK_INVALID));
	}

	/// A `pfc::bit_array` whose `get` (its only slot we need) marks the indices in `marked`.
	#[repr(C)]
	struct BitArray {
		vtbl: *const BitArrayVtbl,
		marked: Vec<usize>,
	}

	#[repr(C)]
	struct BitArrayVtbl {
		get: unsafe extern "C-unwind" fn(this: *const BitArray, index: usize) -> bool,
	}

	unsafe extern "C-unwind" fn bit_get(this: *const BitArray, index: usize) -> bool {
		unsafe { &*this }.marked.contains(&index)
	}

	static BIT_ARRAY_VTBL: BitArrayVtbl = BitArrayVtbl { get: bit_get };

	fn mask(marked: Vec<usize>) -> BitArray {
		BitArray { vtbl: &raw const BIT_ARRAY_VTBL, marked }
	}

	#[test]
	fn remove_masks_drop_the_marked_fields_and_values() {
		let mut info = FileInfoImpl::new();
		for (name, value) in [("TITLE", "t"), ("ARTIST", "a"), ("ALBUM", "b")] {
			info.file_info().meta_set(name, value).unwrap();
		}
		info.file_info().meta_add("ARTIST", "a2").unwrap();
		let raw = info.as_raw();
		let vtbl = unsafe { &*(*raw).vtbl };
		let fields = mask(vec![0]);
		unsafe { (vtbl.meta_remove_mask)(raw, (&raw const fields).cast::<c_void>()) };
		assert!(info.meta("TITLE").is_empty());
		let values = mask(vec![0]);
		// ARTIST is now at index 0.
		unsafe { (vtbl.meta_remove_values)(raw, 0, (&raw const values).cast::<c_void>()) };
		assert_eq!(info.meta("ARTIST"), ["a2"]);
		assert_eq!(info.meta("ALBUM"), ["b"]);
	}

	unsafe extern "C-unwind" fn throwing_get(_this: *const BitArray, index: usize) -> bool {
		if index == 1 {
			unsafe { crate::cxx::throw(crate::cxx::ExceptionKind::BugCheck, "bad mask") }
		}
		index == 0
	}

	static THROWING_VTBL: BitArrayVtbl = BitArrayVtbl { get: throwing_get };

	#[test]
	fn a_mask_that_throws_removes_nothing() {
		let mut info = FileInfoImpl::new();
		for (name, value) in [("TITLE", "t"), ("ARTIST", "a"), ("ALBUM", "b")] {
			info.file_info().meta_set(name, value).unwrap();
		}
		let raw = info.as_raw();
		let vtbl = unsafe { &*(*raw).vtbl };
		let throwing = BitArray { vtbl: &raw const THROWING_VTBL, marked: Vec::new() };
		let result = crate::cxx::try_host(|| unsafe {
			(vtbl.meta_remove_mask)(raw, (&raw const throwing).cast::<c_void>());
		});
		assert!(result.is_err());
		assert_eq!(info.meta("TITLE"), ["t"]);
		assert_eq!(info.meta("ARTIST"), ["a"]);
		assert_eq!(info.meta("ALBUM"), ["b"]);
	}

	#[test]
	fn meta_reorder_follows_the_order() {
		let mut info = FileInfoImpl::new();
		for name in ["A", "B", "C"] {
			info.file_info().meta_set(name, name).unwrap();
		}
		let raw = info.as_raw();
		let order = [2usize, 0, 1];
		unsafe { ((*(*raw).vtbl).meta_reorder)(raw, order.as_ptr()) };
		let names: Vec<String> = (0..3)
			.map(|index| {
				let name = unsafe { ((*(*raw).vtbl).meta_enum_name)(raw, index) };
				unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy().into_owned()
			})
			.collect();
		assert_eq!(names, ["C", "A", "B"]);
	}

	#[test]
	fn copy_reads_another_file_info_through_its_vtable() {
		let mut source = FileInfoImpl::new();
		source.file_info().meta_add("ARTIST", "Keith Getty").unwrap();
		source.file_info().meta_add("ARTIST", "Kristyn Getty").unwrap();
		source.file_info().info_set("codec", "Vorbis").unwrap();
		source.file_info().set_length(279.0).unwrap();
		let mut target = FileInfoImpl::new();
		target.file_info().meta_set("TITLE", "old").unwrap();
		let raw = target.as_raw();
		unsafe { ((*(*raw).vtbl).copy)(raw, source.as_raw()) };
		assert!(target.meta("TITLE").is_empty());
		assert_eq!(target.meta("ARTIST"), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(target.info("codec").as_deref(), Some("Vorbis"));
		assert!((target.length() - 279.0).abs() < f64::EPSILON);
	}
}
