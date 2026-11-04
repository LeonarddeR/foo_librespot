//! A fake foobar2000 host for tests: implements the host side of the interfaces in Rust and
//! drives services through their vtables, exactly as foobar2000 does.

use std::ffi::{CString, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::abi::abort_callback::{AbortCallback, AbortCallbackVtbl};
use crate::abi::audio_chunk::{AudioChunk as RawAudioChunk, AudioChunkVtbl};
use crate::abi::component::{Api, ApiVtbl, ServiceClassRef};
use crate::abi::componentversion::ComponentVersionVtbl;
use crate::abi::console::{self, ConsoleReceiverVtbl};
use crate::abi::file::{FileStats, FileVtbl, StreamReaderVtbl, StreamWriterVtbl};
use crate::abi::filesystem::{self, FilesystemVtbl};
use crate::abi::initquit::InitQuitVtbl;
use crate::abi::input::{DecoderVtbl, InputEntryVtbl};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::{StringBase, StringBaseVtbl};
use crate::cxx::{ExceptionKind, HostException, try_host};
use crate::guid::Guid;
use crate::host;
use crate::service::{ServiceImpl, ServiceObject, ServicePtr, new_service};

#[cfg(feature = "testing")]
pub use loader::LoadedComponent;

/// A fake `foobar2000_api`: a registry of services, a console that records what is printed, and
/// a temporary profile folder.
#[repr(C)]
pub struct FakeApi {
	base: Api,
	classes: Mutex<Vec<(Guid, Vec<ServicePtr>)>>,
	console: Arc<Mutex<Vec<String>>>,
	profile_path: CString,
}

static API_VTBL: ApiVtbl = ApiVtbl {
	service_enum_find_class: find_class,
	service_enum_create: create,
	service_enum_get_count: get_count,
	get_main_window,
	assert_main_thread: yes,
	is_main_thread: yes,
	is_shutting_down: no,
	get_profile_path,
	is_initializing: no,
	is_portable_mode_enabled: no,
	is_quiet_mode_enabled: yes,
};

impl FakeApi {
	/// A new fake host. It is leaked, because a loaded component keeps its pointer forever.
	pub fn new() -> &'static Self {
		let console = Arc::new(Mutex::new(Vec::new()));
		let profile =
			std::env::temp_dir().join(format!("oxibar-fake-profile-{}", std::process::id()));
		let profile_path =
			CString::new(format!("file://{}", profile.display())).expect("no NUL in a temp path");
		let api: &'static Self = Box::leak(Box::new(Self {
			base: Api { vtbl: &raw const API_VTBL },
			classes: Mutex::new(Vec::new()),
			console: console.clone(),
			profile_path,
		}));
		api.add_service(console::IID, new_service(FakeConsole { lines: console }));
		api
	}

	/// The `foobar2000_api *` to hand to a component.
	pub fn as_raw(&'static self) -> *mut Api {
		(&raw const self.base).cast_mut()
	}

	/// Registers `service` as one of the services of class `iid`.
	pub fn add_service(&self, iid: Guid, service: ServicePtr) {
		let mut classes = self.classes.lock().unwrap_or_else(PoisonError::into_inner);
		match classes.iter_mut().find(|(class, _)| *class == iid) {
			Some((_, services)) => services.push(service),
			None => classes.push((iid, vec![service])),
		}
	}

	/// Every line printed to the console so far.
	pub fn console_lines(&self) -> Vec<String> {
		self.console.lock().unwrap_or_else(PoisonError::into_inner).clone()
	}

	/// The profile path the fake reports, `file://` URL included.
	pub fn profile_path(&self) -> String {
		self.profile_path.to_string_lossy().into_owned()
	}

	/// Makes this fake the host of the OxiBar inside the test binary itself, to test services
	/// in-process without loading a DLL.
	pub fn install(&'static self) {
		host::set_api(self.as_raw(), std::ptr::null_mut());
		host::set_services_available(true);
	}
}

unsafe fn fake<'a>(this: *mut Api) -> &'a FakeApi {
	unsafe { &*this.cast::<FakeApi>() }
}

unsafe extern "C-unwind" fn find_class(this: *mut Api, guid: *const Guid) -> ServiceClassRef {
	let classes = unsafe { fake(this) }.classes.lock().unwrap_or_else(PoisonError::into_inner);
	// The class reference is the 1-based index, so 0 means "no such class".
	let index = classes
		.iter()
		.position(|(class, _)| unsafe { *class == *guid })
		.map_or(0, |index| index + 1);
	std::ptr::without_provenance(index)
}

unsafe extern "C-unwind" fn get_count(this: *mut Api, class: ServiceClassRef) -> usize {
	let classes = unsafe { fake(this) }.classes.lock().unwrap_or_else(PoisonError::into_inner);
	class
		.addr()
		.checked_sub(1)
		.and_then(|index| classes.get(index))
		.map_or(0, |(_, services)| services.len())
}

