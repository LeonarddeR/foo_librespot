//! Try `foo_librespot_core` against a real Spotify account, without foobar2000.
//!
//! ```text
//! cargo run -p foo_librespot_core --example spotify_dev -- login
//! cargo run -p foo_librespot_core --example spotify_dev -- resolve <link>
//! cargo run -p foo_librespot_core --example spotify_dev -- decode <track link> <seconds> [--seek <seconds>]
//! cargo run -p foo_librespot_core --example spotify_dev -- logout
//! ```
//!
//! The login is kept in `.spotify-dev` in the current folder, or in `--profile <folder>`.
//! Set `SPOTIFY_DEV_LOG` to `debug` or `trace` for more librespot output.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;

use foo_librespot_core::audio::decoder::SAMPLE_RATE;
use foo_librespot_core::audio::format::Bitrate;
use foo_librespot_core::link::parse_link;
use foo_librespot_core::resolve::ArtistTracks;
use foo_librespot_core::{Core, CoreError};

const USAGE: &str = "usage: spotify_dev [--profile <folder>] login | logout | resolve <link> | decode <track link> <seconds> [--seek <seconds>]";

fn main() -> ExitCode {
	init_logger();
	let mut args: Vec<String> = std::env::args().skip(1).collect();
	let profile = take_option(&mut args, "--profile")
		.map_or_else(|| PathBuf::from(".spotify-dev"), PathBuf::from);
	let seek = take_option(&mut args, "--seek");
	let core = match Core::new(&profile) {
		Ok(core) => core,
		Err(error) => {
			eprintln!("can't start: {error}");
			return ExitCode::FAILURE;
		}
	};
	let args: Vec<&str> = args.iter().map(String::as_str).collect();
	let result = match args.as_slice() {
		["login"] => login(&core),
		["logout"] => core.logout().map(|()| println!("Logged out.")),
		["resolve", link] => resolve(&core, link),
		["decode", link, seconds] => decode(&core, link, seconds, seek.as_deref()),
		_ => {
			eprintln!("{USAGE}");
			core.shutdown();
			return ExitCode::FAILURE;
		}
	};
	core.shutdown();
	match result {
		Ok(()) => ExitCode::SUCCESS,
		Err(error) => {
			eprintln!("error: {error}");
			ExitCode::FAILURE
		}
	}
}

fn login(core: &Core) -> Result<(), CoreError> {
	let (done, finished) = mpsc::channel();
	let _login = core.start_login(
		|code| {
			println!("Open {} and confirm the code {}.", code.url, code.user_code);
			println!("(Or go to {} and enter the code there.)", code.verification_uri);
		},
		move |result| {
			let _ = done.send(result);
		},
	);
	let username = finished.recv().map_err(|_| CoreError::Aborted)??;
	println!("Logged in as {username}.");
	Ok(())
}

fn resolve(core: &Core, link: &str) -> Result<(), CoreError> {
	let link = parse_link(link)?;
	let resolved = core.resolve(&link, ArtistTracks::Popular, &|| false)?;
	if let Some(name) = &resolved.name {
		println!("{name}");
	}
	for (index, track) in resolved.tracks.iter().enumerate() {
		let seconds = track.duration_ms / 1000;
		println!(
			"{}. {} – {} ({}:{:02}) [{}] {}",
			index + 1,
			track.artists.join(", "),
			track.title,
			seconds / 60,
			seconds % 60,
			track.album,
			track.id.uri().to_uri()
		);
		if track.total_tracks > 0 {
			println!(
				"   track {}/{}, disc {}/{}",
				track.track_number, track.total_tracks, track.disc_number, track.total_discs
			);
		}
		if !track.genres.is_empty() {
			println!("   genre: {}", track.genres.join("; "));
		}
		if let Some(url) = &track.external_url {
			println!("   hosted elsewhere: {url}");
		}
	}
	for skipped in &resolved.skipped {
		println!("skipped {}: {}", skipped.uri, skipped.reason);
	}
	println!("{} tracks, {} skipped", resolved.tracks.len(), resolved.skipped.len());
	Ok(())
}

