//! The foo_oxitone input as foobar2000 drives it, through the vtables of the built DLL.

mod common;

use std::f64::consts::PI;

use foxide2k::abi::input::{DECODER, INFO_READER, INPUT_ENTRY};
use foxide2k::cxx::ExceptionKind;
use foxide2k::testing::{DecoderHandle, FakeAbort, FakeAudioChunk, FakeFileInfo, InputEntryHandle};

fn sine(hz: f64, frame: f64) -> f64 {
	0.25 * (2.0 * PI * hz * frame / 44_100.0).sin()
}

fn entry() -> InputEntryHandle {
	let (_, component) = common::load();
	InputEntryHandle::new(component.create_all(&INPUT_ENTRY).pop().expect("an input_entry service"))
}

/// The bitrate foobar2000 shows after polling the decoder's live info, if it changed.
fn live_bitrate(decoder: &DecoderHandle) -> Option<String> {
	decoder.live_info()?.info("bitrate")
}

#[test]
fn bitrate_shows_live_after_the_first_chunk_and_again_after_a_seek() {
	let abort = FakeAbort::new(false);
	let decoder = entry().open_v3(&DECODER, "oxitone://440", None, None, &abort).unwrap();
	decoder.initialize(0, 0, &abort).unwrap();
	decoder.run(&mut FakeAudioChunk::new(), &abort).unwrap();
	assert_eq!(live_bitrate(&decoder).as_deref(), Some("1411"));
	decoder.run(&mut FakeAudioChunk::new(), &abort).unwrap();
	assert_eq!(live_bitrate(&decoder), None);
	decoder.seek(5.0, &abort).unwrap();
	decoder.run(&mut FakeAudioChunk::new(), &abort).unwrap();
	assert_eq!(live_bitrate(&decoder).as_deref(), Some("1411"));
}

#[test]
fn is_our_path_only_for_oxitone() {
	let entry = entry();
	assert!(entry.is_our_path("oxitone://440"));
	assert!(
		entry.is_our_path("oxitone://abc"),
		"invalid numbers still reach open, which rejects them"
	);
	assert!(!entry.is_our_path("file://C:\\a.flac"));
	assert!(!entry.is_our_path("tone://440"));
}

#[test]
fn info_has_title_length_and_tech_info() {
	let abort = FakeAbort::new(false);
	let reader = entry().open_v3(&INFO_READER, "oxitone://440", None, None, &abort).unwrap();
	let mut info = FakeFileInfo::new();
	reader.get_info(0, &mut info, &abort).unwrap();
	assert_eq!(info.meta("title"), ["Test tone 440 Hz"]);
	assert_eq!(info.meta("artist"), ["foo_oxitone"]);
	assert!((info.length() - 10.0).abs() < f64::EPSILON);
	assert_eq!(info.info("samplerate").as_deref(), Some("44100"));
	assert_eq!(info.info("channels").as_deref(), Some("2"));
	assert_eq!(info.info("codec").as_deref(), Some("Sine"));
	assert_eq!(info.info("encoding").as_deref(), Some("lossless"));
}

#[test]
fn decodes_ten_seconds_in_chunks() {
	let abort = FakeAbort::new(false);
	let decoder = entry().open_v3(&DECODER, "oxitone://440", None, None, &abort).unwrap();
	decoder.initialize(0, 4, &abort).unwrap();
	let mut chunk = FakeAudioChunk::new();
	let mut total = 0;
	while decoder.run(&mut chunk, &abort).unwrap() {
		assert_eq!(chunk.sample_rate(), 44_100);
		assert_eq!(chunk.channels(), 2);
		assert_eq!(chunk.channel_mask(), 3);
		assert!(chunk.sample_count() <= 4_410);
		total += chunk.sample_count();
	}
	assert_eq!(total, 441_000);
}

