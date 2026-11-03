//! MSVC x64 C++ exception metadata: ThrowInfo, catchable types and type descriptors.
//!
//! On x64 these structures point at each other with 32-bit offsets from the start of the module
//! (`__ImageBase`), so they must live in this module's image and the offsets can only be filled
//! in at runtime. Rust's own panic runtime does the same (`library/panic_unwind/src/seh.rs`).

use std::ffi::c_void;
use std::sync::Once;
use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use crate::cxx::kind::{ExceptionKind, MAX_CHAIN, TYPES};
use crate::cxx::throw::{StdException, copy_function_address, destructor_address};

const NAME_CAPACITY: usize = 64;

/// MSVC's `TypeDescriptor`: the `type_info` object behind `typeid`. C++ catch clauses match
/// thrown types by comparing these names.
#[repr(C)]
pub(crate) struct TypeDescriptor {
	vftable: AtomicPtr<c_void>,
	spare: usize,
	name: [u8; NAME_CAPACITY],
}

// The accessors are read by try_host when it inspects a caught exception.
#[cfg_attr(not(test), allow(dead_code))]
impl TypeDescriptor {
	const fn named(name: &str) -> Self {
		let bytes = name.as_bytes();
		assert!(bytes.len() < NAME_CAPACITY);
		let mut buffer = [0; NAME_CAPACITY];
		let mut index = 0;
		while index < bytes.len() {
			buffer[index] = bytes[index];
			index += 1;
		}
		Self { vftable: AtomicPtr::new(std::ptr::null_mut()), spare: 0, name: buffer }
	}

	pub fn name(&self) -> &str {
		let len = self.name.iter().position(|&byte| byte == 0).unwrap_or(NAME_CAPACITY);
		std::str::from_utf8(&self.name[..len]).unwrap_or("")
	}
}

/// MSVC's `PMD`: where a base class sits inside the thrown object.
#[repr(C)]
pub(crate) struct Pmd {
	pub mdisp: i32,
	pub pdisp: i32,
	pub vdisp: i32,
}

/// MSVC's `CatchableType`: one class the thrown object can be caught as.
#[repr(C)]
pub(crate) struct CatchableType {
	properties: u32,
	type_descriptor: AtomicI32,
	pub this_displacement: Pmd,
	size_or_offset: i32,
	copy_function: AtomicI32,
}

#[cfg_attr(not(test), allow(dead_code))]
impl CatchableType {
	const fn new() -> Self {
		Self {
			properties: 0,
			type_descriptor: AtomicI32::new(0),
			// Single inheritance without virtual bases: every base starts at offset 0.
			this_displacement: Pmd { mdisp: 0, pdisp: -1, vdisp: 0 },
			#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
			size_or_offset: size_of::<StdException>() as i32,
			copy_function: AtomicI32::new(0),
		}
	}

	pub fn type_descriptor(&self) -> i32 {
		self.type_descriptor.load(Ordering::Relaxed)
	}

	pub fn copy_function(&self) -> i32 {
		self.copy_function.load(Ordering::Relaxed)
	}
}

/// MSVC's `CatchableTypeArray`: the thrown class and all its bases, most-derived first.
#[repr(C)]
struct CatchableTypeArray {
	count: AtomicI32,
	types: [AtomicI32; MAX_CHAIN],
}

/// MSVC's `ThrowInfo`, the second argument of `_CxxThrowException`.
#[repr(C)]
pub(crate) struct ThrowInfo {
	attributes: u32,
	unwind: AtomicI32,
	forward_compat: i32,
	catchable_type_array: AtomicI32,
}

#[cfg_attr(not(test), allow(dead_code))]
impl ThrowInfo {
	const fn new() -> Self {
		Self {
			attributes: 0,
			unwind: AtomicI32::new(0),
			forward_compat: 0,
			catchable_type_array: AtomicI32::new(0),
		}
	}

	pub fn unwind(&self) -> i32 {
		self.unwind.load(Ordering::Relaxed)
	}

	pub fn catchable_type_array(&self) -> i32 {
		self.catchable_type_array.load(Ordering::Relaxed)
	}
}

unsafe extern "C" {
	// The vtable of vcruntime's `type_info`; the \x01 stops Rust from mangling the name.
	#[link_name = "\x01??_7type_info@@6B@"]
	static TYPE_INFO_VTABLE: *const u8;
	static __ImageBase: u8;
}

