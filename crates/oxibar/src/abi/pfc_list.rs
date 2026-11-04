// The list interface in this file is transcribed from pfc, which ships with the foobar2000 2.x
// SDK. Altered from the original: its C++ class declarations are rewritten as Rust vtable structs.
//
// Copyright (C) Peter Pawlowski
//
// This software is provided 'as-is', without any express or implied warranty. In no event will
// the authors be held liable for any damages arising from the use of this software.
//
// Permission is granted to anyone to use this software for any purpose, including commercial
// applications, and to alter it and redistribute it freely, subject to the following
// restrictions:
//
// 1. The origin of this software must not be misrepresented; you must not claim that you wrote
//    the original software. If you use this software in a product, an acknowledgment in the
//    product documentation is appreciated but not required.
// 2. Altered source versions must be plainly marked as such, and must not be misrepresented as
//    being the original software.
// 3. This notice may not be removed or altered from any source distribution.

//! `pfc::list_base_const_t` and `pfc::list_base_t` (`pfc\list.h:59-285`): the list interfaces
//! foobar2000 passes for a component to read or fill. Neither has a virtual destructor.

use std::ffi::c_void;

/// A `pfc::list_base_const_t<T>`.
#[repr(C)]
pub struct ListBaseConst<T> {
	pub vtbl: *const ListBaseConstVtbl<T>,
}

#[repr(C)]
pub struct ListBaseConstVtbl<T> {
	pub get_count: unsafe extern "C-unwind" fn(this: *const ListBaseConst<T>) -> usize,
	/// Assigns item `n` to `out`, as `T`'s copy assignment does.
	pub get_item_ex:
		unsafe extern "C-unwind" fn(this: *const ListBaseConst<T>, out: *mut T, n: usize),
}

/// A `pfc::list_base_t<T>`.
#[repr(C)]
pub struct ListBase<T> {
	pub vtbl: *const ListBaseVtbl<T>,
}

/// `list_base_const_t`'s two slots, then `list_base_t`'s, in declaration order. `bit_array` and
/// `sort_callback` are left opaque.
#[repr(C)]
pub struct ListBaseVtbl<T> {
	pub get_count: unsafe extern "C-unwind" fn(this: *const ListBase<T>) -> usize,
	pub get_item_ex: unsafe extern "C-unwind" fn(this: *const ListBase<T>, out: *mut T, n: usize),
	pub filter_mask: unsafe extern "C-unwind" fn(this: *mut ListBase<T>, mask: *const c_void),
	/// Inserts copies of every item of `items` at `base`, or at the end for `usize::MAX`.
	/// Returns the index of the first one.
	pub insert_items: unsafe extern "C-unwind" fn(
		this: *mut ListBase<T>,
		items: *const ListBaseConst<T>,
		base: usize,
	) -> usize,
	pub reorder_partial: unsafe extern "C-unwind" fn(
		this: *mut ListBase<T>,
		base: usize,
		order: *const usize,
		count: usize,
	),
	pub sort: unsafe extern "C-unwind" fn(this: *mut ListBase<T>, callback: *mut c_void),
	pub sort_stable: unsafe extern "C-unwind" fn(this: *mut ListBase<T>, callback: *mut c_void),
	pub replace_item:
		unsafe extern "C-unwind" fn(this: *mut ListBase<T>, index: usize, item: *const T),
	pub swap_item_with:
		unsafe extern "C-unwind" fn(this: *mut ListBase<T>, index: usize, item: *mut T),
	pub swap_items:
		unsafe extern "C-unwind" fn(this: *mut ListBase<T>, first: usize, second: usize),
}

#[cfg(test)]
mod tests {
	use std::mem::offset_of;

	use super::{ListBaseConstVtbl, ListBaseVtbl};

	#[test]
	fn list_vtbls_have_the_pfc_slot_counts() {
		assert_eq!(size_of::<ListBaseConstVtbl<u64>>(), 2 * 8);
		assert_eq!(size_of::<ListBaseVtbl<u64>>(), 10 * 8);
		assert_eq!(offset_of!(ListBaseVtbl<u64>, insert_items), 3 * 8);
		assert_eq!(offset_of!(ListBaseVtbl<u64>, swap_items), 9 * 8);
	}
}
