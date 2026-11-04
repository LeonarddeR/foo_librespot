//! foobar2000 inputs implemented in Rust: `input_entry` plus the decoder it opens.

use std::ffi::{CStr, c_char, c_void};
use std::marker::PhantomData;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::abi::abort_callback::AbortCallback;
use crate::abi::audio_chunk::AudioChunk as RawAudioChunk;
use crate::abi::file::FileStats;
use crate::abi::file_info::FileInfo as RawFileInfo;
use crate::abi::input::{
	ByValueServicePtr, DECODER, DECODER_V2, DECODER_V3, DECODER_V4, DECODER_V5, DecoderVtbl,
	INFO_READER, INFO_WRITER, INPUT_ENTRY, INPUT_ENTRY_V2, INPUT_ENTRY_V3, INPUT_ENTRY_V4,
	InputEntryVtbl,
};
use crate::abi::live_info::InputLiveInfo;
use crate::abi::pfc_list::ListBase;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abort::Abort;
use crate::audio_chunk::AudioChunk;
use crate::boundary::guard;
use crate::component::Factory;
use crate::cxx::ExceptionKind;
use crate::error::{Error, Result};
use crate::file_info::FileInfo;
use crate::guid::Guid;
use crate::live_info;
use crate::service::{ServiceImpl, ServiceObject, ServicePtr, new_service};

/// Why foobar2000 opens a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenReason {
	/// To play it.
	Decode,
	/// To read its metadata and technical info.
	InfoRead,
}

/// A foobar2000 input: something that recognises paths and decodes them. One value of the type
/// is one opened track; foobar2000 uses it from one thread at a time.
pub trait Input: Sized + Send + 'static {
	/// Identifies the input in foobar2000's preferences.
	const GUID: Guid;
	/// Shown in foobar2000's list of inputs.
	const NAME: &'static CStr;

	fn is_our_path(path: &str) -> bool;
	fn open(path: &str, reason: OpenReason, abort: &Abort) -> Result<Self>;
	fn get_info(&mut self, info: &mut FileInfo<'_>, abort: &Abort) -> Result<()>;
	fn decode_initialize(&mut self, flags: u32, abort: &Abort) -> Result<()>;
	/// Fills `chunk` and returns true, or returns false at the end of the track.
	fn decode_run(&mut self, chunk: &mut AudioChunk<'_>, abort: &Abort) -> Result<bool>;
	/// Seeking past the end must succeed; the next `decode_run` then returns false.
	fn decode_seek(&mut self, seconds: f64, abort: &Abort) -> Result<()>;
	fn can_seek(&self) -> bool;
	/// The technical info of what plays now, as field names and values, such as the bitrate of
	/// the file chosen; empty while there is none. foobar2000 asks after each `decode_run`.
	/// foxide2k passes it on when it differs from what foobar2000 last got, and again after each
	/// seek or `decode_initialize`, where foobar2000 forgets it; it shows from the start of the
	/// last chunk.
	fn live_info(&self) -> Vec<(String, String)> {
		Vec::new()
	}
}

/// The factory that registers `T` with foobar2000.
pub const fn input_factory<T: Input>() -> Factory {
	Factory::per_instance(&INPUT_ENTRY, entry_service::<T>)
}

/// A new `input_entry` service for `T`.
pub fn entry_service<T: Input>() -> ServicePtr {
	new_service(InputEntry::<T>(PhantomData))
}

/// Releases a `service_ptr_t` that foobar2000 passed by value, as the callee must under MSVC.
pub(crate) unsafe fn release_by_value(slot: ByValueServicePtr) {
	if !slot.is_null() {
		unsafe { drop(ServicePtr::from_raw_owned(*slot)) };
	}
}

unsafe fn path_of<'a>(path: *const c_char) -> &'a str {
	if path.is_null() { "" } else { unsafe { CStr::from_ptr(path) }.to_str().unwrap_or("") }
}

struct InputEntry<T>(PhantomData<fn() -> T>);

unsafe impl<T: Input> ServiceImpl for InputEntry<T> {
	type Vtbl = InputEntryVtbl;
	const IIDS: &'static [Guid] = &[INPUT_ENTRY_V4, INPUT_ENTRY_V3, INPUT_ENTRY_V2, INPUT_ENTRY];
	const VTBL: &'static InputEntryVtbl = &InputEntryVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		is_our_content_type,
		is_our_path: is_our_path::<T>,
		open_for_decoding: open_for::<T, 0>,
		open_for_info_read: open_for::<T, 1>,
		open_for_info_write: open_for::<T, 2>,
		get_extended_data,
		get_flags,
		get_guid: get_guid::<T>,
		get_name: get_name::<T>,
		get_preferences_guid,
		is_low_merit,
		open_v3: open_v3::<T>,
		fallback_is_our_payload,
	};
}

