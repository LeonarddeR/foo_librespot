//! Spotify links as users paste them, and the playlist paths foobar2000 stores.

use std::fmt::Write as _;

use crate::{SpotifyId, SpotifyUri};

/// The scheme of the paths foo_librespot stores in playlists, the same one foo_spotify used,
/// so existing foo_spotify playlists play again.
pub const PATH_SCHEME: &str = "sptf://";

/// The kinds of Spotify link foo_librespot can play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
	Track,
	Album,
	Playlist,
	/// A podcast episode.
	Episode,
	/// A podcast, with all its episodes.
	Show,
	/// An artist, with their popular tracks or their whole discography.
	Artist,
}

/// How foo_librespot stores a Liked Songs link: the URI Spotify's apps use for Liked Songs.
pub const LIKED_SONGS_URI: &str = "spotify:collection:tracks";

/// A parsed Spotify link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
	/// A track, album, playlist, episode, show or artist, named by its Spotify ID.
	Id { kind: LinkKind, id: SpotifyId },
	/// The logged-in account's Liked Songs.
	LikedSongs,
	/// A search for these words, decoded and with single spaces.
	Search(String),
}

impl Link {
	/// The link as a `spotify:` URI, the form foo_librespot stores it in.
	pub fn uri(&self) -> String {
		match self {
			Self::Id { kind, id } => id_uri(*kind, *id).to_uri(),
			Self::LikedSongs => LIKED_SONGS_URI.to_owned(),
			Self::Search(words) => format!("spotify:search:{}", encode_search(words)),
		}
	}

	/// The one item a track or episode link names; `None` for links to several.
	pub const fn item(&self) -> Option<ItemId> {
		match self {
			Self::Id { kind: LinkKind::Track, id } => Some(ItemId::Track(*id)),
			Self::Id { kind: LinkKind::Episode, id } => Some(ItemId::Episode(*id)),
			Self::Id { .. } | Self::LikedSongs | Self::Search(_) => None,
		}
	}
}

/// A link by ID as librespot names it.
const fn id_uri(kind: LinkKind, id: SpotifyId) -> SpotifyUri {
	match kind {
		LinkKind::Track => SpotifyUri::Track { id },
		LinkKind::Album => SpotifyUri::Album { id },
		LinkKind::Playlist => SpotifyUri::Playlist { user: None, id },
		LinkKind::Episode => SpotifyUri::Episode { id },
		LinkKind::Show => SpotifyUri::Show { id },
		LinkKind::Artist => SpotifyUri::Artist { id },
	}
}

/// Something that plays: a track or a podcast episode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemId {
	Track(SpotifyId),
	Episode(SpotifyId),
}

impl ItemId {
	/// The id Spotify knows the item by, within its kind.
	pub const fn id(self) -> SpotifyId {
		match self {
			Self::Track(id) | Self::Episode(id) => id,
		}
	}

	pub const fn uri(self) -> SpotifyUri {
		match self {
			Self::Track(id) => SpotifyUri::Track { id },
			Self::Episode(id) => SpotifyUri::Episode { id },
		}
	}
}

/// Why a string is not a playable Spotify link.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
	#[error("this is not a Spotify link")]
	NotSpotify,
	#[error(
		"Spotify {kind} links can't be played; use a track, album, playlist, episode, show, artist, Liked Songs or search link"
	)]
	Unsupported { kind: String },
	#[error("this Spotify search link has no words")]
	NoSearchWords,
	#[error("this Spotify link has an invalid id")]
	InvalidId,
}

/// Parses an `open.spotify.com` URL, a `spotify:` URI or a foo_spotify `sptf://` path.
pub fn parse_link(input: &str) -> Result<Link, LinkError> {
	let input = input.trim();
	if let Some(uri) = strip_prefix_ignore_case(input, PATH_SCHEME) {
		return parse_uri(uri);
	}
	if strip_prefix_ignore_case(input, "spotify:").is_some() {
		return parse_uri(input);
	}
	parse_url(input)
}

/// The path foobar2000 stores for a Spotify track or episode.
pub fn item_path(item: ItemId) -> String {
	format!("{PATH_SCHEME}{}", item.uri().to_uri())
}

/// The item a path made by [`item_path`], or a bare `spotify:` URI, names; `None` for anything
/// else, web links included.
pub fn parse_item(text: &str) -> Option<ItemId> {
	let text = text.trim();
	parse_uri(strip_prefix_ignore_case(text, PATH_SCHEME).unwrap_or(text)).ok()?.item()
}

