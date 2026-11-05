//! Spotify links as users paste them, and the playlist paths foobar2000 stores.

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
	/// An artist, with their popular tracks.
	Artist,
}

/// A parsed Spotify track, album, playlist, episode, show or artist link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
	pub kind: LinkKind,
	pub id: SpotifyId,
}

impl Link {
	/// The link as librespot names it.
	pub const fn uri(&self) -> SpotifyUri {
		let id = self.id;
		match self.kind {
			LinkKind::Track => SpotifyUri::Track { id },
			LinkKind::Album => SpotifyUri::Album { id },
			LinkKind::Playlist => SpotifyUri::Playlist { user: None, id },
			LinkKind::Episode => SpotifyUri::Episode { id },
			LinkKind::Show => SpotifyUri::Show { id },
			LinkKind::Artist => SpotifyUri::Artist { id },
		}
	}

	/// The one item a track or episode link names; `None` for links to several.
	pub const fn item(&self) -> Option<ItemId> {
		match self.kind {
			LinkKind::Track => Some(ItemId::Track(self.id)),
			LinkKind::Episode => Some(ItemId::Episode(self.id)),
			LinkKind::Album | LinkKind::Playlist | LinkKind::Show | LinkKind::Artist => None,
		}
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
		"Spotify {kind} links can't be played; use a track, album, playlist, episode, show or artist link"
	)]
	Unsupported { kind: String },
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
	// A profile, spotify:user:<name>. Old playlist URIs go on after the name.
	let mut parts = rest.split(':');
	if parts.next().is_some_and(|kind| kind.eq_ignore_ascii_case("user")) && parts.nth(1).is_none()
	{
		return Err(LinkError::Unsupported { kind: "user".to_owned() });
	}
	let uri = SpotifyUri::from_uri(&format!("spotify:{rest}")).map_err(|_| LinkError::InvalidId)?;
	match uri {
		SpotifyUri::Track { id } => Ok(Link { kind: LinkKind::Track, id }),
		SpotifyUri::Album { id } => Ok(Link { kind: LinkKind::Album, id }),
		SpotifyUri::Playlist { id, .. } => Ok(Link { kind: LinkKind::Playlist, id }),
		SpotifyUri::Episode { id } => Ok(Link { kind: LinkKind::Episode, id }),
		SpotifyUri::Show { id } => Ok(Link { kind: LinkKind::Show, id }),
		SpotifyUri::Artist { id } => Ok(Link { kind: LinkKind::Artist, id }),
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
	Ok(Link { kind, id })
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
		Link { kind, id: id(base62) }
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
		assert_eq!(parse_link(&format!("https://open.spotify.com/artist/{TRACK}")), Ok(artist));
		assert_eq!(parse_link(&format!("spotify:artist:{TRACK}")), Ok(artist));
		assert_eq!(parse_link(&format!("sptf://spotify:artist:{TRACK}")), Ok(artist));
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
			link(LinkKind::Playlist, PLAYLIST).uri().to_uri(),
			format!("spotify:playlist:{PLAYLIST}")
		);
		assert_eq!(link(LinkKind::Show, SHOW).uri().to_uri(), format!("spotify:show:{SHOW}"));
		assert_eq!(
			link(LinkKind::Episode, EPISODE).uri().to_uri(),
			format!("spotify:episode:{EPISODE}")
		);
	}

	#[test]
	fn links_to_tracks_and_episodes_name_one_item() {
		assert_eq!(link(LinkKind::Track, TRACK).item(), Some(ItemId::Track(id(TRACK))));
		assert_eq!(link(LinkKind::Episode, EPISODE).item(), Some(ItemId::Episode(id(EPISODE))));
		assert_eq!(link(LinkKind::Show, SHOW).item(), None);
		assert_eq!(link(LinkKind::Album, ALBUM).item(), None);
	}
}
