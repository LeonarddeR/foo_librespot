pub mod Windows {
	pub mod Win32 {
		#[inline]
		pub unsafe fn CreateEventW<P3>(
			lpeventattributes: Option<*const SECURITY_ATTRIBUTES>,
			bmanualreset: bool,
			binitialstate: bool,
			lpname: P3,
		) -> HANDLE
		where
			P3: windows_core::Param<windows_core::PCWSTR>,
		{
			windows_core::link!("kernel32.dll" "system" fn CreateEventW(lpeventattributes : *const SECURITY_ATTRIBUTES, bmanualreset : windows_core::BOOL, binitialstate : windows_core::BOOL, lpname : windows_core::PCWSTR) -> HANDLE);
			unsafe {
				CreateEventW(
					lpeventattributes.unwrap_or(core::mem::zeroed()) as _,
					bmanualreset.into(),
					binitialstate.into(),
					lpname.param().abi(),
				)
			}
		}
		#[inline]
		pub unsafe fn DestroyWindow(hwnd: HWND) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn DestroyWindow(hwnd : HWND) -> windows_core::BOOL);
			unsafe { DestroyWindow(hwnd) }
		}
		#[inline]
		pub unsafe fn IsWindow(hwnd: Option<HWND>) -> windows_core::BOOL {
			windows_core::link!("user32.dll" "system" fn IsWindow(hwnd : HWND) -> windows_core::BOOL);
			unsafe { IsWindow(hwnd.unwrap_or(core::mem::zeroed()) as _) }
		}
		#[inline]
		pub unsafe fn RaiseException(
			dwexceptioncode: u32,
			dwexceptionflags: u32,
			lparguments: Option<&[usize]>,
		) {
			windows_core::link!("kernel32.dll" "system" fn RaiseException(dwexceptioncode : u32, dwexceptionflags : u32, nnumberofarguments : u32, lparguments : *const usize));
			unsafe {
				RaiseException(
					dwexceptioncode,
					dwexceptionflags,
					lparguments.map_or(0, |slice| slice.len().try_into().unwrap()),
					lparguments.map_or(core::ptr::null(), |slice| slice.as_ptr()),
				)
			}
		}
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		#[inline]
		pub unsafe fn RtlUnwindEx(
			targetframe: Option<*const core::ffi::c_void>,
			targetip: Option<*const core::ffi::c_void>,
			exceptionrecord: Option<*const EXCEPTION_RECORD>,
			returnvalue: *const core::ffi::c_void,
			contextrecord: *const CONTEXT,
			historytable: Option<*const UNWIND_HISTORY_TABLE>,
		) {
			windows_core::link!("kernel32.dll" "system" fn RtlUnwindEx(targetframe : *const core::ffi::c_void, targetip : *const core::ffi::c_void, exceptionrecord : *const EXCEPTION_RECORD, returnvalue : *const core::ffi::c_void, contextrecord : *const CONTEXT, historytable : *const UNWIND_HISTORY_TABLE));
			unsafe {
				RtlUnwindEx(
					targetframe.unwrap_or(core::mem::zeroed()) as _,
					targetip.unwrap_or(core::mem::zeroed()) as _,
					exceptionrecord.unwrap_or(core::mem::zeroed()) as _,
					returnvalue,
					contextrecord,
					historytable.unwrap_or(core::mem::zeroed()) as _,
				)
			}
		}
		#[cfg(target_arch = "aarch64")]
		#[inline]
		pub unsafe fn RtlUnwindEx(
			targetframe: Option<*const core::ffi::c_void>,
			targetip: Option<*const core::ffi::c_void>,
			exceptionrecord: Option<*const EXCEPTION_RECORD>,
			returnvalue: *const core::ffi::c_void,
			contextrecord: *const ARM64_NT_CONTEXT,
			historytable: Option<*const UNWIND_HISTORY_TABLE>,
		) {
			windows_core::link!("kernel32.dll" "system" fn RtlUnwindEx(targetframe : *const core::ffi::c_void, targetip : *const core::ffi::c_void, exceptionrecord : *const EXCEPTION_RECORD, returnvalue : *const core::ffi::c_void, contextrecord : *const ARM64_NT_CONTEXT, historytable : *const UNWIND_HISTORY_TABLE));
			unsafe {
				RtlUnwindEx(
					targetframe.unwrap_or(core::mem::zeroed()) as _,
					targetip.unwrap_or(core::mem::zeroed()) as _,
					exceptionrecord.unwrap_or(core::mem::zeroed()) as _,
					returnvalue,
					contextrecord,
					historytable.unwrap_or(core::mem::zeroed()) as _,
				)
			}
		}
		#[inline]
		pub unsafe fn SetErrorMode(umode: u32) -> u32 {
			windows_core::link!("kernel32.dll" "system" fn SetErrorMode(umode : u32) -> u32);
			unsafe { SetErrorMode(umode) }
		}
		#[inline]
		pub unsafe fn ShellExecuteW<P1, P2, P3, P4>(
			hwnd: Option<HWND>,
			lpoperation: P1,
			lpfile: P2,
			lpparameters: P3,
			lpdirectory: P4,
			nshowcmd: i32,
		) -> HINSTANCE
		where
			P1: windows_core::Param<windows_core::PCWSTR>,
			P2: windows_core::Param<windows_core::PCWSTR>,
			P3: windows_core::Param<windows_core::PCWSTR>,
			P4: windows_core::Param<windows_core::PCWSTR>,
		{
			windows_core::link!("shell32.dll" "system" fn ShellExecuteW(hwnd : HWND, lpoperation : windows_core::PCWSTR, lpfile : windows_core::PCWSTR, lpparameters : windows_core::PCWSTR, lpdirectory : windows_core::PCWSTR, nshowcmd : i32) -> HINSTANCE);
			unsafe {
				ShellExecuteW(
					hwnd.unwrap_or(core::mem::zeroed()) as _,
					lpoperation.param().abi(),
					lpfile.param().abi(),
					lpparameters.param().abi(),
					lpdirectory.param().abi(),
					nshowcmd,
				)
			}
		}
		#[repr(C, align(16))]
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86", target_arch = "x86_64"))]
		#[derive(Clone, Copy)]
		pub struct ARM64_NT_CONTEXT {
			pub ContextFlags: u32,
			pub Cpsr: u32,
			pub Anonymous: ARM64_NT_CONTEXT_0,
			pub Sp: u64,
			pub Pc: u64,
			pub V: [ARM64_NT_NEON128; 32],
			pub Fpcr: u32,
			pub Fpsr: u32,
			pub Bcr: [u32; 8],
			pub Bvr: [u64; 8],
			pub Wcr: [u32; 2],
			pub Wvr: [u64; 2],
		}
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86", target_arch = "x86_64"))]
		impl Default for ARM64_NT_CONTEXT {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86", target_arch = "x86_64"))]
		#[derive(Clone, Copy)]
		pub union ARM64_NT_CONTEXT_0 {
			pub Anonymous: ARM64_NT_CONTEXT_0_0,
			pub X: [u64; 31],
		}
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86", target_arch = "x86_64"))]
		impl Default for ARM64_NT_CONTEXT_0 {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86", target_arch = "x86_64"))]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct ARM64_NT_CONTEXT_0_0 {
			pub X0: u64,
			pub X1: u64,
			pub X2: u64,
			pub X3: u64,
			pub X4: u64,
			pub X5: u64,
			pub X6: u64,
			pub X7: u64,
			pub X8: u64,
			pub X9: u64,
			pub X10: u64,
			pub X11: u64,
			pub X12: u64,
			pub X13: u64,
			pub X14: u64,
			pub X15: u64,
			pub X16: u64,
			pub X17: u64,
			pub X18: u64,
			pub X19: u64,
			pub X20: u64,
			pub X21: u64,
			pub X22: u64,
			pub X23: u64,
			pub X24: u64,
			pub X25: u64,
			pub X26: u64,
			pub X27: u64,
			pub X28: u64,
			pub Fp: u64,
			pub Lr: u64,
		}
		#[repr(C, align(16))]
		#[cfg(target_arch = "aarch64")]
		#[derive(Clone, Copy)]
		pub struct ARM64_NT_CONTEXT {
			pub ContextFlags: u32,
			pub Cpsr: u32,
			pub Anonymous: ARM64_NT_CONTEXT_0,
			pub Sp: u64,
			pub Pc: u64,
			pub V: [NEON128; 32],
			pub Fpcr: u32,
			pub Fpsr: u32,
			pub Bcr: [u32; 8],
			pub Bvr: [u64; 8],
			pub Wcr: [u32; 2],
			pub Wvr: [u64; 2],
		}
		#[cfg(target_arch = "aarch64")]
		impl Default for ARM64_NT_CONTEXT {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[cfg(target_arch = "aarch64")]
		#[derive(Clone, Copy)]
		pub union ARM64_NT_CONTEXT_0 {
			pub Anonymous: ARM64_NT_CONTEXT_0_0,
			pub X: [u64; 31],
		}
		#[cfg(target_arch = "aarch64")]
		impl Default for ARM64_NT_CONTEXT_0 {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[cfg(target_arch = "aarch64")]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct ARM64_NT_CONTEXT_0_0 {
			pub X0: u64,
			pub X1: u64,
			pub X2: u64,
			pub X3: u64,
			pub X4: u64,
			pub X5: u64,
			pub X6: u64,
			pub X7: u64,
			pub X8: u64,
			pub X9: u64,
			pub X10: u64,
			pub X11: u64,
			pub X12: u64,
			pub X13: u64,
			pub X14: u64,
			pub X15: u64,
			pub X16: u64,
			pub X17: u64,
			pub X18: u64,
			pub X19: u64,
			pub X20: u64,
			pub X21: u64,
			pub X22: u64,
			pub X23: u64,
			pub X24: u64,
			pub X25: u64,
			pub X26: u64,
			pub X27: u64,
			pub X28: u64,
			pub Fp: u64,
			pub Lr: u64,
		}
		#[repr(C)]
		#[derive(Clone, Copy)]
		pub union ARM64_NT_NEON128 {
			pub Anonymous: ARM64_NT_NEON128_0,
			pub D: [f64; 2],
			pub S: [f32; 4],
			pub H: [u16; 8],
			pub B: [u8; 16],
		}
		impl Default for ARM64_NT_NEON128 {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct ARM64_NT_NEON128_0 {
			pub Low: u64,
			pub High: i64,
		}
		#[repr(C)]
		#[derive(Clone, Copy)]
		pub struct ARM64_RUNTIME_FUNCTION {
			pub BeginAddress: u32,
			pub Anonymous: ARM64_RUNTIME_FUNCTION_0,
		}
		impl Default for ARM64_RUNTIME_FUNCTION {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[derive(Clone, Copy)]
		pub union ARM64_RUNTIME_FUNCTION_0 {
			pub UnwindData: u32,
			pub Anonymous: ARM64_RUNTIME_FUNCTION_0_0,
		}
		impl Default for ARM64_RUNTIME_FUNCTION_0 {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct ARM64_RUNTIME_FUNCTION_0_0 {
			pub _bitfield: u32,
		}
		impl ARM64_RUNTIME_FUNCTION_0_0 {
			pub fn Flag(&self) -> u32 {
				(self._bitfield << 30) >> 30
			}
			pub fn set_Flag(&mut self, value: u32) {
				self._bitfield = (self._bitfield & !3) | (value & 3);
			}
			pub fn FunctionLength(&self) -> u32 {
				(self._bitfield << 19) >> 21
			}
			pub fn set_FunctionLength(&mut self, value: u32) {
				self._bitfield = (self._bitfield & !(2047 << 2)) | ((value & 2047) << 2);
			}
			pub fn RegF(&self) -> u32 {
				(self._bitfield << 16) >> 29
			}
			pub fn set_RegF(&mut self, value: u32) {
				self._bitfield = (self._bitfield & !(7 << 13)) | ((value & 7) << 13);
			}
			pub fn RegI(&self) -> u32 {
				(self._bitfield << 12) >> 28
			}
			pub fn set_RegI(&mut self, value: u32) {
				self._bitfield = (self._bitfield & !(15 << 16)) | ((value & 15) << 16);
			}
			pub fn H(&self) -> bool {
				(self._bitfield >> 20) & 1 != 0
			}
			pub fn set_H(&mut self, value: bool) {
				self._bitfield = (self._bitfield & !(1 << 20)) | ((value as u32) << 20);
			}
			pub fn CR(&self) -> u32 {
				(self._bitfield << 9) >> 30
			}
			pub fn set_CR(&mut self, value: u32) {
				self._bitfield = (self._bitfield & !(3 << 21)) | ((value & 3) << 21);
			}
			pub fn FrameSize(&self) -> u32 {
				self._bitfield >> 23
			}
			pub fn set_FrameSize(&mut self, value: u32) {
				self._bitfield = (self._bitfield & !(511 << 23)) | ((value & 511) << 23);
			}
		}
		#[repr(C)]
		#[cfg(target_arch = "x86")]
		#[derive(Clone, Copy, Debug, Eq, PartialEq)]
		pub struct CONTEXT {
			pub ContextFlags: u32,
			pub Dr0: u32,
			pub Dr1: u32,
			pub Dr2: u32,
			pub Dr3: u32,
			pub Dr6: u32,
			pub Dr7: u32,
			pub FloatSave: FLOATING_SAVE_AREA,
			pub SegGs: u32,
			pub SegFs: u32,
			pub SegEs: u32,
			pub SegDs: u32,
			pub Edi: u32,
			pub Esi: u32,
			pub Ebx: u32,
			pub Edx: u32,
			pub Ecx: u32,
			pub Eax: u32,
			pub Ebp: u32,
			pub Eip: u32,
			pub SegCs: u32,
			pub EFlags: u32,
			pub Esp: u32,
			pub SegSs: u32,
			pub ExtendedRegisters: [u8; 512],
		}
		#[cfg(target_arch = "x86")]
		impl Default for CONTEXT {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		#[derive(Clone, Copy)]
		pub struct CONTEXT {
			pub P1Home: u64,
			pub P2Home: u64,
			pub P3Home: u64,
			pub P4Home: u64,
			pub P5Home: u64,
			pub P6Home: u64,
			pub ContextFlags: u32,
			pub MxCsr: u32,
			pub SegCs: u16,
			pub SegDs: u16,
			pub SegEs: u16,
			pub SegFs: u16,
			pub SegGs: u16,
			pub SegSs: u16,
			pub EFlags: u32,
			pub Dr0: u64,
			pub Dr1: u64,
			pub Dr2: u64,
			pub Dr3: u64,
			pub Dr6: u64,
			pub Dr7: u64,
			pub Rax: u64,
			pub Rcx: u64,
			pub Rdx: u64,
			pub Rbx: u64,
			pub Rsp: u64,
			pub Rbp: u64,
			pub Rsi: u64,
			pub Rdi: u64,
			pub R8: u64,
			pub R9: u64,
			pub R10: u64,
			pub R11: u64,
			pub R12: u64,
			pub R13: u64,
			pub R14: u64,
			pub R15: u64,
			pub Rip: u64,
			pub Anonymous: CONTEXT_0,
			pub VectorRegister: [M128A; 26],
			pub VectorControl: u64,
			pub DebugControl: u64,
			pub LastBranchToRip: u64,
			pub LastBranchFromRip: u64,
			pub LastExceptionToRip: u64,
			pub LastExceptionFromRip: u64,
		}
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		impl Default for CONTEXT {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		#[derive(Clone, Copy)]
		pub union CONTEXT_0 {
			pub FltSave: XMM_SAVE_AREA32,
			pub Anonymous: CONTEXT_0_0,
		}
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		impl Default for CONTEXT_0 {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		#[derive(Clone, Copy, Debug, Eq, PartialEq)]
		pub struct CONTEXT_0_0 {
			pub Header: [M128A; 2],
			pub Legacy: [M128A; 8],
			pub Xmm0: M128A,
			pub Xmm1: M128A,
			pub Xmm2: M128A,
			pub Xmm3: M128A,
			pub Xmm4: M128A,
			pub Xmm5: M128A,
			pub Xmm6: M128A,
			pub Xmm7: M128A,
			pub Xmm8: M128A,
			pub Xmm9: M128A,
			pub Xmm10: M128A,
			pub Xmm11: M128A,
			pub Xmm12: M128A,
			pub Xmm13: M128A,
			pub Xmm14: M128A,
			pub Xmm15: M128A,
		}
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		impl Default for CONTEXT_0_0 {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[cfg(target_arch = "aarch64")]
		pub type CONTEXT = ARM64_NT_CONTEXT;
		#[repr(C)]
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		#[derive(Clone, Copy, Debug, Default)]
		pub struct DISPATCHER_CONTEXT {
			pub ControlPc: u64,
			pub ImageBase: u64,
			pub FunctionEntry: PRUNTIME_FUNCTION,
			pub EstablisherFrame: u64,
			pub TargetIp: u64,
			pub ContextRecord: PCONTEXT,
			pub LanguageHandler: PEXCEPTION_ROUTINE,
			pub HandlerData: *mut core::ffi::c_void,
			pub HistoryTable: *mut UNWIND_HISTORY_TABLE,
			pub ScopeIndex: u32,
			pub Fill0: u32,
		}
		#[cfg(target_arch = "aarch64")]
		pub type DISPATCHER_CONTEXT = DISPATCHER_CONTEXT_ARM64;
		#[repr(C)]
		#[cfg(target_arch = "x86")]
		#[derive(Clone, Copy, Debug, Default)]
		pub struct DISPATCHER_CONTEXT_ARM64 {
			pub ControlPc: usize,
			pub ImageBase: usize,
			pub FunctionEntry: PARM64_RUNTIME_FUNCTION,
			pub EstablisherFrame: usize,
			pub TargetPc: usize,
			pub ContextRecord: PARM64_NT_CONTEXT,
			pub LanguageHandler: PEXCEPTION_ROUTINE,
			pub HandlerData: *mut core::ffi::c_void,
			pub HistoryTable: *mut _UNWIND_HISTORY_TABLE,
			pub ScopeIndex: u32,
			pub ControlPcIsUnwound: bool,
			pub NonVolatileRegisters: PBYTE,
		}
		#[repr(C)]
		#[cfg(any(target_arch = "aarch64", target_arch = "arm64ec", target_arch = "x86_64"))]
		#[derive(Clone, Copy, Debug, Default)]
		pub struct DISPATCHER_CONTEXT_ARM64 {
			pub ControlPc: usize,
			pub ImageBase: usize,
			pub FunctionEntry: PARM64_RUNTIME_FUNCTION,
			pub EstablisherFrame: usize,
			pub TargetPc: usize,
			pub ContextRecord: PARM64_NT_CONTEXT,
			pub LanguageHandler: PEXCEPTION_ROUTINE,
			pub HandlerData: *mut core::ffi::c_void,
			pub HistoryTable: *mut UNWIND_HISTORY_TABLE,
			pub ScopeIndex: u32,
			pub ControlPcIsUnwound: bool,
			pub NonVolatileRegisters: PBYTE,
		}
		pub type EXCEPTION_DISPOSITION = i32;
		pub const EXCEPTION_NONCONTINUABLE: i32 = 1;
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Eq, PartialEq)]
		pub struct EXCEPTION_RECORD {
			pub ExceptionCode: u32,
			pub ExceptionFlags: u32,
			pub ExceptionRecord: *mut Self,
			pub ExceptionAddress: *mut core::ffi::c_void,
			pub NumberParameters: u32,
			pub ExceptionInformation: [usize; 15],
		}
		impl Default for EXCEPTION_RECORD {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		pub const ExceptionCollidedUnwind: EXCEPTION_DISPOSITION = 3;
		pub const ExceptionContinueExecution: EXCEPTION_DISPOSITION = 0;
		pub const ExceptionContinueSearch: EXCEPTION_DISPOSITION = 1;
		pub const ExceptionNestedException: EXCEPTION_DISPOSITION = 2;
		#[repr(C)]
		#[cfg(target_arch = "x86")]
		#[derive(Clone, Copy, Debug, Eq, PartialEq)]
		pub struct FLOATING_SAVE_AREA {
			pub ControlWord: u32,
			pub StatusWord: u32,
			pub TagWord: u32,
			pub ErrorOffset: u32,
			pub ErrorSelector: u32,
			pub DataOffset: u32,
			pub DataSelector: u32,
			pub RegisterArea: [u8; 80],
			pub Spare0: u32,
		}
		#[cfg(target_arch = "x86")]
		impl Default for FLOATING_SAVE_AREA {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct HANDLE(pub *mut core::ffi::c_void);
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct HINSTANCE(pub *mut core::ffi::c_void);
		#[repr(transparent)]
		#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
		pub struct HWND(pub *mut core::ffi::c_void);
		#[repr(C, align(16))]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct M128A {
			pub Low: u64,
			pub High: i64,
		}
		#[cfg(target_arch = "aarch64")]
		pub type NEON128 = ARM64_NT_NEON128;
		pub type PARM64_NT_CONTEXT = *mut ARM64_NT_CONTEXT;
		pub type PARM64_RUNTIME_FUNCTION = *mut ARM64_RUNTIME_FUNCTION;
		pub type PBYTE = *mut u8;
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86", target_arch = "x86_64"))]
		pub type PCONTEXT = *mut CONTEXT;
		#[cfg(target_arch = "aarch64")]
		pub type PCONTEXT = *mut ARM64_NT_CONTEXT;
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86", target_arch = "x86_64"))]
		pub type PEXCEPTION_ROUTINE = Option<
			unsafe extern "system" fn(
				exceptionrecord: *mut EXCEPTION_RECORD,
				establisherframe: *const core::ffi::c_void,
				contextrecord: *mut CONTEXT,
				dispatchercontext: *const core::ffi::c_void,
			) -> EXCEPTION_DISPOSITION,
		>;
		#[cfg(target_arch = "aarch64")]
		pub type PEXCEPTION_ROUTINE = Option<
			unsafe extern "system" fn(
				exceptionrecord: *mut EXCEPTION_RECORD,
				establisherframe: *const core::ffi::c_void,
				contextrecord: *mut ARM64_NT_CONTEXT,
				dispatchercontext: *const core::ffi::c_void,
			) -> EXCEPTION_DISPOSITION,
		>;
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		pub type PRUNTIME_FUNCTION = *mut RUNTIME_FUNCTION;
		#[cfg(target_arch = "aarch64")]
		pub type PRUNTIME_FUNCTION = *mut ARM64_RUNTIME_FUNCTION;
		#[repr(C)]
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		#[derive(Clone, Copy)]
		pub struct RUNTIME_FUNCTION {
			pub BeginAddress: u32,
			pub EndAddress: u32,
			pub Anonymous: RUNTIME_FUNCTION_0,
		}
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		impl Default for RUNTIME_FUNCTION {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		#[derive(Clone, Copy)]
		pub union RUNTIME_FUNCTION_0 {
			pub UnwindInfoAddress: u32,
			pub UnwindData: u32,
		}
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		impl Default for RUNTIME_FUNCTION_0 {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[cfg(target_arch = "aarch64")]
		pub type RUNTIME_FUNCTION = ARM64_RUNTIME_FUNCTION;
		#[repr(C)]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct SECURITY_ATTRIBUTES {
			pub nLength: u32,
			pub lpSecurityDescriptor: *mut core::ffi::c_void,
			pub bInheritHandle: windows_core::BOOL,
		}
		pub const SEM_FAILCRITICALERRORS: i32 = 1;
		pub const SEM_NOGPFAULTERRORBOX: i32 = 2;
		pub const SW_SHOWNORMAL: i32 = 1;
		#[repr(C)]
		#[cfg(any(target_arch = "aarch64", target_arch = "arm64ec", target_arch = "x86_64"))]
		#[derive(Clone, Copy, Debug, Eq, PartialEq)]
		pub struct UNWIND_HISTORY_TABLE {
			pub Count: u32,
			pub LocalHint: u8,
			pub GlobalHint: u8,
			pub Search: u8,
			pub Once: u8,
			pub LowAddress: usize,
			pub HighAddress: usize,
			pub Entry: [UNWIND_HISTORY_TABLE_ENTRY; 12],
		}
		#[cfg(any(target_arch = "aarch64", target_arch = "arm64ec", target_arch = "x86_64"))]
		impl Default for UNWIND_HISTORY_TABLE {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[cfg(any(target_arch = "aarch64", target_arch = "arm64ec", target_arch = "x86_64"))]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct UNWIND_HISTORY_TABLE_ENTRY {
			pub ImageBase: usize,
			pub FunctionEntry: PRUNTIME_FUNCTION,
		}
		#[cfg(any(target_arch = "arm64ec", target_arch = "x86_64"))]
		pub type XMM_SAVE_AREA32 = XSAVE_FORMAT;
		#[repr(C)]
		#[cfg(target_arch = "x86")]
		#[derive(Clone, Copy, Debug, Eq, PartialEq)]
		pub struct XSAVE_FORMAT {
			pub ControlWord: u16,
			pub StatusWord: u16,
			pub TagWord: u8,
			pub Reserved1: u8,
			pub ErrorOpcode: u16,
			pub ErrorOffset: u32,
			pub ErrorSelector: u16,
			pub Reserved2: u16,
			pub DataOffset: u32,
			pub DataSelector: u16,
			pub Reserved3: u16,
			pub MxCsr: u32,
			pub MxCsr_Mask: u32,
			pub FloatRegisters: [M128A; 8],
			pub XmmRegisters: [M128A; 8],
			pub Reserved4: [u8; 224],
		}
		#[cfg(target_arch = "x86")]
		impl Default for XSAVE_FORMAT {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[cfg(any(target_arch = "aarch64", target_arch = "arm64ec", target_arch = "x86_64"))]
		#[derive(Clone, Copy, Debug, Eq, PartialEq)]
		pub struct XSAVE_FORMAT {
			pub ControlWord: u16,
			pub StatusWord: u16,
			pub TagWord: u8,
			pub Reserved1: u8,
			pub ErrorOpcode: u16,
			pub ErrorOffset: u32,
			pub ErrorSelector: u16,
			pub Reserved2: u16,
			pub DataOffset: u32,
			pub DataSelector: u16,
			pub Reserved3: u16,
			pub MxCsr: u32,
			pub MxCsr_Mask: u32,
			pub FloatRegisters: [M128A; 8],
			pub XmmRegisters: [M128A; 16],
			pub Reserved4: [u8; 96],
		}
		#[cfg(any(target_arch = "aarch64", target_arch = "arm64ec", target_arch = "x86_64"))]
		impl Default for XSAVE_FORMAT {
			fn default() -> Self {
				unsafe { core::mem::zeroed() }
			}
		}
		#[repr(C)]
		#[cfg(target_arch = "x86")]
		#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
		pub struct _UNWIND_HISTORY_TABLE(pub u8);
	}
}