fn parse_uri(uri: &str) -> Result<Link, LinkError> {
	// librespot only accepts the lowercase scheme.
	let rest = strip_prefix_ignore_case(uri, "spotify:").ok_or(LinkError::NotSpotify)?;
	let mut parts = rest.split(':');
	let first = parts.next().unwrap_or_default();
	if first.eq_ignore_ascii_case("collection") {
		return liked_songs(parts.next(), parts.next());
	}
	if first.eq_ignore_ascii_case("search") {
		return search(&decode_search(rest.split_once(':').map_or("", |(_, words)| words), true));
	}
	if first.eq_ignore_ascii_case("user") {
		// A profile is spotify:user:<name>. Old playlist URIs go on after the name, and
		// spotify:user:<name>:collection is that account's Liked Songs.
		match parts.nth(1) {
			None => return Err(LinkError::Unsupported { kind: "user".to_owned() }),
			Some(kind) if kind.eq_ignore_ascii_case("collection") => {
				return liked_songs(Some("tracks"), parts.next());
			}
			Some(_) => {}
		}
	}
	let uri = SpotifyUri::from_uri(&format!("spotify:{rest}")).map_err(|_| LinkError::InvalidId)?;
	match uri {
		SpotifyUri::Track { id } => Ok(Link::Id { kind: LinkKind::Track, id }),
		SpotifyUri::Album { id } => Ok(Link::Id { kind: LinkKind::Album, id }),
		SpotifyUri::Playlist { id, .. } => Ok(Link::Id { kind: LinkKind::Playlist, id }),
		SpotifyUri::Episode { id } => Ok(Link::Id { kind: LinkKind::Episode, id }),
		SpotifyUri::Show { id } => Ok(Link::Id { kind: LinkKind::Show, id }),
		SpotifyUri::Artist { id } => Ok(Link::Id { kind: LinkKind::Artist, id }),
		SpotifyUri::Local { .. } => Err(LinkError::Unsupported { kind: "local".to_owned() }),
		other @ SpotifyUri::Unknown { .. } => {
			Err(LinkError::Unsupported { kind: other.item_type().to_owned() })
		}
	}
}

fn parse_url(url: &str) -> Result<Link, LinkError> {
	let rest = strip_prefix_ignore_case(url, "https://")
		.or_else(|| strip_prefix_ignore_case(url, "http://"))
		.unwrap_or(url);
	let rest = rest.split(['?', '#']).next().unwrap_or_default();
	let mut parts = rest.split('/').filter(|part| !part.is_empty()).peekable();
	let host = parts.next().ok_or(LinkError::NotSpotify)?;
	if !host.eq_ignore_ascii_case("open.spotify.com")
		&& !host.eq_ignore_ascii_case("play.spotify.com")
	{
		return Err(LinkError::NotSpotify);
	}
	if parts.peek().is_some_and(|part| strip_prefix_ignore_case(part, "intl-").is_some()) {
		parts.next();
	}
	if parts.peek().is_some_and(|part| part.eq_ignore_ascii_case("user")) {
		parts.next();
		parts.next();
		// Old playlist links go on after the user name; a profile page ends there.
		if parts.peek().is_none() {
			return Err(LinkError::Unsupported { kind: "user".to_owned() });
		}
	}
	let kind = parts.next().ok_or(LinkError::NotSpotify)?.to_ascii_lowercase();
	if kind == "collection" {
		return liked_songs(parts.next(), parts.next());
	}
	if kind == "search" {
		return search(&decode_search(parts.next().unwrap_or_default(), false));
	}
	let kind = match kind.as_str() {
		"track" => LinkKind::Track,
		"album" => LinkKind::Album,
		"playlist" => LinkKind::Playlist,
		"episode" => LinkKind::Episode,
		"show" => LinkKind::Show,
		"artist" => LinkKind::Artist,
		_ => return Err(LinkError::Unsupported { kind }),
	};
	let id = parts.next().ok_or(LinkError::InvalidId)?;
	let id = SpotifyId::from_base62(id).map_err(|_| LinkError::InvalidId)?;
	Ok(Link::Id { kind, id })
}

/// Liked Songs for a collection link whose part after `collection` is `what` and whose part after
/// that is `rest`; other collections, such as podcasts, are unsupported.
fn liked_songs(what: Option<&str>, rest: Option<&str>) -> Result<Link, LinkError> {
	if what.is_some_and(|what| what.eq_ignore_ascii_case("tracks")) && rest.is_none() {
		Ok(Link::LikedSongs)
	} else {
		Err(LinkError::Unsupported { kind: "collection".to_owned() })
	}
}