unsafe extern "C-unwind" fn create(
	this: *mut Api,
	out: *mut *mut ServiceBase,
	class: ServiceClassRef,
	index: usize,
) -> bool {
	let classes = unsafe { fake(this) }.classes.lock().unwrap_or_else(PoisonError::into_inner);
	let Some(service) = class
		.addr()
		.checked_sub(1)
		.and_then(|class| classes.get(class))
		.and_then(|(_, services)| services.get(index))
	else {
		return false;
	};
	unsafe {
		let previous = std::mem::replace(&mut *out, service.clone().into_raw());
		drop(ServicePtr::from_raw_owned(previous));
	}
	true
}

unsafe extern "C-unwind" fn get_main_window(_this: *mut Api) -> *mut c_void {
	std::ptr::null_mut()
}

unsafe extern "C-unwind" fn yes(_this: *mut Api) -> bool {
	true
}

unsafe extern "C-unwind" fn no(_this: *mut Api) -> bool {
	false
}

unsafe extern "C-unwind" fn get_profile_path(this: *mut Api) -> *const c_char {
	unsafe { fake(this) }.profile_path.as_ptr()
}

/// Bytes up to the NUL terminator, or up to `length` if that comes first, as pfc reads strings.
unsafe fn bytes_of<'a>(text: *const c_char, length: usize) -> &'a [u8] {
	if text.is_null() {
		return &[];
	}
	let mut len = 0;
	while len < length && unsafe { *text.add(len) } != 0 {
		len += 1;
	}
	unsafe { std::slice::from_raw_parts(text.cast(), len) }
}

struct FakeConsole {
	lines: Arc<Mutex<Vec<String>>>,
}

unsafe impl ServiceImpl for FakeConsole {
	type Vtbl = ConsoleReceiverVtbl;
	const IIDS: &'static [Guid] = &[console::IID];
	const VTBL: &'static ConsoleReceiverVtbl =
		&ConsoleReceiverVtbl { base: ServiceBaseVtbl::new::<Self>(), print: console_print };
}

unsafe extern "C-unwind" fn console_print(
	this: *mut ServiceBase,
	message: *const c_char,
	length: usize,
) {
	let console = unsafe { ServiceObject::<FakeConsole>::value(this) };
	let line = String::from_utf8_lossy(unsafe { bytes_of(message, length) }).into_owned();
	console.lines.lock().unwrap_or_else(PoisonError::into_inner).push(line);
}

/// A `pfc::string_base` implemented in Rust, to receive string out-parameters.
#[repr(C)]
pub struct FakeString {
	base: StringBase,
	/// Always NUL-terminated.
	bytes: Vec<u8>,
}

static FAKE_STRING_VTBL: StringBaseVtbl = StringBaseVtbl {
	add_string: string_add,
	get_ptr: string_get_ptr,
	set_string: string_set,
	truncate: string_truncate,
	get_length: string_get_length,
	lock_buffer: string_lock_buffer,
	unlock_buffer: string_unlock_buffer,
};

impl FakeString {
	pub fn new() -> Box<Self> {
		Box::new(Self { base: StringBase { vtbl: &raw const FAKE_STRING_VTBL }, bytes: vec![0] })
	}

	/// The `pfc::string_base &` to pass to a slot.
	pub fn as_raw(&mut self) -> *mut StringBase {
		(&raw mut self.base).cast()
	}

	pub fn value(&self) -> String {
		String::from_utf8_lossy(&self.bytes[..self.bytes.len() - 1]).into_owned()
	}
}

unsafe fn fake_string<'a>(this: *const StringBase) -> &'a mut FakeString {
	unsafe { &mut *this.cast::<FakeString>().cast_mut() }
}

unsafe extern "C-unwind" fn string_add(this: *mut StringBase, text: *const c_char, length: usize) {
	let string = unsafe { fake_string(this) };
	let added = unsafe { bytes_of(text, length) }.to_vec();
	string.bytes.pop();
	string.bytes.extend_from_slice(&added);
	string.bytes.push(0);
}

unsafe extern "C-unwind" fn string_get_ptr(this: *const StringBase) -> *const c_char {
	unsafe { fake_string(this) }.bytes.as_ptr().cast()
}

unsafe extern "C-unwind" fn string_set(this: *mut StringBase, text: *const c_char, length: usize) {
	let string = unsafe { fake_string(this) };
	let value = unsafe { bytes_of(text, length) }.to_vec();
	string.bytes = value;
	string.bytes.push(0);
}

unsafe extern "C-unwind" fn string_truncate(this: *mut StringBase, length: usize) {
	let string = unsafe { fake_string(this) };
	if length < string.bytes.len() - 1 {
		string.bytes.truncate(length);
		string.bytes.push(0);
	}
}

unsafe extern "C-unwind" fn string_get_length(this: *const StringBase) -> usize {
	unsafe { fake_string(this) }.bytes.len() - 1
}

unsafe extern "C-unwind" fn string_lock_buffer(
	this: *mut StringBase,
	length: usize,
) -> *mut c_char {
	let string = unsafe { fake_string(this) };
	string.bytes.resize(length + 1, 0);
	string.bytes.as_mut_ptr().cast()
}