unsafe extern "C-unwind" fn is_our_content_type(
	_this: *mut ServiceBase,
	_content_type: *const c_char,
) -> bool {
	false
}

unsafe extern "C-unwind" fn is_our_path<T: Input>(
	_this: *mut ServiceBase,
	path: *const c_char,
	_extension: *const c_char,
) -> bool {
	unsafe { guard(|| Ok(T::is_our_path(path_of(path)))) }
}

fn open<T: Input>(
	what_for: &Guid,
	path: *const c_char,
	abort: *mut AbortCallback,
) -> Result<ServicePtr> {
	let reason = match *what_for {
		DECODER => OpenReason::Decode,
		INFO_READER => OpenReason::InfoRead,
		INFO_WRITER => return Err(Error::tagging_unsupported()),
		_ => return Err(Error::not_implemented()),
	};
	let abort = unsafe { Abort::from_raw(abort) };
	let input = T::open(unsafe { path_of(path) }, reason, &abort)?;
	Ok(new_service(Decoder { input: Mutex::new(input), reported: Mutex::new(Vec::new()) }))
}

/// Slots 5-7, `open_for_decoding`, `open_for_info_read` and `open_for_info_write`, as
/// `input_entry_v3` implements them on top of `open_v3`.
unsafe extern "C-unwind" fn open_for<T: Input, const WHAT: u8>(
	_this: *mut ServiceBase,
	instance: *mut *mut ServiceBase,
	file_hint: ByValueServicePtr,
	path: *const c_char,
	abort: *mut AbortCallback,
) {
	unsafe {
		release_by_value(file_hint);
		guard(|| {
			let what_for = match WHAT {
				0 => DECODER,
				1 => INFO_READER,
				_ => INFO_WRITER,
			};
			let service = open::<T>(&what_for, path, abort)?;
			drop(ServicePtr::from_raw_owned(std::mem::replace(&mut *instance, service.into_raw())));
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn get_extended_data(
	_this: *mut ServiceBase,
	file_hint: ByValueServicePtr,
	_location: *const c_void,
	_guid: *const Guid,
	_out: *mut c_void,
	_abort: *mut AbortCallback,
) {
	unsafe { release_by_value(file_hint) };
}

unsafe extern "C-unwind" fn get_flags(_this: *mut ServiceBase) -> u32 {
	0
}

unsafe extern "C-unwind" fn get_guid<T: Input>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
) -> *mut Guid {
	unsafe { ret.write(T::GUID) };
	ret
}

unsafe extern "C-unwind" fn get_name<T: Input>(_this: *mut ServiceBase) -> *const c_char {
	T::NAME.as_ptr()
}

unsafe extern "C-unwind" fn get_preferences_guid(
	_this: *mut ServiceBase,
	ret: *mut Guid,
) -> *mut Guid {
	unsafe { ret.write(Guid::NULL) };
	ret
}

unsafe extern "C-unwind" fn is_low_merit(_this: *mut ServiceBase) -> bool {
	false
}

unsafe extern "C-unwind" fn open_v3<T: Input>(
	_this: *mut ServiceBase,
	ret: *mut *mut ServiceBase,
	what_for: *const Guid,
	hint: ByValueServicePtr,
	path: *const c_char,
	logger: ByValueServicePtr,
	abort: *mut AbortCallback,
) -> *mut *mut ServiceBase {
	unsafe {
		release_by_value(hint);
		release_by_value(logger);
		let service = guard(|| open::<T>(&*what_for, path, abort));
		// The hidden return slot is uninitialised: write, don't release what was there.
		ret.write(service.into_raw());
	}
	ret
}

unsafe extern "C-unwind" fn fallback_is_our_payload(
	_this: *mut ServiceBase,
	_bytes: *const c_void,
	_available: usize,
	_whole: u64,
) -> bool {
	false
}

/// The decoder `open` returns: one opened track behind every reader and decoder interface.
struct Decoder<T> {
	input: Mutex<T>,
	/// The live info foobar2000 has now; cleared where it forgets it.
	reported: Mutex<Vec<(String, String)>>,
}

impl<T: Input> Decoder<T> {
	unsafe fn of<'a>(this: *mut ServiceBase) -> &'a Self {
		unsafe { ServiceObject::<Self>::value(this) }
	}

	fn input(&self) -> Result<MutexGuard<'_, T>> {
		self.input.lock().map_err(|_| {
			Error::new(ExceptionKind::BugCheck, "the input panicked earlier and is unusable")
		})
	}

	fn reported(&self) -> MutexGuard<'_, Vec<(String, String)>> {
		self.reported.lock().unwrap_or_else(PoisonError::into_inner)
	}
}

fn check_subsong(subsong: u32) -> Result<()> {
	if subsong == 0 {
		Ok(())
	} else {
		Err(Error::new(
			ExceptionKind::IoBadSubsongIndex,
			ExceptionKind::IoBadSubsongIndex.default_message(),
		))
	}
}

unsafe impl<T: Input> ServiceImpl for Decoder<T> {
	type Vtbl = DecoderVtbl;
	// Not input_info_reader_v2: answering it would need a second vtable, and foobar2000 falls back
	// to get_file_stats without it.
	const IIDS: &'static [Guid] =
		&[DECODER_V5, DECODER_V4, DECODER_V3, DECODER_V2, DECODER, INFO_READER];
	const VTBL: &'static DecoderVtbl = &DecoderVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_subsong_count,
		get_subsong,
		get_info: get_info::<T>,
		get_file_stats,
		initialize: initialize::<T>,
		run: run::<T>,
		seek: seek::<T>,
		can_seek: can_seek::<T>,
		get_dynamic_info,
		get_dynamic_info_track: get_dynamic_info,
		on_idle,
		run_raw,
		set_logger,
		set_pause,
		flush_on_pause,
		extended_param,
		poll_live_info: poll_live_info::<T>,
	};
}

