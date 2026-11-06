//! Spotify track and episode metadata, reduced to the fields foobar2000 shows.

use librespot_core::date::Date;
use librespot_metadata::availability::Availabilities;
use librespot_metadata::image::Images;
use librespot_metadata::restriction::Restrictions;
use librespot_metadata::{Album, Episode, Track};

use crate::link::ItemId;
use crate::replaygain::ReplayGain;
use crate::{CoreError, FileId, SpotifyId};

/// One size of an album cover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cover {
	pub file_id: FileId,
	pub width: i32,
	pub height: i32,
}

/// What foobar2000 shows for a Spotify track or podcast episode.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackInfo {
	pub id: ItemId,
	pub title: String,
	pub artists: Vec<String>,
	pub album: String,
	pub album_artists: Vec<String>,
	pub track_number: u32,
	pub disc_number: u32,
	/// How many tracks the track's disc has; 0 until its album is read, and for episodes.
	pub total_tracks: u32,
	/// How many discs the track's album has; 0 until its album is read, and for episodes.
	pub total_discs: u32,
	/// `YYYY-MM-DD`, or `YYYY` when Spotify only knows the year.
	pub date: Option<String>,
	pub duration_ms: u32,
	/// Widest first.
	pub covers: Vec<Cover>,
	/// False when Spotify lists neither an audio file nor an alternative track to play instead.
	pub has_audio: bool,
	/// Why Spotify won't play this track for this account, such as a region restriction; `None`
	/// when it can, as far as the metadata shows. Set by whoever knows the account's country.
	pub unavailable: Option<String>,
	/// ReplayGain from Spotify's loudness metadata, known before the track is ever played; `None`
	/// when Spotify has none. Set by whoever fetched the metadata.
	pub replay_gain: Option<ReplayGain>,
	/// The genres Spotify tags a track with, surest first; empty for episodes. Set by whoever
	/// fetched the metadata.
	pub genres: Vec<String>,
	/// What an episode is about; `None` for tracks.
	pub description: Option<String>,
	/// The podcast's own audio link, for an episode Spotify doesn't host.
	pub external_url: Option<String>,
}

pub fn track_info(track: &Track) -> Result<TrackInfo, CoreError> {
	let album = &track.album;
	let covers = if album.covers.is_empty() { &album.cover_group } else { &album.covers };
	Ok(TrackInfo {
		id: ItemId::Track(SpotifyId::try_from(&track.id)?),
		title: track.name.clone(),
		artists: track.artists.iter().map(|artist| artist.name.clone()).collect(),
		album: album.name.clone(),
		album_artists: album.artists.iter().map(|artist| artist.name.clone()).collect(),
		track_number: u32::try_from(track.number).unwrap_or(0),
		disc_number: u32::try_from(track.disc_number).unwrap_or(0),
		total_tracks: 0,
		total_discs: 0,
		date: format_date(album.date.year(), u8::from(album.date.month()), album.date.day()),
		duration_ms: u32::try_from(track.duration).unwrap_or(0),
		covers: sorted_covers(covers),
		has_audio: !track.files.0.is_empty() || !track.alternatives.0.is_empty(),
		unavailable: None,
		replay_gain: None,
		genres: Vec::new(),
		description: None,
		external_url: None,
	})
}

/// What foobar2000 shows for a podcast episode: the show stands in for both the artist and the
/// album, as podcast apps show it.
pub fn episode_info(episode: &Episode) -> Result<TrackInfo, CoreError> {
	let show: Vec<String> =
		Some(episode.show_name.clone()).filter(|name| !name.is_empty()).into_iter().collect();
	let has_audio = !episode.audio.0.is_empty();
	let date = &episode.publish_time;
	Ok(TrackInfo {
		id: ItemId::Episode(SpotifyId::try_from(&episode.id)?),
		title: episode.name.clone(),
		artists: show.clone(),
		album: episode.show_name.clone(),
		album_artists: show,
		track_number: u32::try_from(episode.number).unwrap_or(0),
		disc_number: 0,
		total_tracks: 0,
		total_discs: 0,
		date: format_date(date.year(), u8::from(date.month()), date.day()),
		duration_ms: u32::try_from(episode.duration).unwrap_or(0),
		covers: sorted_covers(&episode.covers),
		has_audio,
		unavailable: None,
		replay_gain: None,
		genres: Vec::new(),
		description: Some(episode.description.clone()).filter(|text| !text.is_empty()),
		external_url: Some(episode.external_url.clone())
			.filter(|url| !has_audio && !url.is_empty()),
	})
}