unsafe extern "C-unwind" fn string_unlock_buffer(this: *mut StringBase) {
	let string = unsafe { fake_string(this) };
	let end = string.bytes.iter().position(|&byte| byte == 0).unwrap_or(string.bytes.len());
	string.bytes.truncate(end);
	string.bytes.push(0);
}

/// Counts how often a [`CountedService`] was destroyed.
#[derive(Clone)]
pub struct Counter(Arc<AtomicUsize>);

impl Counter {
	pub fn get(&self) -> usize {
		self.0.load(Ordering::SeqCst)
	}
}

/// The interface a [`CountedService`] answers to.
pub const COUNTED_SERVICE_IID: Guid = Guid::from_u128(0x6f1d3c52_8a0e_4b7c_9e21_5d4c3b2a1908);

/// A service that only counts its own destruction, to check reference counting.
pub struct CountedService {
	destroyed: Arc<AtomicUsize>,
}

impl CountedService {
	/// A new counted service, owned by the returned pointer, and its destruction counter.
	#[allow(clippy::new_ret_no_self)]
	pub fn new() -> (ServicePtr, Counter) {
		let destroyed = Arc::new(AtomicUsize::new(0));
		(new_service(Self { destroyed: destroyed.clone() }), Counter(destroyed))
	}
}

impl Drop for CountedService {
	fn drop(&mut self) {
		self.destroyed.fetch_add(1, Ordering::SeqCst);
	}
}

unsafe impl ServiceImpl for CountedService {
	type Vtbl = ServiceBaseVtbl;
	const IIDS: &'static [Guid] = &[COUNTED_SERVICE_IID];
	const VTBL: &'static ServiceBaseVtbl = &ServiceBaseVtbl::new::<Self>();
}

/// The four strings a `componentversion` service reports.
#[derive(Debug)]
pub struct ComponentVersionStrings {
	pub file_name: String,
	pub name: String,
	pub version: String,
	pub about: String,
}

/// Reads all four strings from a `componentversion` service through its vtable.
pub fn component_version_strings(service: &ServicePtr) -> ComponentVersionStrings {
	let raw = service.as_raw();
	let vtbl = unsafe { &*(*raw).vtbl.cast::<ComponentVersionVtbl>() };
	let read = |slot: unsafe extern "C-unwind" fn(*mut ServiceBase, *mut StringBase)| {
		let mut out = FakeString::new();
		try_host(|| unsafe { slot(raw, out.as_raw()) }).expect("componentversion slot threw");
		out.value()
	};
	ComponentVersionStrings {
		file_name: read(vtbl.get_file_name),
		name: read(vtbl.get_component_name),
		version: read(vtbl.get_component_version),
		about: read(vtbl.get_about_message),
	}
}

/// Calls `on_init` on an `initquit` service through its vtable.
pub fn on_init(service: &ServicePtr) {
	let raw = service.as_raw();
	let vtbl = unsafe { &*(*raw).vtbl.cast::<InitQuitVtbl>() };
	try_host(|| unsafe { (vtbl.on_init)(raw) }).expect("on_init threw");
}

/// The Rust-owned `file_info`, under the name the tests have always used.
pub use crate::file_info_impl::FileInfoImpl as FakeFileInfo;

/// An `audio_chunk` implemented in Rust, recording what the decoder wrote.
#[repr(C)]
pub struct FakeAudioChunk {
	base: RawAudioChunk,
	data: Vec<f64>,
	sample_count: usize,
	sample_rate: u32,
	channels: u32,
	channel_mask: u32,
}

static FAKE_AUDIO_CHUNK_VTBL: AudioChunkVtbl = AudioChunkVtbl {
	get_data_overload_0: chunk_get_data,
	get_data_overload_1: chunk_get_data,
	get_data_size: chunk_get_data_size,
	set_data_size: chunk_set_data_size,
	get_srate: chunk_get_srate,
	set_srate: chunk_set_srate,
	get_channels: chunk_get_channels,
	get_channel_config: chunk_get_channel_config,
	set_channels: chunk_set_channels,
	get_sample_count: chunk_get_sample_count,
	set_sample_count: chunk_set_sample_count,
};

impl FakeAudioChunk {
	pub fn new() -> Box<Self> {
		Box::new(Self {
			base: RawAudioChunk { vtbl: &raw const FAKE_AUDIO_CHUNK_VTBL },
			data: Vec::new(),
			sample_count: 0,
			sample_rate: 0,
			channels: 0,
			channel_mask: 0,
		})
	}

	/// The `audio_chunk &` to pass to a slot.
	pub fn as_raw(&mut self) -> *mut RawAudioChunk {
		(&raw mut self.base).cast()
	}

	pub fn sample_count(&self) -> usize {
		self.sample_count
	}

	pub fn sample_rate(&self) -> u32 {
		self.sample_rate
	}

