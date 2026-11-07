//! Controlling foobar2000's playback and finding the playing track. Main thread only: from
//! another thread, use [`main_thread::run`](crate::main_thread::run).

use crate::abi::playback_control::{IID, PlaybackControlVtbl};
use crate::abi::service::ServiceBase;
use crate::cxx::try_host;
use crate::error::Result;
use crate::host::host;
use crate::metadb::Track;
use crate::service::ServicePtr;

/// The track playing now, holding a reference of its own, or `None` when nothing plays. Main
/// thread only.
pub fn now_playing() -> Result<Option<Track>> {
	let control = host().require(&IID, "playback_control")?;
	let raw = control.as_raw();
	let mut out: *mut ServiceBase = std::ptr::null_mut();
	let playing = try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<PlaybackControlVtbl>();
		(vtbl.get_now_playing)(raw, &raw mut out)
	});
	// `out` holds a reference of ours once foobar2000 has set it, even if it threw after.
	let handle = unsafe { ServicePtr::from_raw_owned(out) };
	// SAFETY: `get_now_playing` gives a `metadb_handle`.
	Ok(if playing? { handle.map(|handle| unsafe { Track::from_handle(handle) }) } else { None })
}

/// Stops playback, as the Stop button does. Main thread only.
pub fn stop() -> Result<()> {
	let control = host().require(&IID, "playback_control")?;
	let raw = control.as_raw();
	try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<PlaybackControlVtbl>();
		(vtbl.stop)(raw);
	})?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::{now_playing, stop};
	use crate::testing::FakeApi;

	const PATH: &str = "sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO";

	#[test]
	fn now_playing_is_none_when_nothing_plays() {
		let api = FakeApi::new();
		api.install();
		assert!(now_playing().unwrap().is_none());
	}

	#[test]
	fn now_playing_gives_the_track_and_its_fields() {
		let api = FakeApi::new();
		api.install();
		api.set_now_playing(Some((PATH, &[("ARTIST", &["Keith Getty", "Kristyn Getty"])])));
		let track = now_playing().unwrap().expect("a track plays");
		// The track holds its own reference, so it outlives the player's.
		api.set_now_playing(None);
		assert!(now_playing().unwrap().is_none());
		assert_eq!(track.path().unwrap(), PATH);
		assert_eq!(track.meta("ARTIST").unwrap(), ["Keith Getty", "Kristyn Getty"]);
	}

	#[test]
	fn now_playing_without_host_fails() {
		assert!(now_playing().is_err());
	}

	#[test]
	fn stop_reaches_the_host_once() {
		let api = FakeApi::new();
		api.install();
		assert_eq!(api.playback_stops(), 0);
		stop().unwrap();
		assert_eq!(api.playback_stops(), 1);
	}

	#[test]
	fn stop_without_host_fails() {
		assert!(stop().is_err());
	}
}