unsafe extern "C-unwind" fn get_subsong_count(_this: *mut ServiceBase) -> u32 {
	1
}

unsafe extern "C-unwind" fn get_subsong(_this: *mut ServiceBase, _index: u32) -> u32 {
	0
}

unsafe extern "C-unwind" fn get_info<T: Input>(
	this: *mut ServiceBase,
	subsong: u32,
	info: *mut RawFileInfo,
	abort: *mut AbortCallback,
) {
	unsafe {
		guard(|| {
			check_subsong(subsong)?;
			Decoder::<T>::of(this)
				.input()?
				.get_info(&mut FileInfo::from_raw(info), &Abort::from_raw(abort))
		});
	}
}

unsafe extern "C-unwind" fn get_file_stats(
	_this: *mut ServiceBase,
	ret: *mut FileStats,
	_abort: *mut AbortCallback,
) -> *mut FileStats {
	unsafe { ret.write(FileStats::INVALID) };
	ret
}

unsafe extern "C-unwind" fn initialize<T: Input>(
	this: *mut ServiceBase,
	subsong: u32,
	flags: u32,
	abort: *mut AbortCallback,
) {
	unsafe {
		guard(|| {
			check_subsong(subsong)?;
			let decoder = Decoder::<T>::of(this);
			decoder.input()?.decode_initialize(flags, &Abort::from_raw(abort))?;
			decoder.reported().clear();
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn run<T: Input>(
	this: *mut ServiceBase,
	chunk: *mut RawAudioChunk,
	abort: *mut AbortCallback,
) -> bool {
	unsafe {
		guard(|| {
			Decoder::<T>::of(this)
				.input()?
				.decode_run(&mut AudioChunk::from_raw(chunk), &Abort::from_raw(abort))
		})
	}
}

unsafe extern "C-unwind" fn seek<T: Input>(
	this: *mut ServiceBase,
	seconds: f64,
	abort: *mut AbortCallback,
) {
	unsafe {
		guard(|| {
			let decoder = Decoder::<T>::of(this);
			decoder.input()?.decode_seek(seconds, &Abort::from_raw(abort))?;
			decoder.reported().clear();
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn can_seek<T: Input>(this: *mut ServiceBase) -> bool {
	unsafe { guard(|| Ok(Decoder::<T>::of(this).input()?.can_seek())) }
}

/// Replaced by `poll_live_info` in foobar2000 2.25, the oldest version Foxide2k supports.
unsafe extern "C-unwind" fn get_dynamic_info(
	_this: *mut ServiceBase,
	_out: *mut RawFileInfo,
	_timestamp_delta: *mut f64,
) -> bool {
	false
}

unsafe extern "C-unwind" fn on_idle(_this: *mut ServiceBase, _abort: *mut AbortCallback) {}

unsafe extern "C-unwind" fn run_raw(
	_this: *mut ServiceBase,
	_chunk: *mut RawAudioChunk,
	_raw: *mut c_void,
	_abort: *mut AbortCallback,
) -> bool {
	unsafe { guard(|| Err(Error::not_implemented())) }
}

unsafe extern "C-unwind" fn set_logger(_this: *mut ServiceBase, logger: ByValueServicePtr) {
	unsafe { release_by_value(logger) };
}

unsafe extern "C-unwind" fn set_pause(_this: *mut ServiceBase, _paused: bool) {}

unsafe extern "C-unwind" fn flush_on_pause(_this: *mut ServiceBase) -> bool {
	false
}

unsafe extern "C-unwind" fn extended_param(
	_this: *mut ServiceBase,
	_param_type: *const Guid,
	_arg1: usize,
	_arg2: *mut c_void,
	_arg2_size: usize,
) -> usize {
	0
}

unsafe extern "C-unwind" fn poll_live_info<T: Input>(
	this: *mut ServiceBase,
	ret: *mut ListBase<InputLiveInfo>,
	_abort: *mut AbortCallback,
) {
	unsafe {
		guard(|| {
			let decoder = Decoder::<T>::of(this);
			// The input is unlocked again before foobar2000's list is called.
			let fields = decoder.input()?.live_info();
			let mut reported = decoder.reported();
			if fields.is_empty() || *reported == fields {
				return Ok(());
			}
			live_info::add_info_change(ret, fields.clone())?;
			*reported = fields;
			Ok(())
		});
	}
}

#[cfg(test)]
mod tests {
	use std::cell::Cell;
	use std::ffi::CStr;

	use super::{Input, OpenReason, entry_service};
	use crate::abi::input::{DECODER, DECODER_V5, INFO_READER, INFO_READER_V2, INFO_WRITER};
	use crate::abi::live_info::SCOPE_INFO;
	use crate::abort::Abort;
	use crate::audio_chunk::{AudioChunk, CHANNEL_MASK_STEREO};
	use crate::cxx::ExceptionKind;
	use crate::error::{Error, Result};
	use crate::file_info::FileInfo;
	use crate::guid::Guid;
	use crate::testing::{
		CountedService, DecoderHandle, FakeAbort, FakeAudioChunk, FakeFileInfo, FakeLiveInfoList,
		InputEntryHandle,
	};

	const TEST_GUID: Guid = Guid::from_u128(0x0b6e2f5a_1c3d_4e7f_8a9b_0c1d2e3f4a5b);

	thread_local! {
		static SEEKED_TO: Cell<Option<f64>> = const { Cell::new(None) };
	}

	struct TestInput {
		runs: usize,
	}

	impl Input for TestInput {
		const GUID: Guid = TEST_GUID;
		const NAME: &'static CStr = c"Foxide2k test input";

		fn is_our_path(path: &str) -> bool {
			path.starts_with("test://")
		}

		fn open(path: &str, _reason: OpenReason, _abort: &Abort) -> Result<Self> {
			if path == "test://ok" {
				Ok(Self { runs: 0 })
			} else {
				Err(Error::unsupported_format("not a test path"))
			}
		}

		fn get_info(&mut self, info: &mut FileInfo<'_>, _abort: &Abort) -> Result<()> {
			info.meta_set("title", "T")?;
			info.set_length(1.0)
		}

		fn decode_initialize(&mut self, _flags: u32, _abort: &Abort) -> Result<()> {
			Ok(())
		}

		fn decode_run(&mut self, chunk: &mut AudioChunk<'_>, abort: &Abort) -> Result<bool> {
			abort.check()?;
			self.runs += 1;
			if self.runs > 1 {
				return Ok(false);
			}
			chunk.set_data(&[0.1, 0.2, 0.3, 0.4], 2, 44_100, 2, CHANNEL_MASK_STEREO)?;
			Ok(true)
		}

		fn decode_seek(&mut self, seconds: f64, _abort: &Abort) -> Result<()> {
			SEEKED_TO.set(Some(seconds));
			Ok(())
		}

		fn can_seek(&self) -> bool {
			true
		}

		/// A variable bitrate: 320 kbit/s in the first chunk, 256 after.
		fn live_info(&self) -> Vec<(String, String)> {
			let kbps = match self.runs {
				0 => return Vec::new(),
				1 => "320",
				_ => "256",
			};
			vec![("bitrate".to_owned(), kbps.to_owned())]
		}
	}

	fn entry() -> InputEntryHandle {
		InputEntryHandle::new(entry_service::<TestInput>())
	}

	#[test]
	fn entry_matches_path_and_not_content_type() {
		let entry = entry();
		assert!(entry.is_our_path("test://ok"));
		assert!(!entry.is_our_path("file://C:\\a.flac"));
		assert!(!entry.is_our_content_type("audio/mpeg"));
	}

	#[test]
	fn entry_reports_guid_name_and_flags() {
		let entry = entry();
		assert_eq!(entry.guid(), TEST_GUID);
		assert_eq!(entry.name(), "Foxide2k test input");
		assert_eq!(entry.flags(), 0);
		assert_eq!(entry.preferences_guid(), Guid::NULL);
		assert!(!entry.is_low_merit());
	}

	#[test]
	fn open_v3_decoder_answers_v5_but_not_reader_v2() {
		let abort = FakeAbort::new(false);
		let decoder = entry().open_v3(&DECODER, "test://ok", None, None, &abort).unwrap();
		assert!(decoder.service().query(&DECODER_V5).is_some());
		assert!(decoder.service().query(&INFO_READER).is_some());
		assert!(decoder.service().query(&INFO_READER_V2).is_none());
	}

	#[test]
	fn open_v3_info_reader_returns_reader() {
		let abort = FakeAbort::new(false);
		let reader = entry().open_v3(&INFO_READER, "test://ok", None, None, &abort).unwrap();
		let mut info = FakeFileInfo::new();
		reader.get_info(0, &mut info, &abort).unwrap();
		assert_eq!(info.meta("title"), vec!["T"]);
		assert!((info.length() - 1.0).abs() < f64::EPSILON);
	}

	#[test]
	fn open_v3_writer_throws_tagging_unsupported() {
		let abort = FakeAbort::new(false);
		let error = entry().open_v3(&INFO_WRITER, "test://ok", None, None, &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::TaggingUnsupported);
	}

	#[test]
	fn open_v3_unknown_guid_throws_not_implemented() {
		let abort = FakeAbort::new(false);
		let unknown = Guid::from_u128(0x12345678_0000_0000_0000_000000000000);
		let error = entry().open_v3(&unknown, "test://ok", None, None, &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::NotImplemented);
	}

	#[test]
	fn open_bad_path_throws_unsupported_format() {
		let abort = FakeAbort::new(false);
		let error = entry().open_v3(&DECODER, "test://bad", None, None, &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::IoUnsupportedFormat);
		assert_eq!(error.message, "not a test path");
	}

	#[test]
	fn open_for_decoding_slot_matches_open_v3() {
		let abort = FakeAbort::new(false);
		let decoder = entry().open_for_decoding("test://ok", &abort).unwrap();
		assert!(decoder.service().query(&DECODER_V5).is_some());
		let error = entry().open_for_decoding("test://bad", &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::IoUnsupportedFormat);
	}

	#[test]
	fn by_value_hint_and_logger_are_released() {
		let abort = FakeAbort::new(false);
		let (hint, hint_destroyed) = CountedService::new();
		let (logger, logger_destroyed) = CountedService::new();
		let decoder =
			entry().open_v3(&DECODER, "test://ok", Some(hint), Some(logger), &abort).unwrap();
		assert_eq!(hint_destroyed.get(), 1);
		assert_eq!(logger_destroyed.get(), 1);
		let (logger, logger_destroyed) = CountedService::new();
		decoder.set_logger(logger);
		assert_eq!(logger_destroyed.get(), 1);
	}

	#[test]
	fn nonzero_subsong_throws_bad_subsong_index() {
		let abort = FakeAbort::new(false);
		let decoder = entry().open_v3(&DECODER, "test://ok", None, None, &abort).unwrap();
		assert_eq!(decoder.subsong_count(), 1);
		let mut info = FakeFileInfo::new();
		let error = decoder.get_info(1, &mut info, &abort).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::IoBadSubsongIndex);
	}

	#[test]
	fn get_file_stats_returns_invalid_size() {
		let abort = FakeAbort::new(false);
		let decoder = entry().open_v3(&DECODER, "test://ok", None, None, &abort).unwrap();
		let stats = decoder.file_stats(&abort);
		assert_eq!(stats.size, u64::MAX);
		assert_eq!(stats.timestamp, 0);
	}

	#[test]
	fn run_fills_chunk_through_vtable() {
		let abort = FakeAbort::new(false);
		let decoder = entry().open_v3(&DECODER, "test://ok", None, None, &abort).unwrap();
		decoder.initialize(0, 0, &abort).unwrap();
		let mut chunk = FakeAudioChunk::new();
		assert!(decoder.run(&mut chunk, &abort).unwrap());
		assert_eq!(chunk.sample_count(), 2);
		assert_eq!(chunk.sample_rate(), 44_100);
		assert_eq!(chunk.channels(), 2);
		assert_eq!(chunk.channel_mask(), 3);
		assert_eq!(chunk.samples(), [0.1, 0.2, 0.3, 0.4]);
		assert!(!decoder.run(&mut chunk, &abort).unwrap());
	}

	#[test]
	fn run_raw_throws_not_implemented() {
		let abort = FakeAbort::new(false);
		let decoder = entry().open_v3(&DECODER, "test://ok", None, None, &abort).unwrap();
		let mut chunk = FakeAudioChunk::new();
		assert_eq!(
			decoder.run_raw(&mut chunk, &abort).unwrap_err().kind,
			ExceptionKind::NotImplemented
		);
	}

	#[test]
	fn aborted_run_throws_aborted() {
		let decoder =
			entry().open_v3(&DECODER, "test://ok", None, None, &FakeAbort::new(false)).unwrap();
		let mut chunk = FakeAudioChunk::new();
		let error = decoder.run(&mut chunk, &FakeAbort::new(true)).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Aborted);
	}

	#[test]
	fn seek_receives_seconds() {
		let abort = FakeAbort::new(false);
		let decoder = entry().open_v3(&DECODER, "test://ok", None, None, &abort).unwrap();
		assert!(decoder.can_seek());
		decoder.seek(2.5, &abort).unwrap();
		assert_eq!(SEEKED_TO.get(), Some(2.5));
	}

	fn playing() -> DecoderHandle {
		let abort = FakeAbort::new(false);
		let decoder = entry().open_v3(&DECODER, "test://ok", None, None, &abort).unwrap();
		decoder.initialize(0, 0, &abort).unwrap();
		decoder.run(&mut FakeAudioChunk::new(), &abort).unwrap();
		decoder
	}

	fn bitrate(decoder: &DecoderHandle) -> Option<String> {
		decoder.live_info()?.info("bitrate")
	}

	#[test]
	fn live_info_reaches_foobar2000_as_a_file_info_filter_once() {
		let decoder = playing();
		let list = FakeLiveInfoList::new();
		decoder.poll_live_info(&list, &FakeAbort::new(false)).unwrap();
		assert_eq!(list.scopes(), [SCOPE_INFO]);
		let mut info = FakeFileInfo::new();
		assert!(list.apply(0, &mut info).unwrap());
		assert_eq!(info.info("bitrate").as_deref(), Some("320"));
		assert_eq!(bitrate(&decoder), None);
	}

	#[test]
	fn changed_live_info_is_reported_again() {
		let decoder = playing();
		assert_eq!(bitrate(&decoder).as_deref(), Some("320"));
		decoder.run(&mut FakeAudioChunk::new(), &FakeAbort::new(false)).unwrap();
		assert_eq!(bitrate(&decoder).as_deref(), Some("256"));
		assert_eq!(bitrate(&decoder), None);
	}

	#[test]
	fn live_info_is_reported_again_after_a_seek_or_a_new_initialize() {
		let decoder = playing();
		let abort = FakeAbort::new(false);
		assert!(bitrate(&decoder).is_some());
		decoder.seek(1.0, &abort).unwrap();
		assert_eq!(bitrate(&decoder).as_deref(), Some("320"));
		assert_eq!(bitrate(&decoder), None);
		decoder.initialize(0, 0, &abort).unwrap();
		assert_eq!(bitrate(&decoder).as_deref(), Some("320"));
	}

	#[test]
	fn the_filter_outlives_the_decoder_and_goes_with_the_list() {
		let decoder = playing();
		let list = FakeLiveInfoList::new();
		decoder.poll_live_info(&list, &FakeAbort::new(false)).unwrap();
		drop(decoder);
		assert!(list.apply(0, &mut FakeFileInfo::new()).unwrap());
		assert_eq!(crate::live_info::filters_alive(), 1);
		drop(list);
		assert_eq!(crate::live_info::filters_alive(), 0);
	}

	#[test]
	fn the_old_dynamic_info_slots_report_nothing() {
		let decoder = playing();
		let mut info = FakeFileInfo::new();
		assert_eq!(decoder.dynamic_info(&mut info).unwrap(), None);
		assert_eq!(decoder.dynamic_track_info(&mut info).unwrap(), None);
	}

	#[test]
	fn meta_add_appends_second_value() {
		let mut fake = FakeFileInfo::new();
		let mut info = unsafe { FileInfo::from_raw(fake.as_raw()) };
		info.meta_add("artist", "A").unwrap();
		info.meta_add("artist", "B").unwrap();
		assert_eq!(fake.meta("artist"), vec!["A", "B"]);
	}
}