	pub fn channels(&self) -> u32 {
		self.channels
	}

	pub fn channel_mask(&self) -> u32 {
		self.channel_mask
	}

	/// The interleaved samples of the chunk's frames.
	pub fn samples(&self) -> &[f64] {
		let count = (self.sample_count * self.channels as usize).min(self.data.len());
		&self.data[..count]
	}
}

unsafe fn fake_chunk<'a>(this: *const RawAudioChunk) -> &'a mut FakeAudioChunk {
	unsafe { &mut *this.cast::<FakeAudioChunk>().cast_mut() }
}

unsafe extern "C-unwind" fn chunk_get_data(this: *mut RawAudioChunk) -> *mut f64 {
	unsafe { fake_chunk(this) }.data.as_mut_ptr()
}

unsafe extern "C-unwind" fn chunk_get_data_size(this: *const RawAudioChunk) -> usize {
	unsafe { fake_chunk(this) }.data.len()
}

unsafe extern "C-unwind" fn chunk_set_data_size(this: *mut RawAudioChunk, size: usize) {
	unsafe { fake_chunk(this) }.data.resize(size, 0.0);
}

unsafe extern "C-unwind" fn chunk_get_srate(this: *const RawAudioChunk) -> u32 {
	unsafe { fake_chunk(this) }.sample_rate
}

unsafe extern "C-unwind" fn chunk_set_srate(this: *mut RawAudioChunk, rate: u32) {
	unsafe { fake_chunk(this) }.sample_rate = rate;
}

unsafe extern "C-unwind" fn chunk_get_channels(this: *const RawAudioChunk) -> u32 {
	unsafe { fake_chunk(this) }.channels
}

unsafe extern "C-unwind" fn chunk_get_channel_config(this: *const RawAudioChunk) -> u32 {
	unsafe { fake_chunk(this) }.channel_mask
}

unsafe extern "C-unwind" fn chunk_set_channels(this: *mut RawAudioChunk, count: u32, config: u32) {
	let chunk = unsafe { fake_chunk(this) };
	chunk.channels = count;
	chunk.channel_mask = config;
}

unsafe extern "C-unwind" fn chunk_get_sample_count(this: *const RawAudioChunk) -> usize {
	unsafe { fake_chunk(this) }.sample_count
}

unsafe extern "C-unwind" fn chunk_set_sample_count(this: *mut RawAudioChunk, count: usize) {
	unsafe { fake_chunk(this) }.sample_count = count;
}

/// An `abort_callback` implemented in Rust.
#[repr(C)]
pub struct FakeAbort {
	base: AbortCallback,
	aborting: AtomicBool,
}

static FAKE_ABORT_VTBL: AbortCallbackVtbl =
	AbortCallbackVtbl { is_aborting: abort_is_aborting, get_abort_event: abort_get_event };

impl FakeAbort {
	pub fn new(aborting: bool) -> Box<Self> {
		Box::new(Self {
			base: AbortCallback { vtbl: &raw const FAKE_ABORT_VTBL },
			aborting: AtomicBool::new(aborting),
		})
	}

	/// The `abort_callback &` to pass to a slot.
	pub fn as_raw(&self) -> *mut AbortCallback {
		(&raw const self.base).cast_mut()
	}

	pub fn set_aborting(&self, aborting: bool) {
		self.aborting.store(aborting, Ordering::SeqCst);
	}
}

unsafe extern "C-unwind" fn abort_is_aborting(this: *const AbortCallback) -> bool {
	unsafe { &*this.cast::<FakeAbort>() }.aborting.load(Ordering::SeqCst)
}

unsafe extern "C-unwind" fn abort_get_event(_this: *const AbortCallback) -> *mut c_void {
	std::ptr::null_mut()
}

/// A filesystem that claims `file://` paths and finds nothing there: its `get_stats` throws
/// `exception_io_not_found`, as foobar2000's own filesystem does for a missing file.
pub struct FakeFilesystem;

impl FakeFilesystem {
	pub fn service() -> ServicePtr {
		new_service(Self)
	}
}

unsafe impl ServiceImpl for FakeFilesystem {
	type Vtbl = FilesystemVtbl;
	const IIDS: &'static [Guid] = &[filesystem::IID];
	const VTBL: &'static FilesystemVtbl = &FilesystemVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_canonical_path: filesystem_no_path,
		is_our_path: filesystem_is_our_path,
		get_display_path: filesystem_no_path,
		open: filesystem_open,
		remove: filesystem_remove,
		move_file: filesystem_move,
		is_remote: filesystem_is_remote,
		get_stats: filesystem_get_stats,
		relative_path_create: filesystem_no_relative_path,
		relative_path_parse: filesystem_no_relative_path,
		create_directory: filesystem_remove,
		list_directory: filesystem_list_directory,
		supports_content_types: filesystem_no_content_types,
	};
}

unsafe extern "C-unwind" fn filesystem_no_relative_path(
	_this: *mut ServiceBase,
	_first: *const c_char,
	_second: *const c_char,
	_out: *mut StringBase,
) -> bool {
	false
}

