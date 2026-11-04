//! Reading and seeking a Spotify track on a thread of its own, so Stop never waits for a stalled
//! download.
//!
//! librespot's reads block until the data arrives, for 8 seconds or more when the network
//! stalls, and nothing can interrupt them. foobar2000's decode thread therefore only waits for
//! the decoder thread, in short steps, checking the abort flag in between.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use foo_librespot_core::CoreError;
use foo_librespot_core::audio::format::FileFormat;

use crate::backend::PcmTrack;

/// How often a waiting call checks whether foobar2000 wants it to stop.
const ABORT_POLL_INTERVAL: Duration = Duration::from_millis(50);

enum Request {
	Read { max_frames: usize, buffer: Vec<f64> },
	Seek(u64),
}

enum Reply {
	Read(Result<(usize, Vec<f64>), CoreError>),
	Seek(Result<(), CoreError>),
}

/// A [`PcmTrack`] whose reads and seeks can be abandoned.
pub struct AbortableTrack {
	requests: Sender<Request>,
	replies: Receiver<Reply>,
	/// Set once a call was abandoned: its reply may still arrive, so later replies can't be
	/// matched to their requests. foobar2000 drops a decoder it has aborted anyway.
	abandoned: bool,
	file_format: FileFormat,
}

impl AbortableTrack {
	/// Moves `track` to a new decoder thread. The thread ends, dropping the track and stopping
	/// its download, once this value is dropped and any call in progress has returned.
	pub fn new(mut track: Box<dyn PcmTrack>) -> Result<Self, CoreError> {
		let file_format = track.file_format();
		let (requests, incoming) = mpsc::channel::<Request>();
		let (outgoing, replies) = mpsc::channel::<Reply>();
		std::thread::Builder::new().name("foo_librespot decoder".to_owned()).spawn(move || {
			for request in incoming {
				let reply = match request {
					Request::Read { max_frames, mut buffer } => Reply::Read(
						track.read(&mut buffer, max_frames).map(|frames| (frames, buffer)),
					),
					Request::Seek(frame) => Reply::Seek(track.seek(frame)),
				};
				if outgoing.send(reply).is_err() {
					break;
				}
			}
		})?;
		Ok(Self { requests, replies, abandoned: false, file_format })
	}

	/// See [`PcmTrack::file_format`].
	pub const fn file_format(&self) -> FileFormat {
		self.file_format
	}

	/// See [`PcmTrack::read`].
	pub fn read(
		&mut self,
		out: &mut Vec<f64>,
		max_frames: usize,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<usize, CoreError> {
		let buffer = std::mem::take(out);
		match self.call(Request::Read { max_frames, buffer }, is_aborting)? {
			Reply::Read(result) => {
				let (frames, buffer) = result?;
				*out = buffer;
				Ok(frames)
			}
			Reply::Seek(_) => Err(out_of_step()),
		}
	}

	/// See [`PcmTrack::seek`].
	pub fn seek(&mut self, frame: u64, is_aborting: &dyn Fn() -> bool) -> Result<(), CoreError> {
		match self.call(Request::Seek(frame), is_aborting)? {
			Reply::Seek(result) => result,
			Reply::Read(_) => Err(out_of_step()),
		}
	}

	fn call(
		&mut self,
		request: Request,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Reply, CoreError> {
		if self.abandoned || is_aborting() {
			return Err(CoreError::Aborted);
		}
		self.requests.send(request).map_err(|_| stopped())?;
		loop {
			match self.replies.recv_timeout(ABORT_POLL_INTERVAL) {
				Ok(reply) => return Ok(reply),
				Err(RecvTimeoutError::Timeout) if is_aborting() => {
					self.abandoned = true;
					return Err(CoreError::Aborted);
				}
				Err(RecvTimeoutError::Timeout) => {}
				Err(RecvTimeoutError::Disconnected) => return Err(stopped()),
			}
		}
	}
}

fn stopped() -> CoreError {
	CoreError::Decode("the decoder thread stopped".to_owned())
}

fn out_of_step() -> CoreError {
	CoreError::Decode("the decoder thread answered another request".to_owned())
}
