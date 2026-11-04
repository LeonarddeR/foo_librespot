pub mod Windows {
	pub mod Win32 {
		#[inline]
		pub unsafe fn CryptProtectData<P1>(
			pdatain: *const DATA_BLOB,
			szdatadescr: P1,
			poptionalentropy: Option<*const DATA_BLOB>,
			pvreserved: Option<*const core::ffi::c_void>,
			ppromptstruct: Option<*const CRYPTPROTECT_PROMPTSTRUCT>,
			dwflags: u32,
			pdataout: *mut DATA_BLOB,
		) -> windows_core::BOOL
		where
			P1: windows_core::Param<windows_core::PCWSTR>,
		{
			windows_core::link!("crypt32.dll" "system" fn CryptProtectData(pdatain : *const DATA_BLOB, szdatadescr : windows_core::PCWSTR, poptionalentropy : *const DATA_BLOB, pvreserved : *const core::ffi::c_void, ppromptstruct : *const CRYPTPROTECT_PROMPTSTRUCT, dwflags : u32, pdataout : *mut DATA_BLOB) -> windows_core::BOOL);
			unsafe {
				CryptProtectData(
					pdatain,
					szdatadescr.param().abi(),
					poptionalentropy.unwrap_or(core::mem::zeroed()) as _,
					pvreserved.unwrap_or(core::mem::zeroed()) as _,
					ppromptstruct.unwrap_or(core::mem::zeroed()) as _,
					dwflags,
					pdataout as _,
				)
			}
		}
		#[inline]
		pub unsafe fn CryptUnprotectData(
			pdatain: *const DATA_BLOB,
			ppszdatadescr: *mut windows_core::PWSTR,
			poptionalentropy: Option<*const DATA_BLOB>,
			pvreserved: Option<*const core::ffi::c_void>,
			ppromptstruct: Option<*const CRYPTPROTECT_PROMPTSTRUCT>,
			dwflags: u32,
			pdataout: *mut DATA_BLOB,
		) -> windows_core::BOOL {
			windows_core::link!("crypt32.dll" "system" fn CryptUnprotectData(pdatain : *const DATA_BLOB, ppszdatadescr : *mut windows_core::PWSTR, poptionalentropy : *const DATA_BLOB, pvreserved : *const core::ffi::c_void, ppromptstruct : *const CRYPTPROTECT_PROMPTSTRUCT, dwflags : u32, pdataout : *mut DATA_BLOB) -> windows_core::BOOL);
			unsafe {
				CryptUnprotectData(
					pdatain,
					ppszdatadescr as _,
					poptionalentropy.unwrap_or(core::mem::zeroed()) as _,
					pvreserved.unwrap_or(core::mem::zeroed()) as _,
					ppromptstruct.unwrap_or(core::mem::zeroed()) as _,
					dwflags,
					pdataout as _,
				)
			}
		}
		#[inline]
		pub unsafe fn LocalFree(hmem: HLOCAL) -> HLOCAL {
			windows_core::link!("kernel32.dll" "system" fn LocalFree(hmem : HLOCAL) -> HLOCAL);
			unsafe { LocalFree(hmem) }
		}
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct CRYPTPROTECT_PROMPTSTRUCT {
			pub cbSize: u32,
			pub dwPromptFlags: u32,
			pub hwndApp: HWND,
			pub szPrompt: windows_core::PCWSTR,
		}
		pub const CRYPTPROTECT_UI_FORBIDDEN: i32 = 1;
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct CRYPT_INTEGER_BLOB {
			pub cbData: u32,
			pub pbData: *mut u8,
		}
		pub type DATA_BLOB = CRYPT_INTEGER_BLOB;
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct HANDLE(pub *mut core::ffi::c_void);
		pub type HLOCAL = HANDLE;
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct HWND(pub *mut core::ffi::c_void);
	}
}