unsafe extern "C-unwind" fn filesystem_list_directory(
	_this: *mut ServiceBase,
	_path: *const c_char,
	_callback: *mut c_void,
	_abort: *mut AbortCallback,
) {
}

unsafe extern "C-unwind" fn filesystem_no_content_types(_this: *mut ServiceBase) -> bool {
	false
}

unsafe extern "C-unwind" fn filesystem_no_path(
	_this: *mut ServiceBase,
	_path: *const c_char,
	_out: *mut StringBase,
) -> bool {
	false
}

unsafe extern "C-unwind" fn filesystem_is_our_path(
	_this: *mut ServiceBase,
	path: *const c_char,
) -> bool {
	unsafe { bytes_of(path, usize::MAX) }.starts_with(b"file://")
}

unsafe extern "C-unwind" fn filesystem_open(
	_this: *mut ServiceBase,
	_out: *mut *mut ServiceBase,
	_path: *const c_char,
	_mode: u32,
	_abort: *mut AbortCallback,
) {
	unsafe {
		crate::cxx::throw(ExceptionKind::IoNotFound, ExceptionKind::IoNotFound.default_message())
	}
}

unsafe extern "C-unwind" fn filesystem_remove(
	_this: *mut ServiceBase,
	_path: *const c_char,
	_abort: *mut AbortCallback,
) {
	unsafe {
		crate::cxx::throw(ExceptionKind::IoNotFound, ExceptionKind::IoNotFound.default_message())
	}
}

unsafe extern "C-unwind" fn filesystem_move(
	_this: *mut ServiceBase,
	_source: *const c_char,
	_destination: *const c_char,
	_abort: *mut AbortCallback,
) {
	unsafe {
		crate::cxx::throw(ExceptionKind::IoNotFound, ExceptionKind::IoNotFound.default_message())
	}
}

unsafe extern "C-unwind" fn filesystem_is_remote(
	_this: *mut ServiceBase,
	_path: *const c_char,
) -> bool {
	false
}

unsafe extern "C-unwind" fn filesystem_get_stats(
	_this: *mut ServiceBase,
	_path: *const c_char,
	_stats: *mut FileStats,
	_is_writeable: *mut bool,
	_abort: *mut AbortCallback,
) {
	// Thrown straight from the slot, as C++ code would; there is no catch_unwind here to abort.
	unsafe {
		crate::cxx::throw(ExceptionKind::IoNotFound, ExceptionKind::IoNotFound.default_message())
	}
}

/// Calls an `input_entry` service through its vtable, as foobar2000 does.
pub struct InputEntryHandle(ServicePtr);

impl InputEntryHandle {
	pub fn new(service: ServicePtr) -> Self {
		Self(service)
	}

	fn vtbl(&self) -> &InputEntryVtbl {
		unsafe { &*(*self.0.as_raw()).vtbl.cast::<InputEntryVtbl>() }
	}

	pub fn is_our_path(&self, path: &str) -> bool {
		let path = CString::new(path).expect("no NUL in a path");
		try_host(|| unsafe {
			(self.vtbl().is_our_path)(self.0.as_raw(), path.as_ptr(), c"".as_ptr())
		})
		.expect("is_our_path threw")
	}

	pub fn is_our_content_type(&self, content_type: &str) -> bool {
		let content_type = CString::new(content_type).expect("no NUL in a content type");
		unsafe { (self.vtbl().is_our_content_type)(self.0.as_raw(), content_type.as_ptr()) }
	}

	pub fn flags(&self) -> u32 {
		unsafe { (self.vtbl().get_flags)(self.0.as_raw()) }
	}

	pub fn guid(&self) -> Guid {
		let mut guid = std::mem::MaybeUninit::<Guid>::uninit();
		unsafe { *(self.vtbl().get_guid)(self.0.as_raw(), guid.as_mut_ptr()) }
	}

	pub fn name(&self) -> String {
		unsafe { std::ffi::CStr::from_ptr((self.vtbl().get_name)(self.0.as_raw())) }
			.to_string_lossy()
			.into_owned()
	}

	pub fn preferences_guid(&self) -> Guid {
		let mut guid = std::mem::MaybeUninit::<Guid>::uninit();
		unsafe { *(self.vtbl().get_preferences_guid)(self.0.as_raw(), guid.as_mut_ptr()) }
	}

	pub fn is_low_merit(&self) -> bool {
		unsafe { (self.vtbl().is_low_merit)(self.0.as_raw()) }
	}