/// Decodes `seconds` of a track and prints, for each whole second, a fingerprint of its samples
/// and its peak. Nothing is written to a file: decrypted Spotify audio never leaves memory. A
/// decode with `--seek 60` prints the same fingerprints from 1:00 as one from the start does when
/// the seek is sample-exact.
fn decode(core: &Core, link: &str, seconds: &str, seek: Option<&str>) -> Result<(), CoreError> {
	let Some(item) = parse_link(link)?.item() else {
		return Err(CoreError::Unavailable("decode needs a track or episode link".to_owned()));
	};
	let seconds: f64 =
		seconds.parse().map_err(|_| CoreError::Unavailable(format!("not a number: {seconds}")))?;
	let info = core.track_info(item, &|| false)?;
	println!("replay gain from metadata: {:?}", info.replay_gain);
	let mut track = core.open_track(item, Bitrate::default(), &|| false)?;
	println!("format: {:?}", track.format());
	println!("total frames: {:?}", track.total_frames());
	println!("replay gain from the file header: {:?}", track.replay_gain());
	let mut position = 0;
	if let Some(seek) = seek {
		let seek: f64 =
			seek.parse().map_err(|_| CoreError::Unavailable(format!("not a number: {seek}")))?;
		position = frames(seek);
		track.seek(position)?;
	}
	let second = u64::from(SAMPLE_RATE);
	let wanted = frames(seconds);
	let mut decoded = 0u64;
	let mut buffer = Vec::new();
	let mut block = Fingerprint::default();
	let started = std::time::Instant::now();
	while decoded < wanted {
		// Never past the end of the current second, so each block covers exactly one second.
		let left_in_second = second - position % second;
		let chunk =
			usize::try_from((wanted - decoded).min(left_in_second).min(4096)).expect("small");
		let read = track.read(&mut buffer, chunk)?;
		if read == 0 {
			break;
		}
		block.add(&buffer);
		position += read as u64;
		decoded += read as u64;
		if position % second == 0 {
			println!("{}: {}", clock(position / second - 1), block.finish());
		}
	}
	if !block.is_empty() {
		println!("{} (part): {}", clock(position / second), block.finish());
	}
	println!(
		"decoded {decoded} frames ({:.3} s) in {:.2?}",
		decoded as f64 / f64::from(SAMPLE_RATE),
		started.elapsed()
	);
	Ok(())
}

/// An FNV-1a hash of a block's samples, and its peak.
#[derive(Default)]
struct Fingerprint {
	hash: Option<u64>,
	peak: f64,
}

impl Fingerprint {
	fn add(&mut self, samples: &[f64]) {
		let mut hash = self.hash.unwrap_or(0xcbf2_9ce4_8422_2325);
		for sample in samples {
			for byte in sample.to_bits().to_le_bytes() {
				hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
			}
			self.peak = self.peak.max(sample.abs());
		}
		self.hash = Some(hash);
	}

	const fn is_empty(&self) -> bool {
		self.hash.is_none()
	}

	fn finish(&mut self) -> String {
		let done = std::mem::take(self);
		format!("fingerprint {:016x}, peak {:.3}", done.hash.unwrap_or_default(), done.peak)
	}
}

fn clock(second: u64) -> String {
	format!("{}:{:02}", second / 60, second % 60)
}

#[allow(
	clippy::cast_possible_truncation,
	clippy::cast_sign_loss,
	reason = "seconds are small and positive"
)]
fn frames(seconds: f64) -> u64 {
	(seconds.max(0.0) * f64::from(SAMPLE_RATE)).round() as u64
}

fn take_option(args: &mut Vec<String>, name: &str) -> Option<String> {
	let index = args.iter().position(|arg| arg == name)?;
	args.remove(index);
	(index < args.len()).then(|| args.remove(index))
}

struct StderrLogger(log::LevelFilter);

impl log::Log for StderrLogger {
	fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
		metadata.level() <= self.0
	}

	fn log(&self, record: &log::Record<'_>) {
		if self.enabled(record.metadata()) {
			eprintln!("[{} {}] {}", record.level(), record.target(), record.args());
		}
	}

	fn flush(&self) {}
}

fn init_logger() {
	let level = std::env::var("SPOTIFY_DEV_LOG")
		.ok()
		.and_then(|level| level.parse().ok())
		.unwrap_or(log::LevelFilter::Info);
	if log::set_logger(Box::leak(Box::new(StderrLogger(level)))).is_ok() {
		log::set_max_level(level);
	}
}