/// How an album is split into discs, for the totals of its tracks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumTotals {
	/// Each disc's number and how many tracks it has.
	discs: Vec<(i32, u32)>,
}

impl AlbumTotals {
	pub fn of(album: &Album) -> Self {
		let discs = album
			.discs
			.iter()
			.map(|disc| (disc.number, u32::try_from(disc.tracks.len()).unwrap_or(u32::MAX)))
			.collect();
		Self { discs }
	}

	/// Sets `info`'s totals: the tracks on its disc and the discs on its album.
	pub fn apply(&self, info: &mut TrackInfo) {
		info.total_discs = u32::try_from(self.discs.len()).unwrap_or(u32::MAX);
		info.total_tracks = self
			.discs
			.iter()
			.find(|(number, _)| i64::from(*number) == i64::from(info.disc_number))
			.map_or(0, |(_, tracks)| *tracks);
	}
}

const NOT_RELEASED: &str = "not released on Spotify yet";

/// Why an explicit item doesn't play for an account set not to play explicit content.
pub(crate) const EXPLICIT_FILTERED: &str =
	"explicit, and your Spotify account is set not to play explicit content";

/// Whether the account's explicit-content filter lets an item through. librespot refuses the same
/// items when they play, in `AudioItem::get_file`.
pub fn explicit_allowed(is_explicit: bool, filter_explicit: bool) -> Result<(), String> {
	if is_explicit && filter_explicit { Err(EXPLICIT_FILTERED.to_owned()) } else { Ok(()) }
}

/// Whether Spotify plays `track` for an account in `country` with the given `catalogue`
/// attribute (usually "premium"). The same rules as librespot-metadata's private
/// `available_for_user` in `audio/item.rs`, so a playlist import can skip what would only fail
/// when played.
pub fn availability(
	track: &Track,
	country: &str,
	catalogue: &str,
	now: &Date,
) -> Result<(), String> {
	if *now < track.earliest_live_timestamp {
		return Err(NOT_RELEASED.to_owned());
	}
	available_for_user(&track.availability, &track.restrictions, country, catalogue, now)
}

/// [`availability`] for a podcast episode Spotify hosts. librespot checks the same windows and
/// restrictions when it plays one; episodes have no earliest release time.
pub fn episode_availability(
	episode: &Episode,
	country: &str,
	catalogue: &str,
	now: &Date,
) -> Result<(), String> {
	available_for_user(&episode.availability, &episode.restrictions, country, catalogue, now)
}