	/// `open_v3`, passing `hint` and `logger` by value: the input must release them.
	pub fn open_v3(
		&self,
		what_for: &Guid,
		path: &str,
		hint: Option<ServicePtr>,
		logger: Option<ServicePtr>,
		abort: &FakeAbort,
	) -> Result<DecoderHandle, HostException> {
		let path = CString::new(path).expect("no NUL in a path");
		let mut hint = hint.map_or(std::ptr::null_mut(), ServicePtr::into_raw);
		let mut logger = logger.map_or(std::ptr::null_mut(), ServicePtr::into_raw);
		let mut ret = std::mem::MaybeUninit::<*mut ServiceBase>::uninit();
		try_host(|| unsafe {
			(self.vtbl().open_v3)(
				self.0.as_raw(),
				ret.as_mut_ptr(),
				what_for,
				&raw mut hint,
				path.as_ptr(),
				&raw mut logger,
				abort.as_raw(),
			);
		})?;
		let service = unsafe { ServicePtr::from_raw_owned(ret.assume_init()) }
			.expect("open_v3 returned a service");
		Ok(DecoderHandle(service))
	}

	/// `open_for_decoding` (slot 5), with no file hint.
	pub fn open_for_decoding(
		&self,
		path: &str,
		abort: &FakeAbort,
	) -> Result<DecoderHandle, HostException> {
		let path = CString::new(path).expect("no NUL in a path");
		let mut hint: *mut ServiceBase = std::ptr::null_mut();
		let mut out: *mut ServiceBase = std::ptr::null_mut();
		try_host(|| unsafe {
			(self.vtbl().open_for_decoding)(
				self.0.as_raw(),
				&raw mut out,
				&raw mut hint,
				path.as_ptr(),
				abort.as_raw(),
			);
		})?;
		let service = unsafe { ServicePtr::from_raw_owned(out) }
			.expect("open_for_decoding returned a service");
		Ok(DecoderHandle(service))
	}
}

/// Calls a decoder through its `input_decoder_v5` vtable, as foobar2000 does.
#[derive(Debug)]
pub struct DecoderHandle(ServicePtr);

impl DecoderHandle {
	pub fn service(&self) -> &ServicePtr {
		&self.0
	}

	fn vtbl(&self) -> &DecoderVtbl {
		unsafe { &*(*self.0.as_raw()).vtbl.cast::<DecoderVtbl>() }
	}

	pub fn subsong_count(&self) -> u32 {
		unsafe { (self.vtbl().get_subsong_count)(self.0.as_raw()) }
	}

	pub fn get_info(
		&self,
		subsong: u32,
		info: &mut FakeFileInfo,
		abort: &FakeAbort,
	) -> Result<(), HostException> {
		try_host(|| unsafe {
			(self.vtbl().get_info)(self.0.as_raw(), subsong, info.as_raw(), abort.as_raw());
		})
	}

	pub fn file_stats(&self, abort: &FakeAbort) -> FileStats {
		let mut stats = std::mem::MaybeUninit::<FileStats>::uninit();
		unsafe {
			*(self.vtbl().get_file_stats)(self.0.as_raw(), stats.as_mut_ptr(), abort.as_raw())
		}
	}

	pub fn initialize(
		&self,
		subsong: u32,
		flags: u32,
		abort: &FakeAbort,
	) -> Result<(), HostException> {
		try_host(|| unsafe {
			(self.vtbl().initialize)(self.0.as_raw(), subsong, flags, abort.as_raw());
		})
	}

	pub fn run(
		&self,
		chunk: &mut FakeAudioChunk,
		abort: &FakeAbort,
	) -> Result<bool, HostException> {
		try_host(|| unsafe { (self.vtbl().run)(self.0.as_raw(), chunk.as_raw(), abort.as_raw()) })
	}

	pub fn run_raw(
		&self,
		chunk: &mut FakeAudioChunk,
		abort: &FakeAbort,
	) -> Result<bool, HostException> {
		try_host(|| unsafe {
			(self.vtbl().run_raw)(
				self.0.as_raw(),
				chunk.as_raw(),
				std::ptr::null_mut(),
				abort.as_raw(),
			)
		})
	}

	pub fn seek(&self, seconds: f64, abort: &FakeAbort) -> Result<(), HostException> {
		try_host(|| unsafe { (self.vtbl().seek)(self.0.as_raw(), seconds, abort.as_raw()) })
	}

	pub fn can_seek(&self) -> bool {
		try_host(|| unsafe { (self.vtbl().can_seek)(self.0.as_raw()) }).expect("can_seek threw")
	}

	/// `set_logger`, passing `logger` by value: the decoder must release it.
	pub fn set_logger(&self, logger: ServicePtr) {
		let mut logger = logger.into_raw();
		try_host(|| unsafe { (self.vtbl().set_logger)(self.0.as_raw(), &raw mut logger) })
			.expect("set_logger threw");
	}
}

/// Calls a `filesystem` service through its vtable, as foobar2000 does.
pub struct FilesystemHandle(ServicePtr);

impl FilesystemHandle {
	pub fn new(service: ServicePtr) -> Self {
		Self(service)
	}

	fn call<R>(
		&self,
		f: impl FnOnce(&FilesystemVtbl, *mut ServiceBase) -> R,
	) -> Result<R, HostException> {
		let raw = self.0.as_raw();
		let vtbl = unsafe { &*(*raw).vtbl.cast::<FilesystemVtbl>() };
		try_host(|| f(vtbl, raw))
	}

