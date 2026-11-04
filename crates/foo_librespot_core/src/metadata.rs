//! Spotify track metadata, reduced to the fields foobar2000 shows.

use librespot_metadata::Track;
use librespot_metadata::image::Images;

use crate::{CoreError, FileId, SpotifyId};

/// One size of an album cover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cover {
	pub file_id: FileId,
	pub width: i32,
	pub height: i32,
}

/// What foobar2000 shows for a Spotify track.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackInfo {
	pub id: SpotifyId,
	pub title: String,
	pub artists: Vec<String>,
	pub album: String,
	pub album_artists: Vec<String>,
	pub track_number: u32,
	pub disc_number: u32,
	/// `YYYY-MM-DD`, or `YYYY` when Spotify only knows the year.
	pub date: Option<String>,
	pub duration_ms: u32,
	/// Widest first.
	pub covers: Vec<Cover>,
	/// False when Spotify lists neither an audio file nor an alternative track to play instead.
	pub has_audio: bool,
}

pub fn track_info(track: &Track) -> Result<TrackInfo, CoreError> {
	let album = &track.album;
	let covers = if album.covers.is_empty() { &album.cover_group } else { &album.covers };
	Ok(TrackInfo {
		id: SpotifyId::try_from(&track.id)?,
		title: track.name.clone(),
		artists: track.artists.iter().map(|artist| artist.name.clone()).collect(),
		album: album.name.clone(),
		album_artists: album.artists.iter().map(|artist| artist.name.clone()).collect(),
		track_number: u32::try_from(track.number).unwrap_or(0),
		disc_number: u32::try_from(track.disc_number).unwrap_or(0),
		date: format_date(album.date.year(), u8::from(album.date.month()), album.date.day()),
		duration_ms: u32::try_from(track.duration).unwrap_or(0),
		covers: sorted_covers(covers),
		has_audio: !track.files.0.is_empty() || !track.alternatives.0.is_empty(),
	})
}

/// librespot turns a missing month or day into 1, so 1 January stands for "only the year is
/// known"; a real release on 1 January loses its day, which foobar2000 users won't miss.
pub fn format_date(year: i32, month: u8, day: u8) -> Option<String> {
	match (year, month, day) {
		(..=0, _, _) => None,
		(_, 1, 1) => Some(format!("{year:04}")),
		_ => Some(format!("{year:04}-{month:02}-{day:02}")),
	}
}

pub fn largest_cover(covers: &[Cover]) -> Option<&Cover> {
	covers.iter().max_by_key(|cover| cover.width)
}

fn sorted_covers(images: &Images) -> Vec<Cover> {
	let mut covers: Vec<Cover> = images
		.iter()
		.map(|image| Cover { file_id: image.id, width: image.width, height: image.height })
		.collect();
	covers.sort_by_key(|cover| std::cmp::Reverse(cover.width));
	covers
}

#[cfg(test)]
mod tests {
	use librespot_protocol::metadata::audio_file::Format;
	use librespot_protocol::metadata::{
		Album as AlbumMessage, Artist as ArtistMessage, AudioFile as AudioFileMessage,
		Date as DateMessage, Image as ImageMessage, ImageGroup, Track as TrackMessage,
	};
	use protobuf::MessageField;

	use super::{Cover, format_date, largest_cover, track_info};
	use crate::{FileId, SpotifyId};

	fn gid(byte: u8) -> Vec<u8> {
		vec![byte; 16]
	}

	fn artist(byte: u8, name: &str) -> ArtistMessage {
		let mut artist = ArtistMessage::new();
		artist.set_gid(gid(byte));
		artist.set_name(name.to_owned());
		artist
	}

	fn image(width: i32) -> ImageMessage {
		let mut image = ImageMessage::new();
		image.set_file_id(vec![u8::try_from(width % 251).unwrap(); 20]);
		image.set_width(width);
		image.set_height(width);
		image
	}

