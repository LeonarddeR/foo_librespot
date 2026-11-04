//! Try `foo_librespot_core` against a real Spotify account, without foobar2000.
//!
//! ```text
//! cargo run -p foo_librespot_core --example spotify_dev -- login
//! cargo run -p foo_librespot_core --example spotify_dev -- resolve <link>
//! cargo run -p foo_librespot_core --example spotify_dev -- decode <track link> <seconds> <out.wav> [--seek <seconds>]
//! cargo run -p foo_librespot_core --example spotify_dev -- logout
//! ```
//!
//! The login is kept in `.spotify-dev` in the current folder, or in `--profile <folder>`.
//! Set `SPOTIFY_DEV_LOG` to `debug` or `trace` for more librespot output.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;

use foo_librespot_core::audio::decoder::{CHANNELS, SAMPLE_RATE};
use foo_librespot_core::audio::format::Bitrate;
use foo_librespot_core::link::parse_link;
use foo_librespot_core::{Core, CoreError};

const USAGE: &str = "usage: spotify_dev [--profile <folder>] login | logout | resolve <link> | decode <track link> <seconds> <out.wav> [--seek <seconds>]";

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
		["decode", link, seconds, out] => decode(&core, link, seconds, out, seek.as_deref()),
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
	let resolved = core.resolve(&link, &|| false)?;
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

fn decode(
	core: &Core,
	link: &str,
	seconds: &str,
	out: &str,
	seek: Option<&str>,
) -> Result<(), CoreError> {
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
	if let Some(seek) = seek {
		let seek: f64 =
			seek.parse().map_err(|_| CoreError::Unavailable(format!("not a number: {seek}")))?;
		track.seek(frames(seek))?;
	}
	let spec = hound::WavSpec {
		channels: u16::try_from(CHANNELS).expect("two channels"),
		sample_rate: SAMPLE_RATE,
		bits_per_sample: 32,
		sample_format: hound::SampleFormat::Float,
	};
	let mut writer = hound::WavWriter::create(out, spec).map_err(std::io::Error::other)?;
	let wanted = frames(seconds);
	let mut written = 0u64;
	let mut buffer = Vec::new();
	while written < wanted {
		let chunk = usize::try_from((wanted - written).min(4096)).expect("small");
		let read = track.read(&mut buffer, chunk)?;
		if read == 0 {
			break;
		}
		for sample in &buffer {
			#[allow(clippy::cast_possible_truncation, reason = "WAV output is 32-bit float")]
			writer.write_sample(*sample as f32).map_err(std::io::Error::other)?;
		}
		written += read as u64;
	}
	writer.finalize().map_err(std::io::Error::other)?;
	println!("wrote {written} frames ({:.3} s) to {out}", written as f64 / f64::from(SAMPLE_RATE));
	Ok(())
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