	pub fn is_our_path(&self, path: &str) -> bool {
		let path = CString::new(path).unwrap();
		self.call(|vtbl, raw| unsafe { (vtbl.is_our_path)(raw, path.as_ptr()) })
			.expect("is_our_path threw")
	}

	pub fn supports_content_types(&self) -> bool {
		self.call(|vtbl, raw| unsafe { (vtbl.supports_content_types)(raw) })
			.expect("supports_content_types threw")
	}

	pub fn canonical_path(&self, path: &str) -> Option<String> {
		let path = CString::new(path).unwrap();
		let mut out = FakeString::new();
		let out_raw = out.as_raw();
		let found = self
			.call(|vtbl, raw| unsafe { (vtbl.get_canonical_path)(raw, path.as_ptr(), out_raw) })
			.expect("get_canonical_path threw");
		found.then(|| out.value())
	}

	pub fn display_path(&self, path: &str) -> Option<String> {
		let path = CString::new(path).unwrap();
		let mut out = FakeString::new();
		let out_raw = out.as_raw();
		let found = self
			.call(|vtbl, raw| unsafe { (vtbl.get_display_path)(raw, path.as_ptr(), out_raw) })
			.expect("get_display_path threw");
		found.then(|| out.value())
	}

	pub fn open(&self, path: &str, mode: u32) -> Result<FileHandle, HostException> {
		let path = CString::new(path).unwrap();
		let abort = FakeAbort::new(false);
		let mut out: *mut ServiceBase = std::ptr::null_mut();
		let out_slot = &raw mut out;
		self.call(|vtbl, raw| unsafe {
			(vtbl.open)(raw, out_slot, path.as_ptr(), mode, abort.as_raw());
		})?;
		Ok(FileHandle(unsafe { ServicePtr::from_raw_owned(out) }.expect("open gave no file")))
	}

	pub fn list_directory(&self, path: &str) -> Result<(), HostException> {
		let path = CString::new(path).unwrap();
		let abort = FakeAbort::new(false);
		self.call(|vtbl, raw| unsafe {
			(vtbl.list_directory)(raw, path.as_ptr(), std::ptr::null_mut(), abort.as_raw());
		})
	}

	pub fn get_stats(&self, path: &str) -> Result<(FileStats, bool), HostException> {
		let path = CString::new(path).unwrap();
		let abort = FakeAbort::new(false);
		let mut stats = FileStats { size: 0, timestamp: 1 };
		let mut writeable = true;
		let (stats_slot, writeable_slot) = (&raw mut stats, &raw mut writeable);
		self.call(|vtbl, raw| unsafe {
			(vtbl.get_stats)(raw, path.as_ptr(), stats_slot, writeable_slot, abort.as_raw());
		})?;
		Ok((stats, writeable))
	}
}

/// Calls a `file` through its three vtables, as foobar2000 does.
pub struct FileHandle(ServicePtr);

impl FileHandle {
	pub fn service(&self) -> &ServicePtr {
		&self.0
	}

	fn primary(&self) -> (&FileVtbl, *mut ServiceBase) {
		let raw = self.0.as_raw();
		(unsafe { &*(*raw).vtbl.cast::<FileVtbl>() }, raw)
	}

	/// The `stream_reader` base, at offset 8.
	fn reader(&self) -> (&StreamReaderVtbl, *mut c_void) {
		let base = unsafe { self.0.as_raw().byte_add(8) }.cast::<c_void>();
		(unsafe { &**base.cast::<*const StreamReaderVtbl>() }, base)
	}

	/// The `stream_writer` base, at offset 16.
	fn writer(&self) -> (&StreamWriterVtbl, *mut c_void) {
		let base = unsafe { self.0.as_raw().byte_add(16) }.cast::<c_void>();
		(unsafe { &**base.cast::<*const StreamWriterVtbl>() }, base)
	}

	pub fn size(&self) -> u64 {
		let (vtbl, raw) = self.primary();
		let abort = FakeAbort::new(false);
		try_host(|| unsafe { (vtbl.get_size)(raw, abort.as_raw()) }).expect("get_size threw")
	}

	pub fn can_seek(&self) -> bool {
		let (vtbl, raw) = self.primary();
		try_host(|| unsafe { (vtbl.can_seek)(raw) }).expect("can_seek threw")
	}

	pub fn is_remote(&self) -> bool {
		let (vtbl, raw) = self.primary();
		try_host(|| unsafe { (vtbl.is_remote)(raw) }).expect("is_remote threw")
	}

	pub fn seek(&self, position: u64) -> Result<(), HostException> {
		let (vtbl, raw) = self.primary();
		let abort = FakeAbort::new(false);
		try_host(|| unsafe { (vtbl.seek)(raw, position, abort.as_raw()) })
	}