	fn album() -> AlbumMessage {
		let mut album = AlbumMessage::new();
		album.set_gid(gid(0x20));
		album.set_name("Whenever You Need Somebody".to_owned());
		album.artist = vec![artist(0x30, "Rick Astley"), artist(0x31, "Various")];
		let mut date = DateMessage::new();
		date.set_year(1987);
		date.set_month(11);
		date.set_day(12);
		album.date = MessageField::some(date);
		album.cover_group = MessageField::some(ImageGroup {
			image: vec![image(64), image(640), image(300)],
			..Default::default()
		});
		album
	}

	fn track() -> TrackMessage {
		let mut track = TrackMessage::new();
		track.set_gid(gid(0x10));
		track.set_name("Never Gonna Give You Up".to_owned());
		track.album = MessageField::some(album());
		track.artist = vec![artist(0x30, "Rick Astley"), artist(0x32, "Guest")];
		track.set_number(3);
		track.set_disc_number(2);
		track.set_duration(215_000);
		let mut file = AudioFileMessage::new();
		file.set_file_id(vec![0xab; 20]);
		file.set_format(Format::OGG_VORBIS_320);
		track.file = vec![file];
		track
	}

	fn parse(message: &TrackMessage) -> librespot_metadata::Track {
		librespot_metadata::Track::try_from(message).unwrap()
	}

	#[test]
	fn maps_title_artists_album_and_numbers() {
		let info = track_info(&parse(&track())).unwrap();
		assert_eq!(info.id, SpotifyId::from_raw(&gid(0x10)).unwrap());
		assert_eq!(info.title, "Never Gonna Give You Up");
		assert_eq!(info.artists, ["Rick Astley", "Guest"]);
		assert_eq!(info.album, "Whenever You Need Somebody");
		assert_eq!(info.track_number, 3);
		assert_eq!(info.disc_number, 2);
		assert_eq!(info.duration_ms, 215_000);
		assert_eq!(info.date.as_deref(), Some("1987-11-12"));
	}

	#[test]
	fn album_artists_come_from_the_album() {
		let info = track_info(&parse(&track())).unwrap();
		assert_eq!(info.album_artists, ["Rick Astley", "Various"]);
	}

	#[test]
	fn date_formats() {
		assert_eq!(format_date(2019, 5, 3).as_deref(), Some("2019-05-03"));
		assert_eq!(format_date(1973, 1, 1).as_deref(), Some("1973"));
		assert_eq!(format_date(0, 1, 1), None);
	}

	#[test]
	fn covers_are_sorted_widest_first() {
		let info = track_info(&parse(&track())).unwrap();
		let widths: Vec<i32> = info.covers.iter().map(|cover| cover.width).collect();
		assert_eq!(widths, [640, 300, 64]);
		assert_eq!(largest_cover(&info.covers).map(|cover| cover.width), Some(640));
		assert_eq!(
			largest_cover(&info.covers).map(|cover| cover.file_id),
			Some(FileId::from_raw(&[u8::try_from(640 % 251).unwrap(); 20]))
		);
		assert_eq!(largest_cover(&[] as &[Cover]), None);
	}

	#[test]
	fn track_without_files_or_alternatives_has_no_audio() {
		assert!(track_info(&parse(&track())).unwrap().has_audio);

		let mut silent = track();
		silent.file.clear();
		assert!(!track_info(&parse(&silent)).unwrap().has_audio);

		let mut alternative = TrackMessage::new();
		alternative.set_gid(gid(0x11));
		silent.alternative = vec![alternative];
		assert!(track_info(&parse(&silent)).unwrap().has_audio);
	}

	#[test]
	fn out_of_range_numbers_become_zero() {
		let mut odd = track();
		odd.set_number(-1);
		odd.set_disc_number(-5);
		odd.set_duration(-10);
		let info = track_info(&parse(&odd)).unwrap();
		assert_eq!((info.track_number, info.disc_number, info.duration_ms), (0, 0, 0));
	}
}