#[test]
fn seek_then_decode_matches_formula() {
	let abort = FakeAbort::new(false);
	let decoder = entry().open_v3(&DECODER, "oxitone://440", None, None, &abort).unwrap();
	decoder.initialize(0, 4, &abort).unwrap();
	decoder.seek(5.0, &abort).unwrap();
	let mut chunk = FakeAudioChunk::new();
	assert!(decoder.run(&mut chunk, &abort).unwrap());
	assert!((chunk.samples()[0] - sine(440.0, 220_500.0)).abs() < 1e-9);
}

#[test]
fn bad_path_throws_unsupported_format_across_dll() {
	let abort = FakeAbort::new(false);
	let error = entry().open_v3(&DECODER, "oxitone://abc", None, None, &abort).unwrap_err();
	assert_eq!(error.kind, ExceptionKind::IoUnsupportedFormat);
	assert!(error.message.contains("oxitone://abc"), "{}", error.message);
}

#[test]
fn abort_during_run_throws_aborted() {
	let decoder =
		entry().open_v3(&DECODER, "oxitone://440", None, None, &FakeAbort::new(false)).unwrap();
	let mut chunk = FakeAudioChunk::new();
	let error = decoder.run(&mut chunk, &FakeAbort::new(true)).unwrap_err();
	assert_eq!(error.kind, ExceptionKind::Aborted);
}

#[test]
fn two_decoders_on_two_threads_are_independent() {
	let (_, component) = common::load();
	let service = component.create_all(&INPUT_ENTRY).pop().expect("an input_entry service");
	let threads: Vec<_> = [440_u32, 1000]
		.into_iter()
		.map(|hz| {
			let service = service.clone();
			std::thread::spawn(move || {
				let abort = FakeAbort::new(false);
				let entry = InputEntryHandle::new(service);
				let decoder = entry
					.open_v3(&DECODER, &format!("oxitone://{hz}"), None, None, &abort)
					.unwrap();
				decoder.initialize(0, 4, &abort).unwrap();
				let mut chunk = FakeAudioChunk::new();
				let mut frame = 0;
				for _ in 0..20 {
					assert!(decoder.run(&mut chunk, &abort).unwrap());
					let samples = chunk.samples();
					for (offset, pair) in samples.as_chunks::<2>().0.iter().enumerate() {
						let expected =
							sine(f64::from(hz), f64::from(u32::try_from(frame + offset).unwrap()));
						assert!(
							(pair[0] - expected).abs() < 1e-9,
							"{hz} Hz frame {}",
							frame + offset
						);
					}
					frame += chunk.sample_count();
				}
			})
		})
		.collect();
	for thread in threads {
		thread.join().unwrap();
	}
}

#[cfg(feature = "test-panic")]
#[test]
fn panicking_input_open_reaches_host_as_bug_check() {
	let abort = FakeAbort::new(false);
	let error = entry().open_v3(&DECODER, "oxitone://panic", None, None, &abort).unwrap_err();
	assert_eq!(error.kind, ExceptionKind::BugCheck);
	assert!(error.message.contains("oxitone://panic"), "{}", error.message);
}

#[test]
fn hostcheck_rethrows_host_not_found_and_logs() {
	let (api, component) = common::load();
	api.add_service(foxide2k::abi::filesystem::IID, foxide2k::testing::FakeFilesystem::service());
	let entry = InputEntryHandle::new(
		component.create_all(&INPUT_ENTRY).pop().expect("an input_entry service"),
	);
	let abort = FakeAbort::new(false);
	let error = entry.open_v3(&DECODER, "oxitone://hostcheck", None, None, &abort).unwrap_err();
	assert_eq!(error.kind, ExceptionKind::IoNotFound);
	let logged = api.console_lines().iter().any(|line| {
		line.starts_with(
			"foo_oxitone: caught .?AVexception_io_not_found@foobar2000_io@@ from foobar2000",
		)
	});
	assert!(logged, "{:?}", api.console_lines());
}