fn available_for_user(
	windows: &Availabilities,
	restrictions: &Restrictions,
	country: &str,
	catalogue: &str,
	now: &Date,
) -> Result<(), String> {
	if !windows.0.is_empty() && !windows.0.iter().any(|window| *now >= window.start) {
		return Err(NOT_RELEASED.to_owned());
	}
	// Spotify hasn't said yet which country the account is in. Let the item through: playing
	// checks again, by then with the country, and a skip here would last until the next login.
	if country.is_empty() {
		return Ok(());
	}
	let not_here = || Err(format!("not available in your country ({country})"));
	let applies = |restriction: &&librespot_metadata::restriction::Restriction| {
		restriction.catalogue_strs.iter().any(|restricted| restricted == catalogue)
	};
	// A restriction has either an allow list or a block list; the first one that applies decides.
	for restriction in restrictions.0.iter().filter(applies) {
		if let Some(allowed) = &restriction.countries_allowed {
			return if allowed.iter().any(|code| code == country) { Ok(()) } else { not_here() };
		}
		if let Some(forbidden) = &restriction.countries_forbidden {
			return if forbidden.iter().any(|code| code == country) { not_here() } else { Ok(()) };
		}
	}
	Ok(())
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
	use librespot_metadata::Episode;
	use librespot_protocol::metadata::audio_file::Format;
	use librespot_protocol::metadata::{
		Album as AlbumMessage, Artist as ArtistMessage, AudioFile as AudioFileMessage,
		Date as DateMessage, Disc as DiscMessage, Episode as EpisodeMessage, Image as ImageMessage,
		ImageGroup, Show as ShowMessage, Track as TrackMessage,
	};
	use protobuf::MessageField;

	use librespot_core::date::Date;
	use librespot_protocol::metadata::{
		Availability as AvailabilityMessage, Restriction as RestrictionMessage,
	};

	use super::{
		AlbumTotals, Cover, EXPLICIT_FILTERED, availability, episode_availability, episode_info,
		explicit_allowed, format_date, largest_cover, track_info,
	};
	use crate::link::ItemId;
	use crate::{FileId, SpotifyId};

	fn episode_message() -> EpisodeMessage {
		let mut episode = EpisodeMessage::new();
		episode.set_gid(gid(50));
		episode.set_name("Singing the Psalms".to_owned());
		episode.set_duration(1_800_000);
		episode.set_description("How the Psalms shaped the songs of the church.".to_owned());
		episode.set_number(12);
		let mut date = DateMessage::new();
		date.set_year(2026);
		date.set_month(10);
		date.set_day(3);
		episode.publish_time = MessageField::some(date);
		let mut show = ShowMessage::new();
		show.set_name("Sing! Podcast".to_owned());
		episode.show = MessageField::some(show);
		let mut cover = ImageGroup::new();
		cover.image = vec![image(300), image(640)];
		episode.cover_image = MessageField::some(cover);
		let mut file = AudioFileMessage::new();
		file.set_file_id(vec![7; 20]);
		file.set_format(Format::OGG_VORBIS_96);
		episode.audio = vec![file];
		episode
	}

	#[test]
	fn episode_info_puts_the_show_in_artist_and_album_with_date_number_and_description() {
		let info = episode_info(&Episode::try_from(&episode_message()).unwrap()).unwrap();
		assert_eq!(info.id, ItemId::Episode(SpotifyId::from_raw(&gid(50)).unwrap()));
		assert_eq!(info.title, "Singing the Psalms");
		assert_eq!(info.artists, ["Sing! Podcast"]);
		assert_eq!(info.album, "Sing! Podcast");
		assert_eq!(info.album_artists, ["Sing! Podcast"]);
		assert_eq!(info.track_number, 12);
		assert_eq!(info.date.as_deref(), Some("2026-10-03"));
		assert_eq!(info.duration_ms, 1_800_000);
		assert_eq!(
			info.description.as_deref(),
			Some("How the Psalms shaped the songs of the church.")
		);
		assert_eq!(info.covers[0].width, 640);
		assert!(info.has_audio);
		assert_eq!(info.external_url, None);
	}

	#[test]
	fn an_episode_hosted_elsewhere_keeps_its_own_link() {
		let mut message = episode_message();
		message.audio.clear();
		message.set_external_url("https://podcasts.example/12.mp3".to_owned());
		let info = episode_info(&Episode::try_from(&message).unwrap()).unwrap();
		assert!(!info.has_audio);
		assert_eq!(info.external_url.as_deref(), Some("https://podcasts.example/12.mp3"));
	}

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

	fn disc(number: i32, tracks: u8) -> DiscMessage {
		let mut disc = DiscMessage::new();
		disc.set_number(number);
		disc.track = (0..tracks)
			.map(|index| {
				let mut track = TrackMessage::new();
				track.set_gid(gid(0x40 + index));
				track
			})
			.collect();
		disc
	}

	#[test]
	fn album_totals_count_the_tracks_on_each_disc() {
		let mut message = album();
		message.disc = vec![disc(1, 3), disc(2, 2)];
		let totals = AlbumTotals::of(&librespot_metadata::Album::try_from(&message).unwrap());
		let mut info = track_info(&parse(&track())).unwrap();
		info.disc_number = 2;
		totals.apply(&mut info);
		assert_eq!((info.total_tracks, info.total_discs), (2, 2));
		info.disc_number = 1;
		totals.apply(&mut info);
		assert_eq!((info.total_tracks, info.total_discs), (3, 2));
	}

	#[test]
	fn a_track_knows_no_totals_until_its_album_is_read() {
		let info = track_info(&parse(&track())).unwrap();
		assert_eq!((info.total_tracks, info.total_discs), (0, 0));
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
		assert_eq!(info.id, ItemId::Track(SpotifyId::from_raw(&gid(0x10)).unwrap()));
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

	fn restricted_episode(allowed: &str) -> Episode {
		let mut restriction = RestrictionMessage::new();
		restriction.catalogue_str = vec!["premium".to_owned()];
		restriction.set_countries_allowed(allowed.to_owned());
		let mut message = episode_message();
		message.restriction = vec![restriction];
		Episode::try_from(&message).unwrap()
	}

	#[test]
	fn episode_availability_follows_its_restrictions() {
		let episode = restricted_episode("US");
		assert_eq!(episode_availability(&episode, "US", "premium", &now()), Ok(()));
		let result = episode_availability(&episode, "NL", "premium", &now());
		assert!(matches!(&result, Err(reason) if reason.contains("country")), "{result:?}");
	}

	#[test]
	fn unreleased_episode_is_unavailable() {
		let mut message = episode_message();
		let mut start = DateMessage::new();
		start.set_year(2999);
		let mut window = AvailabilityMessage::new();
		window.catalogue_str = vec!["premium".to_owned()];
		window.start = MessageField::some(start);
		message.availability = vec![window];
		let episode = Episode::try_from(&message).unwrap();
		let result = episode_availability(&episode, "NL", "premium", &now());
		assert!(matches!(&result, Err(reason) if reason.contains("released")), "{result:?}");
	}

	fn restricted(allowed: Option<&str>, forbidden: Option<&str>, catalogue: &str) -> TrackMessage {
		let mut restriction = RestrictionMessage::new();
		restriction.catalogue_str = vec![catalogue.to_owned()];
		if let Some(allowed) = allowed {
			restriction.set_countries_allowed(allowed.to_owned());
		}
		if let Some(forbidden) = forbidden {
			restriction.set_countries_forbidden(forbidden.to_owned());
		}
		let mut message = track();
		message.restriction = vec![restriction];
		message
	}

	fn now() -> Date {
		Date::from_timestamp_ms(1_790_000_000_000).unwrap()
	}

	#[test]
	fn unrestricted_track_is_available() {
		assert_eq!(availability(&parse(&track()), "NL", "premium", &now()), Ok(()));
	}

	#[test]
	fn allow_list_decides_by_country() {
		let track = parse(&restricted(Some("NLBE"), None, "premium"));
		assert_eq!(availability(&track, "BE", "premium", &now()), Ok(()));
		let result = availability(&track, "US", "premium", &now());
		assert!(matches!(&result, Err(reason) if reason.contains("country")), "{result:?}");
	}

	#[test]
	fn block_list_decides_by_country() {
		let track = parse(&restricted(None, Some("USCA"), "premium"));
		assert_eq!(availability(&track, "NL", "premium", &now()), Ok(()));
		assert!(availability(&track, "CA", "premium", &now()).is_err());
	}

	#[test]
	fn unknown_country_rules_nothing_out_by_country() {
		// Spotify sends the country in its own packet; until it arrives the country is empty.
		let allowed = parse(&restricted(Some("NLBE"), None, "premium"));
		assert_eq!(availability(&allowed, "", "premium", &now()), Ok(()));
		let blocked = parse(&restricted(None, Some("USCA"), "premium"));
		assert_eq!(availability(&blocked, "", "premium", &now()), Ok(()));
	}

	#[test]
	fn restrictions_for_other_catalogues_are_ignored() {
		let track = parse(&restricted(Some("NL"), None, "free"));
		assert_eq!(availability(&track, "US", "premium", &now()), Ok(()));
	}

	#[test]
	fn unreleased_track_is_unavailable() {
		let mut message = track();
		message.set_earliest_live_timestamp(1_900_000_000_000);
		let result = availability(&parse(&message), "NL", "premium", &now());
		assert!(matches!(&result, Err(reason) if reason.contains("released")), "{result:?}");

		let mut message = track();
		let mut start = DateMessage::new();
		start.set_year(2999);
		let mut window = AvailabilityMessage::new();
		window.catalogue_str = vec!["premium".to_owned()];
		window.start = MessageField::some(start);
		message.availability = vec![window];
		assert!(availability(&parse(&message), "NL", "premium", &now()).is_err());
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

	#[test]
	fn an_explicit_item_is_blocked_when_the_filter_is_on() {
		assert_eq!(explicit_allowed(true, true), Err(EXPLICIT_FILTERED.to_owned()));
	}

	#[test]
	fn an_explicit_item_plays_when_the_filter_is_off() {
		assert_eq!(explicit_allowed(true, false), Ok(()));
	}

	#[test]
	fn a_clean_item_plays_when_the_filter_is_on() {
		assert_eq!(explicit_allowed(false, true), Ok(()));
	}
}