fn search(words: &str) -> Result<Link, LinkError> {
	if words.is_empty() {
		Err(LinkError::NoSearchWords)
	} else {
		Ok(Link::Search(words.to_owned()))
	}
}

/// Search words as a link carries them: `%XX` decoded, and `+` too when `plus_is_space`, then
/// trimmed to single spaces.
fn decode_search(text: &str, plus_is_space: bool) -> String {
	let bytes = text.as_bytes();
	let mut decoded = Vec::with_capacity(bytes.len());
	let mut index = 0;
	while index < bytes.len() {
		let byte = bytes[index];
		if byte == b'+' && plus_is_space {
			decoded.push(b' ');
		} else if byte == b'%'
			&& let Some(value) =
				text.get(index + 1..index + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok())
		{
			decoded.push(value);
			index += 2;
		} else {
			decoded.push(byte);
		}
		index += 1;
	}
	String::from_utf8_lossy(&decoded).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Search words as a `spotify:` URI carries them: `+` for a space, `%XX` for anything but
/// letters, digits and `-_.~`.
fn encode_search(words: &str) -> String {
	let mut encoded = String::with_capacity(words.len());
	for byte in words.bytes() {
		match byte {
			b' ' => encoded.push('+'),
			b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
				encoded.push(char::from(byte));
			}
			_ => {
				let _ = write!(encoded, "%{byte:02X}");
			}
		}
	}
	encoded
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
	let head = text.get(..prefix.len())?;
	head.eq_ignore_ascii_case(prefix).then(|| &text[prefix.len()..])
}

#[cfg(test)]
mod tests {
	use super::{ItemId, Link, LinkError, LinkKind, item_path, parse_item, parse_link};
	use crate::SpotifyId;

	const TRACK: &str = "4uLU6hMCjMI75M1A2tKUQC";
	const ALBUM: &str = "4LH4d3cOWNNsVw41Gqt2kv";
	const PLAYLIST: &str = "37i9dQZF1DXcBWIGoYBM5M";
	const EPISODE: &str = "4QomYTPgZ1mpVbUmxBsUpz";
	const SHOW: &str = "5CfCWKI5pZ28U0uOzXkDHe";

	fn id(base62: &str) -> SpotifyId {
		SpotifyId::from_base62(base62).unwrap()
	}

	fn link(kind: LinkKind, base62: &str) -> Link {
		Link::Id { kind, id: id(base62) }
	}

	#[test]
	fn parses_track_url() {
		let parsed = parse_link(&format!("https://open.spotify.com/track/{TRACK}?si=abc"));
		assert_eq!(parsed, Ok(link(LinkKind::Track, TRACK)));
	}

	#[test]
	fn parses_album_and_playlist_urls() {
		assert_eq!(
			parse_link(&format!("https://open.spotify.com/album/{ALBUM}")),
			Ok(link(LinkKind::Album, ALBUM))
		);
		assert_eq!(
			parse_link(&format!("https://open.spotify.com/playlist/{PLAYLIST}")),
			Ok(link(LinkKind::Playlist, PLAYLIST))
		);
	}

	#[test]
	fn parses_episode_and_show_urls() {
		assert_eq!(
			parse_link(&format!(
				"https://open.spotify.com/episode/{EPISODE}?si=e9wYxjRdSRGHEGfuaT3DZg&utm_source=whatsapp"
			)),
			Ok(link(LinkKind::Episode, EPISODE))
		);
		assert_eq!(
			parse_link(&format!("https://open.spotify.com/show/{SHOW}")),
			Ok(link(LinkKind::Show, SHOW))
		);
	}

	#[test]
	fn parses_intl_and_legacy_user_urls() {
		assert_eq!(
			parse_link(&format!("https://open.spotify.com/intl-nl/album/{ALBUM}")),
			Ok(link(LinkKind::Album, ALBUM))
		);
		assert_eq!(
			parse_link(&format!("https://open.spotify.com/user/spotify/playlist/{PLAYLIST}")),
			Ok(link(LinkKind::Playlist, PLAYLIST))
		);
	}

