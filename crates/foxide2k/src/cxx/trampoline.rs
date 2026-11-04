//! The assembly half of `try_host`: a function with its own x64 exception handler.
//!
//! Rust cannot express "catch a foreign C++ exception": `catch_unwind` aborts on one. Windows x64
//! exception handling is table-driven, so a function written in assembly can name its own
//! language-specific handler in its unwind info, exactly as MSVC does for `__try`/`__except`.

use std::ffi::c_void;

use crate::cxx::catch::foxide2k_cxx_handler;

// foxide2k_cxx_try(data, call, slot) calls `call(data)` and returns 0. If foxide2k_cxx_handler catches
// a C++ exception, it unwinds to foxide2k_cxx_try_landing, which returns 1.
//
// Frame: `sub rsp, 40` keeps rsp 16-byte aligned at the call and reserves the 32-byte home area
// plus one slot at [rsp + 32] for `slot`. With no frame pointer, the establisher frame the
// handler receives is rsp after the prologue, so the handler finds `slot` at frame + 32.
std::arch::global_asm!(
	".text",
	".p2align 4",
	".globl foxide2k_cxx_try",
	".def foxide2k_cxx_try",
	".scl 2",
	".type 32",
	".endef",
	"foxide2k_cxx_try:",
	".seh_proc foxide2k_cxx_try",
	".seh_handler {handler}, @except",
	"sub rsp, 40",
	".seh_stackalloc 40",
	".seh_endprologue",
	"mov qword ptr [rsp + 32], r8",
	"call rdx",
	"xor eax, eax",
	"2:",
	"add rsp, 40",
	"ret",
	".globl foxide2k_cxx_try_landing",
	"foxide2k_cxx_try_landing:",
	"mov eax, 1",
	"jmp 2b",
	".seh_endproc",
	handler = sym foxide2k_cxx_handler,
);

/// Where the handler finds the `slot` argument, relative to the establisher frame.
pub(crate) const SLOT_OFFSET: usize = 32;

unsafe extern "C-unwind" {
	pub(crate) fn foxide2k_cxx_try(
		data: *mut c_void,
		call: unsafe extern "C-unwind" fn(*mut c_void),
		slot: *mut c_void,
	) -> u32;
}

unsafe extern "C" {
	static foxide2k_cxx_try_landing: u8;
}

/// The address `RtlUnwindEx` resumes at after a catch.
pub(crate) fn landing() -> *const c_void {
	(&raw const foxide2k_cxx_try_landing).cast()
}
