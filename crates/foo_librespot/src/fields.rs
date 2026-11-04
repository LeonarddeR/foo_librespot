//! Spotify track metadata into the fields foobar2000 shows.

use foo_librespot_core::link::ItemId;
use foo_librespot_core::metadata::TrackInfo;
use foo_librespot_core::replaygain::ReplayGain;
use foxide2k::abi::file_info::ReplayGainInfo;
use foxide2k::error::Result;
use foxide2k::file_info::FileInfo;

/// Writes `track` into `info` with foobar2000's standard field names, so existing layouts,
/// sorting and ReplayGain work as for any local file.
pub fn fill_info(info: &mut FileInfo<'_>, track: &TrackInfo) -> Result<()> {
	info.meta_set("TITLE", &track.title)?;
	add_all(info, "ARTIST", &track.artists)?;
	if !track.album.is_empty() {
		info.meta_set("ALBUM", &track.album)?;
	}
	add_all(info, "ALBUM ARTIST", &track.album_artists)?;
	if track.track_number > 0 {
		info.meta_set("TRACKNUMBER", &track.track_number.to_string())?;
	}
	if track.disc_number > 0 {
		info.meta_set("DISCNUMBER", &track.disc_number.to_string())?;
	}
	if track.total_tracks > 0 {
		info.meta_set("TOTALTRACKS", &track.total_tracks.to_string())?;
	}
	if track.total_discs > 0 {
		info.meta_set("TOTALDISCS", &track.total_discs.to_string())?;
	}
	if let Some(date) = &track.date {
		info.meta_set("DATE", date)?;
	}
	if let Some(description) = &track.description {
		info.meta_set("COMMENT", description)?;
	}
	info.set_length(f64::from(track.duration_ms) / 1000.0)?;
	match track.id {
		// Spotify's tracks are 44.1 kHz stereo Vorbis; the bitrate shows once a file is chosen.
		ItemId::Track(_) => {
			info.info_set("codec", "Vorbis")?;
			info.info_set_int("samplerate", 44_100)?;
			info.info_set_int("channels", 2)?;
		}
		// An episode's format is only known when it plays, and one hosted elsewhere is whatever
		// file the podcast serves.
		ItemId::Episode(_) => info.meta_set("GENRE", "Podcast")?,
	}
	if let Some(gain) = track.replay_gain {
		info.set_replaygain(replaygain_info(gain))?;
	}
	Ok(())
}

/// One value per entry, as foobar2000 keeps multi-value fields.
fn add_all(info: &mut FileInfo<'_>, name: &str, values: &[String]) -> Result<()> {
	for value in values {
		info.meta_add(name, value)?;
	}
	Ok(())
}

pub fn replaygain_info(gain: ReplayGain) -> ReplayGainInfo {
	ReplayGainInfo {
		album_gain: gain.album_gain_db,
		track_gain: gain.track_gain_db,
		album_peak: gain.album_peak,
		track_peak: gain.track_peak,
	}
}

#[cfg(test)]
mod tests {
	use foxide2k::file_info_impl::FileInfoImpl;

	use super::fill_info;
	use crate::testing::{episode_id, episode_info, track_id, track_info};

	#[test]
	fn fill_info_writes_every_field() {
		let mut info = FileInfoImpl::new();
		fill_info(&mut info.file_info(), &track_info(track_id(), "In Christ Alone")).unwrap();
		assert_eq!(info.meta("TITLE"), ["In Christ Alone"]);
		assert_eq!(info.meta("ARTIST"), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(info.meta("ALBUM"), ["In Christ Alone"]);
		assert_eq!(info.meta("ALBUM ARTIST"), ["Keith & Kristyn Getty"]);
		assert_eq!(info.meta("TRACKNUMBER"), ["4"]);
		assert_eq!(info.meta("DISCNUMBER"), ["1"]);
		assert_eq!(info.meta("TOTALTRACKS"), ["11"]);
		assert_eq!(info.meta("TOTALDISCS"), ["1"]);
		assert_eq!(info.meta("DATE"), ["2007-01-02"]);
		assert!((info.length() - 279.0).abs() < 1e-9);
		assert_eq!(info.info("codec").as_deref(), Some("Vorbis"));
		assert_eq!(info.info("samplerate").as_deref(), Some("44100"));
		assert_eq!(info.info("channels").as_deref(), Some("2"));
		assert!(info.meta("GENRE").is_empty());
		let gain = info.replaygain();
		assert!((gain.track_gain - -6.62).abs() < 1e-4, "{gain:?}");
		assert!((gain.album_gain - -8.51).abs() < 1e-4, "{gain:?}");
		assert!((gain.track_peak - 1.0127).abs() < 1e-4, "{gain:?}");
	}

	#[test]
	fn episodes_are_podcasts_with_their_description_and_no_fixed_audio_format() {
		let mut info = FileInfoImpl::new();
		fill_info(&mut info.file_info(), &episode_info(episode_id(), "Singing the Psalms"))
			.unwrap();
		assert_eq!(info.meta("TITLE"), ["Singing the Psalms"]);
		assert_eq!(info.meta("ALBUM"), ["Sing! Podcast"]);
		assert_eq!(info.meta("GENRE"), ["Podcast"]);
		assert_eq!(info.meta("COMMENT"), ["How the Psalms shaped the songs of the church."]);
		assert_eq!(info.meta("TRACKNUMBER"), ["12"]);
		assert!((info.length() - 1800.0).abs() < 1e-9);
		assert_eq!(info.info("codec"), None);
		assert_eq!(info.info("samplerate"), None);
	}

	#[test]
	fn unknown_numbers_date_and_replaygain_are_left_out() {
		let mut track = track_info(track_id(), "In Christ Alone");
		track.track_number = 0;
		track.disc_number = 0;
		track.total_tracks = 0;
		track.total_discs = 0;
		track.date = None;
		track.replay_gain = None;
		let mut info = FileInfoImpl::new();
		fill_info(&mut info.file_info(), &track).unwrap();
		assert!(info.meta("TRACKNUMBER").is_empty());
		assert!(info.meta("DISCNUMBER").is_empty());
		assert!(info.meta("TOTALTRACKS").is_empty());
		assert!(info.meta("TOTALDISCS").is_empty());
		assert!(info.meta("DATE").is_empty());
		assert_eq!(info.replaygain(), foxide2k::file_info_impl::REPLAYGAIN_UNSET);
	}
}