	#[test]
	fn parses_uris() {
		assert_eq!(parse_link(&format!("spotify:track:{TRACK}")), Ok(link(LinkKind::Track, TRACK)));
		assert_eq!(parse_link(&format!("spotify:album:{ALBUM}")), Ok(link(LinkKind::Album, ALBUM)));
		assert_eq!(
			parse_link(&format!("spotify:playlist:{PLAYLIST}")),
			Ok(link(LinkKind::Playlist, PLAYLIST))
		);
		assert_eq!(
			parse_link(&format!("spotify:user:someone:playlist:{PLAYLIST}")),
			Ok(link(LinkKind::Playlist, PLAYLIST))
		);
		assert_eq!(
			parse_link(&format!("spotify:episode:{EPISODE}")),
			Ok(link(LinkKind::Episode, EPISODE))
		);
		assert_eq!(parse_link(&format!("spotify:show:{SHOW}")), Ok(link(LinkKind::Show, SHOW)));
	}

	#[test]
	fn parses_foo_spotify_path() {
		assert_eq!(
			parse_link(&format!("sptf://spotify:track:{TRACK}")),
			Ok(link(LinkKind::Track, TRACK))
		);
	}

	#[test]
	fn accepts_whitespace_trailing_slash_fragment_and_missing_scheme() {
		let expected = Ok(link(LinkKind::Track, TRACK));
		assert_eq!(
			parse_link(&format!("  https://open.spotify.com/track/{TRACK}/#x \n")),
			expected
		);
		assert_eq!(parse_link(&format!("open.spotify.com/track/{TRACK}")), expected);
		assert_eq!(parse_link(&format!("http://play.spotify.com/track/{TRACK}")), expected);
	}

	#[test]
	fn scheme_and_host_are_case_insensitive_and_id_is_kept_exactly() {
		assert_eq!(
			parse_link(&format!("HTTPS://Open.Spotify.COM/track/{TRACK}")),
			Ok(link(LinkKind::Track, TRACK))
		);
	}

	#[test]
	fn parses_artist_links_in_every_spelling() {
		let artist = link(LinkKind::Artist, TRACK);
		assert_eq!(
			parse_link(&format!("https://open.spotify.com/artist/{TRACK}")),
			Ok(artist.clone())
		);
		assert_eq!(parse_link(&format!("spotify:artist:{TRACK}")), Ok(artist.clone()));
		assert_eq!(parse_link(&format!("sptf://spotify:artist:{TRACK}")), Ok(artist.clone()));
		// An artist names no single item to play.
		assert_eq!(artist.item(), None);
	}

	#[test]
	fn rejects_unsupported_kinds() {
		let unsupported = |kind: &str| Err(LinkError::Unsupported { kind: kind.to_owned() });
		assert_eq!(
			parse_link(&format!("https://open.spotify.com/genre/{TRACK}")),
			unsupported("genre")
		);
		assert_eq!(parse_link("spotify:local:a:b:c:180"), unsupported("local"));
		// A user's profile page; old playlist links also start with /user/<name>/.
		assert_eq!(parse_link("https://open.spotify.com/user/someone"), unsupported("user"));
		// The same profile as a URI, as Spotify's "Copy Spotify URI" gives it.
		assert_eq!(parse_link("spotify:user:someone"), unsupported("user"));
		assert_eq!(parse_link("sptf://spotify:user:someone"), unsupported("user"));
	}

	#[test]
	fn rejects_non_spotify_and_bad_ids() {
		assert_eq!(parse_link(""), Err(LinkError::NotSpotify));
		assert_eq!(
			parse_link(&format!("https://example.com/track/{TRACK}")),
			Err(LinkError::NotSpotify)
		);
		assert_eq!(parse_link("https://open.spotify.com/track/short"), Err(LinkError::InvalidId));
		assert_eq!(parse_link("spotify:track:"), Err(LinkError::InvalidId));
		assert_eq!(parse_link("spotify:track:4uLU6hMCjMI75M1A2tKU-C"), Err(LinkError::InvalidId));
	}

	#[test]
	fn item_paths_round_trip_for_tracks_and_episodes() {
		let track = ItemId::Track(id(TRACK));
		let episode = ItemId::Episode(id(EPISODE));
		assert_eq!(item_path(track), format!("sptf://spotify:track:{TRACK}"));
		assert_eq!(item_path(episode), format!("sptf://spotify:episode:{EPISODE}"));
		assert_eq!(parse_item(&item_path(track)), Some(track));
		assert_eq!(parse_item(&item_path(episode)), Some(episode));
		assert_eq!(parse_item(&format!("SPTF://spotify:track:{TRACK}")), Some(track));
		assert_eq!(parse_item(&format!("sptf://spotify:album:{ALBUM}")), None);
		assert_eq!(parse_item(&format!("sptf://spotify:show:{SHOW}")), None);
	}