static DESCRIPTORS: [TypeDescriptor; TYPES.len()] = {
	let mut descriptors = [const { TypeDescriptor::named("") }; TYPES.len()];
	let mut index = 0;
	while index < TYPES.len() {
		descriptors[index] = TypeDescriptor::named(TYPES[index].name);
		index += 1;
	}
	descriptors
};
static CATCHABLE_TYPES: [CatchableType; TYPES.len()] =
	[const { CatchableType::new() }; TYPES.len()];
static CATCHABLE_TYPE_ARRAYS: [CatchableTypeArray; TYPES.len()] = [const {
	CatchableTypeArray { count: AtomicI32::new(0), types: [const { AtomicI32::new(0) }; MAX_CHAIN] }
}; TYPES.len()];
static THROW_INFOS: [ThrowInfo; TYPES.len()] = [const { ThrowInfo::new() }; TYPES.len()];
static INIT: Once = Once::new();

/// The address this module was loaded at.
pub(crate) fn image_base() -> usize {
	(&raw const __ImageBase) as usize
}

fn image_offset(address: usize) -> i32 {
	i32::try_from(address - image_base()).expect("exception metadata lies inside this module")
}

fn address_of<T>(item: &'static T) -> usize {
	std::ptr::from_ref(item) as usize
}

fn init() {
	INIT.call_once(|| {
		let type_info_vtable = (&raw const TYPE_INFO_VTABLE).cast_mut().cast::<c_void>();
		for (descriptor, catchable) in DESCRIPTORS.iter().zip(&CATCHABLE_TYPES) {
			descriptor.vftable.store(type_info_vtable, Ordering::Relaxed);
			catchable
				.type_descriptor
				.store(image_offset(address_of(descriptor)), Ordering::Relaxed);
			catchable.copy_function.store(image_offset(copy_function_address()), Ordering::Relaxed);
		}
		for kind in ExceptionKind::ALL {
			let array = &CATCHABLE_TYPE_ARRAYS[kind as usize];
			let mut count = 0;
			let mut current = Some(kind);
			while let Some(class) = current {
				array.types[count].store(
					image_offset(address_of(&CATCHABLE_TYPES[class as usize])),
					Ordering::Relaxed,
				);
				count += 1;
				current = TYPES[class as usize].base;
			}
			array.count.store(i32::try_from(count).expect("short chain"), Ordering::Relaxed);
			let info = &THROW_INFOS[kind as usize];
			info.unwind.store(image_offset(destructor_address()), Ordering::Relaxed);
			info.catchable_type_array.store(image_offset(address_of(array)), Ordering::Relaxed);
		}
	});
}

/// The ThrowInfo for throwing `kind`.
pub(crate) fn throw_info(kind: ExceptionKind) -> &'static ThrowInfo {
	init();
	&THROW_INFOS[kind as usize]
}

#[cfg(test)]
mod tests {
	use super::{CatchableType, Pmd, ThrowInfo, TypeDescriptor, image_base, throw_info};
	use crate::cxx::kind::ExceptionKind;
	use crate::cxx::throw::{StdException, copy_function_address, destructor_address};

	unsafe fn at<T>(base: usize, rva: i32) -> &'static T {
		unsafe { &*((base + usize::try_from(rva).unwrap()) as *const T) }
	}

	#[test]
	fn throw_info_offsets_resolve_to_statics() {
		let base = image_base();
		for kind in ExceptionKind::ALL {
			let info = throw_info(kind);
			let names = kind.type_names();
			unsafe {
				assert_eq!(base + usize::try_from(info.unwind()).unwrap(), destructor_address());
				let array_base = base + usize::try_from(info.catchable_type_array()).unwrap();
				let count = *(array_base as *const i32);
				assert_eq!(usize::try_from(count).unwrap(), names.len(), "{kind:?}");
				for (index, name) in names.iter().enumerate() {
					let entry = *((array_base + 4 + index * 4) as *const i32);
					let catchable: &CatchableType = at(base, entry);
					assert_eq!(
						base + usize::try_from(catchable.copy_function()).unwrap(),
						copy_function_address()
					);
					let descriptor: &TypeDescriptor = at(base, catchable.type_descriptor());
					assert_eq!(descriptor.name(), *name, "{kind:?} entry {index}");
				}
			}
		}
	}

	#[test]
	fn layouts_match_msvc() {
		assert_eq!(size_of::<StdException>(), 24);
		assert_eq!(size_of::<CatchableType>(), 28);
		assert_eq!(size_of::<ThrowInfo>(), 16);
		assert_eq!(size_of::<Pmd>(), 12);
	}
}