	pub fn content_type(&self) -> Option<String> {
		let (vtbl, raw) = self.primary();
		let mut out = FakeString::new();
		let out_raw = out.as_raw();
		let found = try_host(|| unsafe { (vtbl.get_content_type)(raw, out_raw) })
			.expect("get_content_type threw");
		found.then(|| out.value())
	}

	pub fn read(&self, bytes: usize) -> Result<usize, HostException> {
		let (vtbl, this) = self.reader();
		let mut buffer = vec![0u8; bytes];
		let abort = FakeAbort::new(false);
		try_host(|| unsafe { (vtbl.read)(this, buffer.as_mut_ptr().cast(), bytes, abort.as_raw()) })
	}

	pub fn read_object(&self, bytes: usize) -> Result<(), HostException> {
		let (vtbl, this) = self.reader();
		let mut buffer = vec![0u8; bytes];
		let abort = FakeAbort::new(false);
		try_host(|| unsafe {
			(vtbl.read_object)(this, buffer.as_mut_ptr().cast(), bytes, abort.as_raw());
		})
	}

	pub fn write(&self, data: &[u8]) -> Result<(), HostException> {
		let (vtbl, this) = self.writer();
		let abort = FakeAbort::new(false);
		try_host(|| unsafe { (vtbl.write)(this, data.as_ptr().cast(), data.len(), abort.as_raw()) })
	}
}

#[cfg(feature = "testing")]
mod loader {
	use std::ffi::{CString, c_void};
	use std::path::Path;

	use super::FakeApi;
	use crate::abi::component::{Api, Client, FactoryBase};
	use crate::abi::service::ServiceBase;
	use crate::cxx::try_host;
	use crate::guid::Guid;
	use crate::service::ServicePtr;

	type GetInterface = unsafe extern "C" fn(*mut Api, *mut c_void) -> *mut Client;

	/// A component DLL loaded into a [`FakeApi`], as foobar2000 loads it.
	pub struct LoadedComponent {
		client: *mut Client,
	}

	impl LoadedComponent {
		/// Loads the DLL, calls its entry point, then `set_library_path` and `services_init(true)`.
		/// The library stays loaded for the rest of the process, as in foobar2000.
		pub fn load(dll: &Path, api: &'static FakeApi) -> Self {
			let library: &'static libloading::Library = Box::leak(Box::new(
				unsafe { libloading::Library::new(dll) }
					.unwrap_or_else(|error| panic!("loading {}: {error}", dll.display())),
			));
			let get_interface =
				unsafe { library.get::<GetInterface>(b"foobar2000_get_interface\0") }
					.expect("the DLL exports foobar2000_get_interface");
			let client = unsafe { get_interface(api.as_raw(), std::ptr::null_mut()) };
			let path = CString::new(dll.to_string_lossy().as_bytes()).expect("no NUL in a path");
			let name =
				CString::new(dll.file_stem().unwrap_or_default().to_string_lossy().as_bytes())
					.expect("no NUL in a file name");
			unsafe {
				let vtbl = &*(*client).vtbl;
				(vtbl.set_library_path)(client, path.as_ptr(), name.as_ptr());
				(vtbl.services_init)(client, true);
			}
			Self { client }
		}

		pub fn client_version(&self) -> u32 {
			unsafe { ((*(*self.client).vtbl).get_version)(self.client) }
		}

		fn factory_list(&self) -> Vec<*mut FactoryBase> {
			let mut factories = Vec::new();
			let mut next = unsafe { ((*(*self.client).vtbl).get_service_list)(self.client) };
			while !next.is_null() {
				factories.push(next);
				next = unsafe { (*next).next };
			}
			factories
		}

		/// The entrypoint GUID of every registered factory, in list order.
		pub fn factories(&self) -> Vec<Guid> {
			self.factory_list().into_iter().map(|factory| unsafe { *(*factory).guid }).collect()
		}

		/// One instance from every factory registered under `iid`.
		pub fn create_all(&self, iid: &Guid) -> Vec<ServicePtr> {
			self.factory_list()
				.into_iter()
				.filter(|factory| unsafe { *(**factory).guid == *iid })
				.filter_map(|factory| {
					let mut out: *mut ServiceBase = std::ptr::null_mut();
					try_host(|| unsafe {
						((*(*factory).vtbl).instance_create)(factory, &raw mut out);
					})
					.expect("instance_create threw");
					unsafe { ServicePtr::from_raw_owned(out) }
				})
				.collect()
		}

		/// Calls the first factory for `iid` with `out` already holding `previous`, which
		/// `instance_create` must release.
		pub fn create_with_previous(&self, iid: &Guid, previous: ServicePtr) -> ServicePtr {
			let factory = self
				.factory_list()
				.into_iter()
				.find(|factory| unsafe { *(**factory).guid == *iid })
				.expect("a factory for the interface");
			let mut out = previous.into_raw();
			try_host(|| unsafe { ((*(*factory).vtbl).instance_create)(factory, &raw mut out) })
				.expect("instance_create threw");
			unsafe { ServicePtr::from_raw_owned(out) }.expect("instance_create produced a service")
		}
	}
}
