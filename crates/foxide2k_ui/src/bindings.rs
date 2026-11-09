pub mod Windows {
	pub mod Win32 {
		#[inline]
		pub unsafe fn CoCreateInstance<P1, T>(
			rclsid: *const windows_core::GUID,
			punkouter: P1,
			dwclscontext: u32,
		) -> windows_core::Result<T>
		where
			P1: windows_core::Param<windows_core::IUnknown>,
			T: windows_core::Interface,
		{
			windows_core::link!("ole32.dll" "system" fn CoCreateInstance(rclsid : *const windows_core::GUID, punkouter : *mut core::ffi::c_void, dwclscontext : u32, riid : *const windows_core::GUID, ppv : *mut *mut core::ffi::c_void) -> windows_core::HRESULT);
			let mut result__ = core::ptr::null_mut();
			unsafe {
				CoCreateInstance(
					rclsid,
					punkouter.param().abi(),
					dwclscontext,
					&T::IID,
					&mut result__,
				)
				.and_then(|| windows_core::imp::Type::from_abi(result__))
			}
		}
		#[inline]
		pub unsafe fn CoInitializeEx(
			pvreserved: Option<*const core::ffi::c_void>,
			dwcoinit: u32,
		) -> windows_core::HRESULT {
			windows_core::link!("ole32.dll" "system" fn CoInitializeEx(pvreserved : *const core::ffi::c_void, dwcoinit : u32) -> windows_core::HRESULT);
			unsafe { CoInitializeEx(pvreserved.unwrap_or(core::mem::zeroed()) as _, dwcoinit) }
		}
		#[inline]
		pub unsafe fn CreateDialogIndirectParamW(
			hinstance: Option<HINSTANCE>,
			lptemplate: *const DLGTEMPLATE,
			hwndparent: Option<HWND>,
			lpdialogfunc: DLGPROC,
			dwinitparam: LPARAM,
		) -> HWND {
			windows_core::link!("user32.dll" "system" fn CreateDialogIndirectParamW(hinstance : HINSTANCE, lptemplate : *const DLGTEMPLATE, hwndparent : HWND, lpdialogfunc : DLGPROC, dwinitparam : LPARAM) -> HWND);
			unsafe {
				CreateDialogIndirectParamW(
					hinstance.unwrap_or(core::mem::zeroed()) as _,
					lptemplate,
					hwndparent.unwrap_or(core::mem::zeroed()) as _,
					lpdialogfunc,
					dwinitparam,
				)
			}
		}
		#[inline]
		pub unsafe fn DestroyWindow(hwnd: HWND) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn DestroyWindow(hwnd : HWND) -> windows_core::BOOL);
			unsafe { DestroyWindow(hwnd) }
		}
		#[inline]
		pub unsafe fn DispatchMessageW(lpmsg: *const MSG) -> LRESULT {
			windows_core::link!("user32.dll" "system" fn DispatchMessageW(lpmsg : *const MSG) -> LRESULT);
			unsafe { DispatchMessageW(lpmsg) }
		}
		#[inline]
		pub unsafe fn EnableWindow(hwnd: HWND, benable: bool) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn EnableWindow(hwnd : HWND, benable : windows_core::BOOL) -> windows_core::BOOL);
			unsafe { EnableWindow(hwnd, benable.into()) }
		}
		#[inline]
		pub unsafe fn GetClientRect(hwnd: HWND, lprect: *mut RECT) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn GetClientRect(hwnd : HWND, lprect : *mut RECT) -> windows_core::BOOL);
			unsafe { GetClientRect(hwnd, lprect as _) }
		}
		#[inline]
		pub unsafe fn GetDlgCtrlID(hwnd: HWND) -> i32 {
			windows_core::link!("user32.dll" "system" fn GetDlgCtrlID(hwnd : HWND) -> i32);
			unsafe { GetDlgCtrlID(hwnd) }
		}
		#[inline]
		pub unsafe fn GetDlgItem(hdlg: Option<HWND>, niddlgitem: i32) -> HWND {
			windows_core::link!("user32.dll" "system" fn GetDlgItem(hdlg : HWND, niddlgitem : i32) -> HWND);
			unsafe { GetDlgItem(hdlg.unwrap_or(core::mem::zeroed()) as _, niddlgitem) }
		}
		#[inline]
		pub unsafe fn GetFocus() -> HWND {
			windows_core::link!("user32.dll" "system" fn GetFocus() -> HWND);
			unsafe { GetFocus() }
		}
		#[inline]
		pub unsafe fn GetNextDlgTabItem(hdlg: HWND, hctl: Option<HWND>, bprevious: bool) -> HWND {
			windows_core::link!("user32.dll" "system" fn GetNextDlgTabItem(hdlg : HWND, hctl : HWND, bprevious : windows_core::BOOL) -> HWND);
			unsafe {
				GetNextDlgTabItem(hdlg, hctl.unwrap_or(core::mem::zeroed()) as _, bprevious.into())
			}
		}
		#[inline]
		pub unsafe fn GetSystemMetrics(nindex: i32) -> i32 {
			windows_core::link!("user32.dll" "system" fn GetSystemMetrics(nindex : i32) -> i32);
			unsafe { GetSystemMetrics(nindex) }
		}
		#[cfg(any(target_arch = "aarch64", target_arch = "arm64ec", target_arch = "x86_64"))]
		#[inline]
		pub unsafe fn GetWindowLongPtrW(hwnd: HWND, nindex: i32) -> isize {
			windows_core::link!("user32.dll" "system" fn GetWindowLongPtrW(hwnd : HWND, nindex : i32) -> isize);
			unsafe { GetWindowLongPtrW(hwnd, nindex) }
		}
		#[cfg(target_pointer_width = "32")]
		pub use GetWindowLongW as GetWindowLongPtrW;
		#[inline]
		pub unsafe fn GetWindowLongW(hwnd: HWND, nindex: i32) -> i32 {
			windows_core::link!("user32.dll" "system" fn GetWindowLongW(hwnd : HWND, nindex : i32) -> i32);
			unsafe { GetWindowLongW(hwnd, nindex) }
		}
		#[inline]
		pub unsafe fn GetWindowTextLengthW(hwnd: HWND) -> i32 {
			windows_core::link!("user32.dll" "system" fn GetWindowTextLengthW(hwnd : HWND) -> i32);
			unsafe { GetWindowTextLengthW(hwnd) }
		}
		#[inline]
		pub unsafe fn GetWindowTextW(
			hwnd: HWND,
			lpstring: windows_core::PWSTR,
			nmaxcount: i32,
		) -> i32 {
			windows_core::link!("user32.dll" "system" fn GetWindowTextW(hwnd : HWND, lpstring : windows_core::PWSTR, nmaxcount : i32) -> i32);
			unsafe { GetWindowTextW(hwnd, lpstring, nmaxcount) }
		}
		#[inline]
		pub unsafe fn InitCommonControlsEx(
			picce: *const INITCOMMONCONTROLSEX,
		) -> windows_core::BOOL {
			windows_core::link!("comctl32.dll" "system" fn InitCommonControlsEx(picce : *const INITCOMMONCONTROLSEX) -> windows_core::BOOL);
			unsafe { InitCommonControlsEx(picce) }
		}
		#[inline]
		pub unsafe fn IsChild(hwndparent: HWND, hwnd: HWND) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn IsChild(hwndparent : HWND, hwnd : HWND) -> windows_core::BOOL);
			unsafe { IsChild(hwndparent, hwnd) }
		}
		#[inline]
		pub unsafe fn IsWindow(hwnd: Option<HWND>) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn IsWindow(hwnd : HWND) -> windows_core::BOOL);
			unsafe { IsWindow(hwnd.unwrap_or(core::mem::zeroed()) as _) }
		}
		#[inline]
		pub unsafe fn IsWindowEnabled(hwnd: HWND) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn IsWindowEnabled(hwnd : HWND) -> windows_core::BOOL);
			unsafe { IsWindowEnabled(hwnd) }
		}
		#[inline]
		pub unsafe fn NotifyWinEvent(event: u32, hwnd: HWND, idobject: i32, idchild: i32) {
			windows_core::link!("user32.dll" "system" fn NotifyWinEvent(event : u32, hwnd : HWND, idobject : i32, idchild : i32));
			unsafe { NotifyWinEvent(event, hwnd, idobject, idchild) }
		}
		#[inline]
		pub unsafe fn PeekMessageW(
			lpmsg: *mut MSG,
			hwnd: Option<HWND>,
			wmsgfiltermin: u32,
			wmsgfiltermax: u32,
			wremovemsg: u32,
		) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn PeekMessageW(lpmsg : *mut MSG, hwnd : HWND, wmsgfiltermin : u32, wmsgfiltermax : u32, wremovemsg : u32) -> windows_core::BOOL);
			unsafe {
				PeekMessageW(
					lpmsg as _,
					hwnd.unwrap_or(core::mem::zeroed()) as _,
					wmsgfiltermin,
					wmsgfiltermax,
					wremovemsg,
				)
			}
		}
		#[inline]
		pub unsafe fn PostMessageW(
			hwnd: Option<HWND>,
			msg: u32,
			wparam: WPARAM,
			lparam: LPARAM,
		) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn PostMessageW(hwnd : HWND, msg : u32, wparam : WPARAM, lparam : LPARAM) -> windows_core::BOOL);
			unsafe { PostMessageW(hwnd.unwrap_or(core::mem::zeroed()) as _, msg, wparam, lparam) }
		}
		#[inline]
		pub unsafe fn SendMessageW(
			hwnd: HWND,
			msg: u32,
			wparam: WPARAM,
			lparam: LPARAM,
		) -> LRESULT {
			windows_core::link!("user32.dll" "system" fn SendMessageW(hwnd : HWND, msg : u32, wparam : WPARAM, lparam : LPARAM) -> LRESULT);
			unsafe { SendMessageW(hwnd, msg, wparam, lparam) }
		}
		#[inline]
		pub unsafe fn SetForegroundWindow(hwnd: HWND) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn SetForegroundWindow(hwnd : HWND) -> windows_core::BOOL);
			unsafe { SetForegroundWindow(hwnd) }
		}
		#[cfg(any(target_arch = "aarch64", target_arch = "arm64ec", target_arch = "x86_64"))]
		#[inline]
		pub unsafe fn SetWindowLongPtrW(hwnd: HWND, nindex: i32, dwnewlong: isize) -> isize {
			windows_core::link!("user32.dll" "system" fn SetWindowLongPtrW(hwnd : HWND, nindex : i32, dwnewlong : isize) -> isize);
			unsafe { SetWindowLongPtrW(hwnd, nindex, dwnewlong) }
		}
		#[cfg(target_pointer_width = "32")]
		pub use SetWindowLongW as SetWindowLongPtrW;
		#[inline]
		pub unsafe fn SetWindowLongW(hwnd: HWND, nindex: i32, dwnewlong: i32) -> i32 {
			windows_core::link!("user32.dll" "system" fn SetWindowLongW(hwnd : HWND, nindex : i32, dwnewlong : i32) -> i32);
			unsafe { SetWindowLongW(hwnd, nindex, dwnewlong) }
		}
		#[inline]
		pub unsafe fn SetWindowTextW<P1>(hwnd: HWND, lpstring: P1) -> windows_core::BOOL
		where
			P1: windows_core::Param<windows_core::PCWSTR>,
		{
			windows_core::link!("user32.dll" "system" fn SetWindowTextW(hwnd : HWND, lpstring : windows_core::PCWSTR) -> windows_core::BOOL);
			unsafe { SetWindowTextW(hwnd, lpstring.param().abi()) }
		}
		#[inline]
		pub unsafe fn ShowWindow(hwnd: HWND, ncmdshow: i32) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn ShowWindow(hwnd : HWND, ncmdshow : i32) -> windows_core::BOOL);
			unsafe { ShowWindow(hwnd, ncmdshow) }
		}
		#[inline]
		pub unsafe fn TranslateMessage(lpmsg: *const MSG) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn TranslateMessage(lpmsg : *const MSG) -> windows_core::BOOL);
			unsafe { TranslateMessage(lpmsg) }
		}
		pub type AnnoScope = i32;
		pub const Assertive: LiveSetting = 2;
		pub const BM_GETCHECK: i32 = 240;
		pub const BM_SETCHECK: i32 = 241;
		pub const BN_CLICKED: i32 = 0;
		pub const BN_SETFOCUS: i32 = 6;
		pub const BST_CHECKED: i32 = 1;
		pub const BST_UNCHECKED: i32 = 0;
		pub const BS_AUTOCHECKBOX: i32 = 3;
		pub const BS_DEFPUSHBUTTON: i32 = 1;
		pub const BS_GROUPBOX: i32 = 7;
		pub const BS_NOTIFY: i32 = 16384;
		pub const BS_PUSHBUTTON: i32 = 0;
		pub const CBN_SELCHANGE: i32 = 1;
		pub const CBS_AUTOHSCROLL: i32 = 64;
		pub const CBS_DROPDOWN: i32 = 2;
		pub const CBS_DROPDOWNLIST: i32 = 3;
		pub const CB_ADDSTRING: i32 = 323;
		pub const CB_GETCOUNT: i32 = 326;
		pub const CB_GETCURSEL: i32 = 327;
		pub const CB_GETEDITSEL: i32 = 320;
		pub const CB_GETLBTEXT: i32 = 328;
		pub const CB_GETLBTEXTLEN: i32 = 329;
		pub const CB_RESETCONTENT: i32 = 331;
		pub const CB_SETCURSEL: i32 = 334;
		pub const CB_SETEDITSEL: i32 = 322;
		pub const CHILDID_SELF: i32 = 0;
		pub type CLSCTX = u32;
		pub const CLSCTX_INPROC_SERVER: CLSCTX = 1;
		pub const CLSID_AccPropServices: windows_core::GUID =
			windows_core::GUID::from_u128(0xb5f8350b_0548_48b1_a6ee_88bd00b4a5e7);
		pub type COINIT = i32;
		pub const COINIT_APARTMENTTHREADED: COINIT = 2;
		#[repr(C)]
		#[derive(Clone, Copy)]
		pub union CY {
			pub Anonymous: CY_0,
			pub int64: i64,
		}
		impl Default for CY {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct CY_0 {
			pub Lo: u32,
			pub Hi: i32,
		}
		#[repr(C)]
		#[derive(Clone, Copy)]
		pub struct DECIMAL {
			pub wReserved: u16,
			pub Anonymous: DECIMAL_0,
			pub Hi32: u32,
			pub Anonymous2: DECIMAL_1,
		}
		impl Default for DECIMAL {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[derive(Clone, Copy)]
		pub union DECIMAL_0 {
			pub Anonymous: DECIMAL_0_0,
			pub signscale: u16,
		}
		impl Default for DECIMAL_0 {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct DECIMAL_0_0 {
			pub scale: u8,
			pub sign: u8,
		}
		#[repr(C)]
		#[derive(Clone, Copy)]
		pub union DECIMAL_1 {
			pub Anonymous: DECIMAL_1_0,
			pub Lo64: u64,
		}
		impl Default for DECIMAL_1 {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct DECIMAL_1_0 {
			pub Lo32: u32,
			pub Mid32: u32,
		}
		pub const DLGC_WANTALLKEYS: i32 = 4;
		pub type DLGPROC = Option<
			unsafe extern "system" fn(
				param0: HWND,
				param1: u32,
				param2: WPARAM,
				param3: LPARAM,
			) -> isize,
		>;
		#[repr(C, packed(2))]
		#[derive(Clone, Copy, Default)]
		pub struct DLGTEMPLATE {
			pub style: u32,
			pub dwExtendedStyle: u32,
			pub cdit: u16,
			pub x: i16,
			pub y: i16,
			pub cx: i16,
			pub cy: i16,
		}
		pub const DS_CENTER: i32 = 2048;
		pub const DS_CONTROL: i32 = 1024;
		pub const DS_FIXEDSYS: i32 = 8;
		pub const DS_MODALFRAME: i32 = 128;
		pub const DS_SETFONT: i32 = 64;
		pub const DWLP_MSGRESULT: i32 = 0;
		pub const EN_SETFOCUS: i32 = 256;
		pub const ES_AUTOHSCROLL: i32 = 128;
		pub const ES_AUTOVSCROLL: i32 = 64;
		pub const ES_MULTILINE: i32 = 4;
		pub const ES_READONLY: i32 = 2048;
		pub const EVENT_OBJECT_LIVEREGIONCHANGED: i32 = 32793;
		pub const GWL_STYLE: i32 = -16;
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct HINSTANCE(pub *mut core::ffi::c_void);
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct HMENU(pub *mut core::ffi::c_void);
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct HWND(pub *mut core::ffi::c_void);
		windows_core::imp::define_interface!(
			IAccPropServer,
			IAccPropServer_Vtbl,
			0x76c0dbbb_15e0_4e7b_b61b_20eeea2001e0
		);
		windows_core::imp::interface_hierarchy!(IAccPropServer, windows_core::IUnknown);
		#[repr(C)]
		pub struct IAccPropServer_Vtbl {
			pub base__: windows_core::IUnknown_Vtbl,
			GetPropValue: usize,
		}
		impl windows_core::RuntimeName for IAccPropServer {}
		windows_core::imp::define_interface!(
			IAccPropServices,
			IAccPropServices_Vtbl,
			0x6e26e776_04f0_495d_80e4_3330352e3169
		);
		windows_core::imp::interface_hierarchy!(IAccPropServices, windows_core::IUnknown);
		impl IAccPropServices {
			pub unsafe fn SetPropValue(
				&self,
				pidstring: *const u8,
				dwidstringlen: u32,
				idprop: MSAAPROPID,
				var: &VARIANT,
			) -> windows_core::HRESULT {
				unsafe {
					(windows_core::Interface::vtable(self).SetPropValue)(
						windows_core::Interface::as_raw(self),
						pidstring,
						dwidstringlen,
						idprop,
						core::mem::transmute_copy(var),
					)
				}
			}
			pub unsafe fn SetPropServer<P4>(
				&self,
				pidstring: *const u8,
				dwidstringlen: u32,
				paprops: *const MSAAPROPID,
				cprops: i32,
				pserver: P4,
				annoscope: AnnoScope,
			) -> windows_core::HRESULT
			where
				P4: windows_core::Param<IAccPropServer>,
			{
				unsafe {
					(windows_core::Interface::vtable(self).SetPropServer)(
						windows_core::Interface::as_raw(self),
						pidstring,
						dwidstringlen,
						paprops,
						cprops,
						pserver.param().abi(),
						annoscope,
					)
				}
			}
			pub unsafe fn ClearProps(
				&self,
				pidstring: *const u8,
				dwidstringlen: u32,
				paprops: *const MSAAPROPID,
				cprops: i32,
			) -> windows_core::HRESULT {
				unsafe {
					(windows_core::Interface::vtable(self).ClearProps)(
						windows_core::Interface::as_raw(self),
						pidstring,
						dwidstringlen,
						paprops,
						cprops,
					)
				}
			}
			pub unsafe fn SetHwndProp(
				&self,
				hwnd: HWND,
				idobject: u32,
				idchild: u32,
				idprop: MSAAPROPID,
				var: &VARIANT,
			) -> windows_core::HRESULT {
				unsafe {
					(windows_core::Interface::vtable(self).SetHwndProp)(
						windows_core::Interface::as_raw(self),
						hwnd,
						idobject,
						idchild,
						idprop,
						core::mem::transmute_copy(var),
					)
				}
			}
			pub unsafe fn SetHwndPropStr<P4>(
				&self,
				hwnd: HWND,
				idobject: u32,
				idchild: u32,
				idprop: MSAAPROPID,
				str: P4,
			) -> windows_core::HRESULT
			where
				P4: windows_core::Param<windows_core::PCWSTR>,
			{
				unsafe {
					(windows_core::Interface::vtable(self).SetHwndPropStr)(
						windows_core::Interface::as_raw(self),
						hwnd,
						idobject,
						idchild,
						idprop,
						str.param().abi(),
					)
				}
			}
			pub unsafe fn SetHwndPropServer<P5>(
				&self,
				hwnd: HWND,
				idobject: u32,
				idchild: u32,
				paprops: *const MSAAPROPID,
				cprops: i32,
				pserver: P5,
				annoscope: AnnoScope,
			) -> windows_core::HRESULT
			where
				P5: windows_core::Param<IAccPropServer>,
			{
				unsafe {
					(windows_core::Interface::vtable(self).SetHwndPropServer)(
						windows_core::Interface::as_raw(self),
						hwnd,
						idobject,
						idchild,
						paprops,
						cprops,
						pserver.param().abi(),
						annoscope,
					)
				}
			}
			pub unsafe fn ClearHwndProps(
				&self,
				hwnd: HWND,
				idobject: u32,
				idchild: u32,
				paprops: *const MSAAPROPID,
				cprops: i32,
			) -> windows_core::HRESULT {
				unsafe {
					(windows_core::Interface::vtable(self).ClearHwndProps)(
						windows_core::Interface::as_raw(self),
						hwnd,
						idobject,
						idchild,
						paprops,
						cprops,
					)
				}
			}
			pub unsafe fn ComposeHwndIdentityString(
				&self,
				hwnd: HWND,
				idobject: u32,
				idchild: u32,
				ppidstring: *mut *mut u8,
				pdwidstringlen: *mut u32,
			) -> windows_core::HRESULT {
				unsafe {
					(windows_core::Interface::vtable(self).ComposeHwndIdentityString)(
						windows_core::Interface::as_raw(self),
						hwnd,
						idobject,
						idchild,
						ppidstring as _,
						pdwidstringlen as _,
					)
				}
			}
			pub unsafe fn DecomposeHwndIdentityString(
				&self,
				pidstring: *const u8,
				dwidstringlen: u32,
				phwnd: *mut HWND,
				pidobject: *mut u32,
				pidchild: *mut u32,
			) -> windows_core::HRESULT {
				unsafe {
					(windows_core::Interface::vtable(self).DecomposeHwndIdentityString)(
						windows_core::Interface::as_raw(self),
						pidstring,
						dwidstringlen,
						phwnd as _,
						pidobject as _,
						pidchild as _,
					)
				}
			}
			pub unsafe fn SetHmenuProp(
				&self,
				hmenu: HMENU,
				idchild: u32,
				idprop: MSAAPROPID,
				var: &VARIANT,
			) -> windows_core::HRESULT {
				unsafe {
					(windows_core::Interface::vtable(self).SetHmenuProp)(
						windows_core::Interface::as_raw(self),
						hmenu,
						idchild,
						idprop,
						core::mem::transmute_copy(var),
					)
				}
			}
			pub unsafe fn SetHmenuPropStr<P3>(
				&self,
				hmenu: HMENU,
				idchild: u32,
				idprop: MSAAPROPID,
				str: P3,
			) -> windows_core::HRESULT
			where
				P3: windows_core::Param<windows_core::PCWSTR>,
			{
				unsafe {
					(windows_core::Interface::vtable(self).SetHmenuPropStr)(
						windows_core::Interface::as_raw(self),
						hmenu,
						idchild,
						idprop,
						str.param().abi(),
					)
				}
			}
			pub unsafe fn SetHmenuPropServer<P4>(
				&self,
				hmenu: HMENU,
				idchild: u32,
				paprops: *const MSAAPROPID,
				cprops: i32,
				pserver: P4,
				annoscope: AnnoScope,
			) -> windows_core::HRESULT
			where
				P4: windows_core::Param<IAccPropServer>,
			{
				unsafe {
					(windows_core::Interface::vtable(self).SetHmenuPropServer)(
						windows_core::Interface::as_raw(self),
						hmenu,
						idchild,
						paprops,
						cprops,
						pserver.param().abi(),
						annoscope,
					)
				}
			}
			pub unsafe fn ClearHmenuProps(
				&self,
				hmenu: HMENU,
				idchild: u32,
				paprops: *const MSAAPROPID,
				cprops: i32,
			) -> windows_core::HRESULT {
				unsafe {
					(windows_core::Interface::vtable(self).ClearHmenuProps)(
						windows_core::Interface::as_raw(self),
						hmenu,
						idchild,
						paprops,
						cprops,
					)
				}
			}
			pub unsafe fn ComposeHmenuIdentityString(
				&self,
				hmenu: HMENU,
				idchild: u32,
				ppidstring: *mut *mut u8,
				pdwidstringlen: *mut u32,
			) -> windows_core::HRESULT {
				unsafe {
					(windows_core::Interface::vtable(self).ComposeHmenuIdentityString)(
						windows_core::Interface::as_raw(self),
						hmenu,
						idchild,
						ppidstring as _,
						pdwidstringlen as _,
					)
				}
			}
			pub unsafe fn DecomposeHmenuIdentityString(
				&self,
				pidstring: *const u8,
				dwidstringlen: u32,
				phmenu: *mut HMENU,
				pidchild: *mut u32,
			) -> windows_core::HRESULT {
				unsafe {
					(windows_core::Interface::vtable(self).DecomposeHmenuIdentityString)(
						windows_core::Interface::as_raw(self),
						pidstring,
						dwidstringlen,
						phmenu as _,
						pidchild as _,
					)
				}
			}
		}
		#[repr(C)]
		pub struct IAccPropServices_Vtbl {
			pub base__: windows_core::IUnknown_Vtbl,
			pub SetPropValue: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				*const u8,
				u32,
				MSAAPROPID,
				VARIANT,
			) -> windows_core::HRESULT,
			pub SetPropServer: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				*const u8,
				u32,
				*const MSAAPROPID,
				i32,
				*mut core::ffi::c_void,
				AnnoScope,
			) -> windows_core::HRESULT,
			pub ClearProps: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				*const u8,
				u32,
				*const MSAAPROPID,
				i32,
			) -> windows_core::HRESULT,
			pub SetHwndProp: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				HWND,
				u32,
				u32,
				MSAAPROPID,
				VARIANT,
			) -> windows_core::HRESULT,
			pub SetHwndPropStr: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				HWND,
				u32,
				u32,
				MSAAPROPID,
				windows_core::PCWSTR,
			) -> windows_core::HRESULT,
			pub SetHwndPropServer: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				HWND,
				u32,
				u32,
				*const MSAAPROPID,
				i32,
				*mut core::ffi::c_void,
				AnnoScope,
			) -> windows_core::HRESULT,
			pub ClearHwndProps: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				HWND,
				u32,
				u32,
				*const MSAAPROPID,
				i32,
			) -> windows_core::HRESULT,
			pub ComposeHwndIdentityString: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				HWND,
				u32,
				u32,
				*mut *mut u8,
				*mut u32,
			) -> windows_core::HRESULT,
			pub DecomposeHwndIdentityString: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				*const u8,
				u32,
				*mut HWND,
				*mut u32,
				*mut u32,
			) -> windows_core::HRESULT,
			pub SetHmenuProp: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				HMENU,
				u32,
				MSAAPROPID,
				VARIANT,
			) -> windows_core::HRESULT,
			pub SetHmenuPropStr: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				HMENU,
				u32,
				MSAAPROPID,
				windows_core::PCWSTR,
			) -> windows_core::HRESULT,
			pub SetHmenuPropServer: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				HMENU,
				u32,
				*const MSAAPROPID,
				i32,
				*mut core::ffi::c_void,
				AnnoScope,
			) -> windows_core::HRESULT,
			pub ClearHmenuProps: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				HMENU,
				u32,
				*const MSAAPROPID,
				i32,
			) -> windows_core::HRESULT,
			pub ComposeHmenuIdentityString: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				HMENU,
				u32,
				*mut *mut u8,
				*mut u32,
			) -> windows_core::HRESULT,
			pub DecomposeHmenuIdentityString: unsafe extern "system" fn(
				*mut core::ffi::c_void,
				*const u8,
				u32,
				*mut HMENU,
				*mut u32,
			)
				-> windows_core::HRESULT,
		}
		pub trait IAccPropServices_Impl: windows_core::IUnknownImpl {
			fn SetPropValue(
				&self,
				pidstring: *const u8,
				dwidstringlen: u32,
				idprop: &MSAAPROPID,
				var: &VARIANT,
			) -> windows_core::Result<()>;
			fn SetPropServer(
				&self,
				pidstring: *const u8,
				dwidstringlen: u32,
				paprops: *const MSAAPROPID,
				cprops: i32,
				pserver: windows_core::Ref<IAccPropServer>,
				annoscope: AnnoScope,
			) -> windows_core::Result<()>;
			fn ClearProps(
				&self,
				pidstring: *const u8,
				dwidstringlen: u32,
				paprops: *const MSAAPROPID,
				cprops: i32,
			) -> windows_core::Result<()>;
			fn SetHwndProp(
				&self,
				hwnd: HWND,
				idobject: u32,
				idchild: u32,
				idprop: &MSAAPROPID,
				var: &VARIANT,
			) -> windows_core::Result<()>;
			fn SetHwndPropStr(
				&self,
				hwnd: HWND,
				idobject: u32,
				idchild: u32,
				idprop: &MSAAPROPID,
				str: &windows_core::PCWSTR,
			) -> windows_core::Result<()>;
			fn SetHwndPropServer(
				&self,
				hwnd: HWND,
				idobject: u32,
				idchild: u32,
				paprops: *const MSAAPROPID,
				cprops: i32,
				pserver: windows_core::Ref<IAccPropServer>,
				annoscope: AnnoScope,
			) -> windows_core::Result<()>;
			fn ClearHwndProps(
				&self,
				hwnd: HWND,
				idobject: u32,
				idchild: u32,
				paprops: *const MSAAPROPID,
				cprops: i32,
			) -> windows_core::Result<()>;
			fn ComposeHwndIdentityString(
				&self,
				hwnd: HWND,
				idobject: u32,
				idchild: u32,
				ppidstring: *mut *mut u8,
				pdwidstringlen: *mut u32,
			) -> windows_core::Result<()>;
			fn DecomposeHwndIdentityString(
				&self,
				pidstring: *const u8,
				dwidstringlen: u32,
				phwnd: *mut HWND,
				pidobject: *mut u32,
				pidchild: *mut u32,
			) -> windows_core::Result<()>;
			fn SetHmenuProp(
				&self,
				hmenu: HMENU,
				idchild: u32,
				idprop: &MSAAPROPID,
				var: &VARIANT,
			) -> windows_core::Result<()>;
			fn SetHmenuPropStr(
				&self,
				hmenu: HMENU,
				idchild: u32,
				idprop: &MSAAPROPID,
				str: &windows_core::PCWSTR,
			) -> windows_core::Result<()>;
			fn SetHmenuPropServer(
				&self,
				hmenu: HMENU,
				idchild: u32,
				paprops: *const MSAAPROPID,
				cprops: i32,
				pserver: windows_core::Ref<IAccPropServer>,
				annoscope: AnnoScope,
			) -> windows_core::Result<()>;
			fn ClearHmenuProps(
				&self,
				hmenu: HMENU,
				idchild: u32,
				paprops: *const MSAAPROPID,
				cprops: i32,
			) -> windows_core::Result<()>;
			fn ComposeHmenuIdentityString(
				&self,
				hmenu: HMENU,
				idchild: u32,
				ppidstring: *mut *mut u8,
				pdwidstringlen: *mut u32,
			) -> windows_core::Result<()>;
			fn DecomposeHmenuIdentityString(
				&self,
				pidstring: *const u8,
				dwidstringlen: u32,
				phmenu: *mut HMENU,
				pidchild: *mut u32,
			) -> windows_core::Result<()>;
		}
		impl IAccPropServices_Vtbl {
			pub const fn new<Identity: IAccPropServices_Impl, const OFFSET: isize>() -> Self {
				unsafe extern "system" fn SetPropValue<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					pidstring: *const u8,
					dwidstringlen: u32,
					idprop: MSAAPROPID,
					var: VARIANT,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::SetPropValue(
							this,
							core::mem::transmute_copy(&pidstring),
							core::mem::transmute_copy(&dwidstringlen),
							core::mem::transmute(&idprop),
							core::mem::transmute(&var),
						)
						.into()
					}
				}
				unsafe extern "system" fn SetPropServer<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					pidstring: *const u8,
					dwidstringlen: u32,
					paprops: *const MSAAPROPID,
					cprops: i32,
					pserver: *mut core::ffi::c_void,
					annoscope: AnnoScope,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::SetPropServer(
							this,
							core::mem::transmute_copy(&pidstring),
							core::mem::transmute_copy(&dwidstringlen),
							core::mem::transmute_copy(&paprops),
							core::mem::transmute_copy(&cprops),
							core::mem::transmute_copy(&pserver),
							core::mem::transmute_copy(&annoscope),
						)
						.into()
					}
				}
				unsafe extern "system" fn ClearProps<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					pidstring: *const u8,
					dwidstringlen: u32,
					paprops: *const MSAAPROPID,
					cprops: i32,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::ClearProps(
							this,
							core::mem::transmute_copy(&pidstring),
							core::mem::transmute_copy(&dwidstringlen),
							core::mem::transmute_copy(&paprops),
							core::mem::transmute_copy(&cprops),
						)
						.into()
					}
				}
				unsafe extern "system" fn SetHwndProp<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					hwnd: HWND,
					idobject: u32,
					idchild: u32,
					idprop: MSAAPROPID,
					var: VARIANT,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::SetHwndProp(
							this,
							core::mem::transmute_copy(&hwnd),
							core::mem::transmute_copy(&idobject),
							core::mem::transmute_copy(&idchild),
							core::mem::transmute(&idprop),
							core::mem::transmute(&var),
						)
						.into()
					}
				}
				unsafe extern "system" fn SetHwndPropStr<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					hwnd: HWND,
					idobject: u32,
					idchild: u32,
					idprop: MSAAPROPID,
					str: windows_core::PCWSTR,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::SetHwndPropStr(
							this,
							core::mem::transmute_copy(&hwnd),
							core::mem::transmute_copy(&idobject),
							core::mem::transmute_copy(&idchild),
							core::mem::transmute(&idprop),
							core::mem::transmute(&str),
						)
						.into()
					}
				}
				unsafe extern "system" fn SetHwndPropServer<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					hwnd: HWND,
					idobject: u32,
					idchild: u32,
					paprops: *const MSAAPROPID,
					cprops: i32,
					pserver: *mut core::ffi::c_void,
					annoscope: AnnoScope,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::SetHwndPropServer(
							this,
							core::mem::transmute_copy(&hwnd),
							core::mem::transmute_copy(&idobject),
							core::mem::transmute_copy(&idchild),
							core::mem::transmute_copy(&paprops),
							core::mem::transmute_copy(&cprops),
							core::mem::transmute_copy(&pserver),
							core::mem::transmute_copy(&annoscope),
						)
						.into()
					}
				}
				unsafe extern "system" fn ClearHwndProps<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					hwnd: HWND,
					idobject: u32,
					idchild: u32,
					paprops: *const MSAAPROPID,
					cprops: i32,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::ClearHwndProps(
							this,
							core::mem::transmute_copy(&hwnd),
							core::mem::transmute_copy(&idobject),
							core::mem::transmute_copy(&idchild),
							core::mem::transmute_copy(&paprops),
							core::mem::transmute_copy(&cprops),
						)
						.into()
					}
				}
				unsafe extern "system" fn ComposeHwndIdentityString<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					hwnd: HWND,
					idobject: u32,
					idchild: u32,
					ppidstring: *mut *mut u8,
					pdwidstringlen: *mut u32,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::ComposeHwndIdentityString(
							this,
							core::mem::transmute_copy(&hwnd),
							core::mem::transmute_copy(&idobject),
							core::mem::transmute_copy(&idchild),
							core::mem::transmute_copy(&ppidstring),
							core::mem::transmute_copy(&pdwidstringlen),
						)
						.into()
					}
				}
				unsafe extern "system" fn DecomposeHwndIdentityString<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					pidstring: *const u8,
					dwidstringlen: u32,
					phwnd: *mut HWND,
					pidobject: *mut u32,
					pidchild: *mut u32,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::DecomposeHwndIdentityString(
							this,
							core::mem::transmute_copy(&pidstring),
							core::mem::transmute_copy(&dwidstringlen),
							core::mem::transmute_copy(&phwnd),
							core::mem::transmute_copy(&pidobject),
							core::mem::transmute_copy(&pidchild),
						)
						.into()
					}
				}
				unsafe extern "system" fn SetHmenuProp<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					hmenu: HMENU,
					idchild: u32,
					idprop: MSAAPROPID,
					var: VARIANT,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::SetHmenuProp(
							this,
							core::mem::transmute_copy(&hmenu),
							core::mem::transmute_copy(&idchild),
							core::mem::transmute(&idprop),
							core::mem::transmute(&var),
						)
						.into()
					}
				}
				unsafe extern "system" fn SetHmenuPropStr<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					hmenu: HMENU,
					idchild: u32,
					idprop: MSAAPROPID,
					str: windows_core::PCWSTR,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::SetHmenuPropStr(
							this,
							core::mem::transmute_copy(&hmenu),
							core::mem::transmute_copy(&idchild),
							core::mem::transmute(&idprop),
							core::mem::transmute(&str),
						)
						.into()
					}
				}
				unsafe extern "system" fn SetHmenuPropServer<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					hmenu: HMENU,
					idchild: u32,
					paprops: *const MSAAPROPID,
					cprops: i32,
					pserver: *mut core::ffi::c_void,
					annoscope: AnnoScope,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::SetHmenuPropServer(
							this,
							core::mem::transmute_copy(&hmenu),
							core::mem::transmute_copy(&idchild),
							core::mem::transmute_copy(&paprops),
							core::mem::transmute_copy(&cprops),
							core::mem::transmute_copy(&pserver),
							core::mem::transmute_copy(&annoscope),
						)
						.into()
					}
				}
				unsafe extern "system" fn ClearHmenuProps<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					hmenu: HMENU,
					idchild: u32,
					paprops: *const MSAAPROPID,
					cprops: i32,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::ClearHmenuProps(
							this,
							core::mem::transmute_copy(&hmenu),
							core::mem::transmute_copy(&idchild),
							core::mem::transmute_copy(&paprops),
							core::mem::transmute_copy(&cprops),
						)
						.into()
					}
				}
				unsafe extern "system" fn ComposeHmenuIdentityString<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					hmenu: HMENU,
					idchild: u32,
					ppidstring: *mut *mut u8,
					pdwidstringlen: *mut u32,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::ComposeHmenuIdentityString(
							this,
							core::mem::transmute_copy(&hmenu),
							core::mem::transmute_copy(&idchild),
							core::mem::transmute_copy(&ppidstring),
							core::mem::transmute_copy(&pdwidstringlen),
						)
						.into()
					}
				}
				unsafe extern "system" fn DecomposeHmenuIdentityString<
					Identity: IAccPropServices_Impl,
					const OFFSET: isize,
				>(
					this: *mut core::ffi::c_void,
					pidstring: *const u8,
					dwidstringlen: u32,
					phmenu: *mut HMENU,
					pidchild: *mut u32,
				) -> windows_core::HRESULT {
					unsafe {
						let this: &Identity =
							&*((this as *const *const ()).offset(OFFSET) as *const Identity);
						IAccPropServices_Impl::DecomposeHmenuIdentityString(
							this,
							core::mem::transmute_copy(&pidstring),
							core::mem::transmute_copy(&dwidstringlen),
							core::mem::transmute_copy(&phmenu),
							core::mem::transmute_copy(&pidchild),
						)
						.into()
					}
				}
				Self {
					base__: windows_core::IUnknown_Vtbl::new::<Identity, OFFSET>(),
					SetPropValue: SetPropValue::<Identity, OFFSET>,
					SetPropServer: SetPropServer::<Identity, OFFSET>,
					ClearProps: ClearProps::<Identity, OFFSET>,
					SetHwndProp: SetHwndProp::<Identity, OFFSET>,
					SetHwndPropStr: SetHwndPropStr::<Identity, OFFSET>,
					SetHwndPropServer: SetHwndPropServer::<Identity, OFFSET>,
					ClearHwndProps: ClearHwndProps::<Identity, OFFSET>,
					ComposeHwndIdentityString: ComposeHwndIdentityString::<Identity, OFFSET>,
					DecomposeHwndIdentityString: DecomposeHwndIdentityString::<Identity, OFFSET>,
					SetHmenuProp: SetHmenuProp::<Identity, OFFSET>,
					SetHmenuPropStr: SetHmenuPropStr::<Identity, OFFSET>,
					SetHmenuPropServer: SetHmenuPropServer::<Identity, OFFSET>,
					ClearHmenuProps: ClearHmenuProps::<Identity, OFFSET>,
					ComposeHmenuIdentityString: ComposeHmenuIdentityString::<Identity, OFFSET>,
					DecomposeHmenuIdentityString: DecomposeHmenuIdentityString::<Identity, OFFSET>,
				}
			}
			pub fn matches(iid: &windows_core::GUID) -> bool {
				iid == &<IAccPropServices as windows_core::Interface>::IID
			}
		}
		impl windows_core::RuntimeName for IAccPropServices {}
		pub const ICC_LISTVIEW_CLASSES: i32 = 1;
		pub const IDCANCEL: i32 = 2;
		pub const IDOK: i32 = 1;
		windows_core::imp::define_interface!(
			IDispatch,
			IDispatch_Vtbl,
			0x00020400_0000_0000_c000_000000000046
		);
		windows_core::imp::interface_hierarchy!(IDispatch, windows_core::IUnknown);
		#[repr(C)]
		pub struct IDispatch_Vtbl {
			pub base__: windows_core::IUnknown_Vtbl,
			GetTypeInfoCount: usize,
			GetTypeInfo: usize,
			GetIDsOfNames: usize,
			Invoke: usize,
		}
		impl windows_core::RuntimeName for IDispatch {}
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct INITCOMMONCONTROLSEX {
			pub dwSize: u32,
			pub dwICC: u32,
		}
		windows_core::imp::define_interface!(
			IRecordInfo,
			IRecordInfo_Vtbl,
			0x0000002f_0000_0000_c000_000000000046
		);
		windows_core::imp::interface_hierarchy!(IRecordInfo, windows_core::IUnknown);
		#[repr(C)]
		pub struct IRecordInfo_Vtbl {
			pub base__: windows_core::IUnknown_Vtbl,
			RecordInit: usize,
			RecordClear: usize,
			RecordCopy: usize,
			GetGuid: usize,
			GetName: usize,
			GetSize: usize,
			GetTypeInfo: usize,
			GetField: usize,
			GetFieldNoCopy: usize,
			PutField: usize,
			PutFieldNoCopy: usize,
			GetFieldNames: usize,
			IsMatchingType: usize,
			RecordCreate: usize,
			RecordCreateCopy: usize,
			RecordDestroy: usize,
		}
		impl windows_core::RuntimeName for IRecordInfo {}
		pub const LBN_DBLCLK: i32 = 2;
		pub const LBS_NOTIFY: i32 = 1;
		pub const LB_ADDSTRING: i32 = 384;
		pub const LB_GETCOUNT: i32 = 395;
		pub const LB_GETCURSEL: i32 = 392;
		pub const LB_GETTEXT: i32 = 393;
		pub const LB_GETTEXTLEN: i32 = 394;
		pub const LB_RESETCONTENT: i32 = 388;
		pub const LB_SETCURSEL: i32 = 390;
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct LPARAM(pub isize);
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct LRESULT(pub isize);
		pub const LVCF_SUBITEM: i32 = 8;
		pub const LVCF_TEXT: i32 = 4;
		pub const LVCF_WIDTH: i32 = 2;
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct LVCOLUMNW {
			pub mask: u32,
			pub fmt: i32,
			pub cx: i32,
			pub pszText: windows_core::PWSTR,
			pub cchTextMax: i32,
			pub iSubItem: i32,
			pub iImage: i32,
			pub iOrder: i32,
			pub cxMin: i32,
			pub cxDefault: i32,
			pub cxIdeal: i32,
		}
		pub const LVIF_TEXT: i32 = 1;
		pub const LVIS_FOCUSED: i32 = 1;
		pub const LVIS_SELECTED: i32 = 2;
		pub const LVIS_STATEIMAGEMASK: i32 = 61440;
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct LVITEMW {
			pub mask: u32,
			pub iItem: i32,
			pub iSubItem: i32,
			pub state: u32,
			pub stateMask: u32,
			pub pszText: windows_core::PWSTR,
			pub cchTextMax: i32,
			pub iImage: i32,
			pub lParam: LPARAM,
			pub iIndent: i32,
			pub iGroupId: i32,
			pub cColumns: u32,
			pub puColumns: PUINT,
			pub piColFmt: *mut i32,
			pub iGroup: i32,
		}
		pub const LVM_DELETEALLITEMS: i32 = 4105;
		pub const LVM_ENSUREVISIBLE: i32 = 4115;
		pub const LVM_GETCOLUMNW: i32 = 4191;
		pub const LVM_GETCOLUMNWIDTH: i32 = 4125;
		pub const LVM_GETCOUNTPERPAGE: i32 = 4136;
		pub const LVM_GETEXTENDEDLISTVIEWSTYLE: i32 = 4151;
		pub const LVM_GETITEMCOUNT: i32 = 4100;
		pub const LVM_GETITEMSTATE: i32 = 4140;
		pub const LVM_GETITEMTEXTW: i32 = 4211;
		pub const LVM_GETNEXTITEM: i32 = 4108;
		pub const LVM_GETTOPINDEX: i32 = 4135;
		pub const LVM_INSERTCOLUMNW: i32 = 4193;
		pub const LVM_INSERTITEMW: i32 = 4173;
		pub const LVM_SETEXTENDEDLISTVIEWSTYLE: i32 = 4150;
		pub const LVM_SETITEMSTATE: i32 = 4139;
		pub const LVM_SETITEMTEXTW: i32 = 4212;
		pub const LVNI_ALL: i32 = 0;
		pub const LVNI_SELECTED: i32 = 2;
		pub const LVS_EX_CHECKBOXES: i32 = 4;
		pub const LVS_EX_FULLROWSELECT: i32 = 32;
		pub const LVS_NOCOLUMNHEADER: i32 = 16384;
		pub const LVS_REPORT: i32 = 1;
		pub const LVS_SHOWSELALWAYS: i32 = 8;
		pub const LVS_SINGLESEL: i32 = 4;
		pub type LiveSetting = i32;
		pub type MSAAPROPID = windows_core::GUID;
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct MSG {
			pub hwnd: HWND,
			pub message: u32,
			pub wParam: WPARAM,
			pub lParam: LPARAM,
			pub time: u32,
			pub pt: POINT,
		}
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct NMHDR {
			pub hwndFrom: HWND,
			pub idFrom: usize,
			pub code: u32,
		}
		pub const NM_DBLCLK: u32 = 4294967293;
		pub const OBJID_CLIENT: i32 = -4;
		pub const Off: LiveSetting = 0;
		pub const PM_REMOVE: i32 = 1;
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct POINT {
			pub x: i32,
			pub y: i32,
		}
		pub type PUINT = *mut u32;
		pub const Polite: LiveSetting = 1;
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct RECT {
			pub left: i32,
			pub top: i32,
			pub right: i32,
			pub bottom: i32,
		}
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Eq, PartialEq)]
		pub struct SAFEARRAY {
			pub cDims: u16,
			pub fFeatures: u16,
			pub cbElements: u32,
			pub cLocks: u32,
			pub pvData: *mut core::ffi::c_void,
			pub rgsabound: [SAFEARRAYBOUND; 1],
		}
		impl Default for SAFEARRAY {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct SAFEARRAYBOUND {
			pub cElements: u32,
			pub lLbound: i32,
		}
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct SCODE(pub i32);
		pub const SM_CXVSCROLL: i32 = 2;
		pub const SS_NOPREFIX: i32 = 128;
		pub const SW_HIDE: i32 = 0;
		pub const SW_SHOW: i32 = 5;
		pub const SW_SHOWNA: i32 = 8;
		pub type VARENUM = i32;
		#[repr(C)]
		pub struct VARIANT {
			pub Anonymous: VARIANT_0,
		}
		impl Clone for VARIANT {
			fn clone(&self) -> Self {
				unsafe { core::mem::transmute_copy(self) }
			}
		}
		impl Default for VARIANT {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		pub union VARIANT_0 {
			pub Anonymous: core::mem::ManuallyDrop<VARIANT_0_0>,
			pub decVal: DECIMAL,
		}
		impl Clone for VARIANT_0 {
			fn clone(&self) -> Self {
				unsafe { core::mem::transmute_copy(self) }
			}
		}
		impl Default for VARIANT_0 {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		pub struct VARIANT_0_0 {
			pub vt: VARTYPE,
			pub wReserved1: u16,
			pub wReserved2: u16,
			pub wReserved3: u16,
			pub Anonymous: VARIANT_0_0_0,
		}
		impl Clone for VARIANT_0_0 {
			fn clone(&self) -> Self {
				unsafe { core::mem::transmute_copy(self) }
			}
		}
		impl Default for VARIANT_0_0 {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		pub union VARIANT_0_0_0 {
			pub llVal: i64,
			pub lVal: i32,
			pub bVal: u8,
			pub iVal: i16,
			pub fltVal: f32,
			pub dblVal: f64,
			pub boolVal: VARIANT_BOOL,
			pub __OBSOLETE__VARIANT_BOOL: VARIANT_BOOL,
			pub scode: SCODE,
			pub cyVal: CY,
			pub date: f64,
			pub bstrVal: core::mem::ManuallyDrop<windows_core::BSTR>,
			pub punkVal: core::mem::ManuallyDrop<Option<windows_core::IUnknown>>,
			pub pdispVal: core::mem::ManuallyDrop<Option<IDispatch>>,
			pub parray: *mut SAFEARRAY,
			pub pbVal: *mut u8,
			pub piVal: *mut i16,
			pub plVal: *mut i32,
			pub pllVal: *mut i64,
			pub pfltVal: *mut f32,
			pub pdblVal: *mut f64,
			pub pboolVal: *mut VARIANT_BOOL,
			pub __OBSOLETE__VARIANT_PBOOL: *mut VARIANT_BOOL,
			pub pscode: *mut SCODE,
			pub pcyVal: *mut CY,
			pub pdate: *mut f64,
			pub pbstrVal: *mut windows_core::BSTR,
			pub ppunkVal: *mut Option<windows_core::IUnknown>,
			pub ppdispVal: *mut Option<IDispatch>,
			pub pparray: *mut *mut SAFEARRAY,
			pub pvarVal: *mut VARIANT,
			pub byref: *mut core::ffi::c_void,
			pub cVal: i8,
			pub uiVal: u16,
			pub ulVal: u32,
			pub ullVal: u64,
			pub intVal: i32,
			pub uintVal: u32,
			pub pdecVal: *mut DECIMAL,
			pub pcVal: *mut i8,
			pub puiVal: *mut u16,
			pub pulVal: *mut u32,
			pub pullVal: *mut u64,
			pub pintVal: *mut i32,
			pub puintVal: *mut u32,
			pub Anonymous: core::mem::ManuallyDrop<VARIANT_0_0_0_0>,
		}
		impl Clone for VARIANT_0_0_0 {
			fn clone(&self) -> Self {
				unsafe { core::mem::transmute_copy(self) }
			}
		}
		impl Default for VARIANT_0_0_0 {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[derive(Clone, Debug, Default, Eq, PartialEq)]
		pub struct VARIANT_0_0_0_0 {
			pub pvRecord: *mut core::ffi::c_void,
			pub pRecInfo: core::mem::ManuallyDrop<Option<IRecordInfo>>,
		}
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct VARIANT_BOOL(pub i16);
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct VARTYPE(pub u16);
		pub const VT_I4: VARENUM = 3;
		pub const WM_APP: i32 = 32768;
		pub const WM_CLOSE: i32 = 16;
		pub const WM_COMMAND: i32 = 273;
		pub const WM_GETDLGCODE: i32 = 135;
		pub const WM_INITDIALOG: i32 = 272;
		pub const WM_NCDESTROY: i32 = 130;
		pub const WM_NEXTDLGCTL: i32 = 40;
		pub const WM_NOTIFY: i32 = 78;
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct WPARAM(pub usize);
		pub const WS_BORDER: i32 = 8388608;
		pub const WS_CAPTION: i32 = 12582912;
		pub const WS_CHILD: i32 = 1073741824;
		pub const WS_POPUP: u32 = 2147483648;
		pub const WS_SYSMENU: i32 = 524288;
		pub const WS_TABSTOP: i32 = 65536;
		pub const WS_VISIBLE: i32 = 268435456;
		pub const WS_VSCROLL: i32 = 2097152;
	}
}