	#[test]
	fn items_are_named_by_paths_and_uris_but_not_web_links() {
		assert_eq!(parse_item(&format!("spotify:track:{TRACK}")), Some(ItemId::Track(id(TRACK))));
		assert_eq!(
			parse_item(&format!(" spotify:episode:{EPISODE}")),
			Some(ItemId::Episode(id(EPISODE)))
		);
		assert_eq!(parse_item(&format!("https://open.spotify.com/track/{TRACK}")), None);
		assert_eq!(parse_item(&format!("spotify:album:{ALBUM}")), None);
	}

	#[test]
	fn every_link_has_its_spotify_uri() {
		assert_eq!(
			link(LinkKind::Playlist, PLAYLIST).uri(),
			format!("spotify:playlist:{PLAYLIST}")
		);
		assert_eq!(link(LinkKind::Show, SHOW).uri(), format!("spotify:show:{SHOW}"));
		assert_eq!(link(LinkKind::Episode, EPISODE).uri(), format!("spotify:episode:{EPISODE}"));
	}

	#[test]
	fn links_to_tracks_and_episodes_name_one_item() {
		assert_eq!(link(LinkKind::Track, TRACK).item(), Some(ItemId::Track(id(TRACK))));
		assert_eq!(link(LinkKind::Episode, EPISODE).item(), Some(ItemId::Episode(id(EPISODE))));
		assert_eq!(link(LinkKind::Show, SHOW).item(), None);
		assert_eq!(link(LinkKind::Album, ALBUM).item(), None);
	}

	const USER: &str = "21e2w4ryeu3jowufvdh5ctyii";

	#[test]
	fn parses_liked_songs_links() {
		for text in [
			"https://open.spotify.com/collection/tracks".to_owned(),
			"spotify:collection:tracks".to_owned(),
			"sptf://spotify:collection:tracks".to_owned(),
			format!("spotify:user:{USER}:collection"),
		] {
			assert_eq!(parse_link(&text), Ok(Link::LikedSongs), "{text}");
		}
	}

	#[test]
	fn other_collections_are_unsupported() {
		let unsupported = Err(LinkError::Unsupported { kind: "collection".to_owned() });
		assert_eq!(parse_link("https://open.spotify.com/collection/podcasts"), unsupported);
		assert_eq!(parse_link("spotify:collection:your-episodes"), unsupported);
		assert_eq!(
			parse_link(&format!("spotify:user:{USER}:collection:artist:0I4Bk2s2BUJyykCwtxx8Xx")),
			unsupported
		);
	}

	#[test]
	fn liked_songs_are_stored_as_the_collection_uri() {
		assert_eq!(Link::LikedSongs.uri(), "spotify:collection:tracks");
		assert_eq!(parse_link(&Link::LikedSongs.uri()), Ok(Link::LikedSongs));
		assert_eq!(Link::LikedSongs.item(), None);
	}

	#[test]
	fn parses_search_links() {
		let search = |words: &str| Ok(Link::Search(words.to_owned()));
		assert_eq!(
			parse_link("https://open.spotify.com/search/in%20christ%20alone"),
			search("in christ alone")
		);
		assert_eq!(
			parse_link("https://open.spotify.com/search/in%20christ%20alone/tracks"),
			search("in christ alone")
		);
		assert_eq!(
			parse_link("spotify:search:bach+cantatas+suzuki"),
			search("bach cantatas suzuki")
		);
		assert_eq!(
			parse_link("sptf://spotify:search:Bach%20Kantaten%20Suzuki"),
			search("Bach Kantaten Suzuki")
		);
		assert_eq!(parse_link("https://open.spotify.com/search/H%C3%A4ndel"), search("Händel"));
		assert_eq!(parse_link("https://open.spotify.com/search/Händel"), search("Händel"));
		assert_eq!(parse_link("spotify:search:Dvo%C5%99%C3%A1k+Rusalka"), search("Dvořák Rusalka"));
	}

	#[test]
	fn a_search_without_words_is_refused() {
		assert_eq!(
			parse_link("https://open.spotify.com/search/%20"),
			Err(LinkError::NoSearchWords)
		);
		assert_eq!(parse_link("spotify:search:"), Err(LinkError::NoSearchWords));
	}

	#[test]
	fn search_words_round_trip_through_their_uri() {
		for words in ["in christ alone", "Jesu, meine Freude", "c++ & ümlaut", "100% Getty"] {
			let link = Link::Search(words.to_owned());
			assert_eq!(parse_link(&link.uri()), Ok(link.clone()), "{words}");
		}
		assert_eq!(
			Link::Search("in christ alone".to_owned()).uri(),
			"spotify:search:in+christ+alone"
		);
	}
}
