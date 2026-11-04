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
}

/// A parsed Spotify track, album or playlist link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
	pub kind: LinkKind,
	pub id: SpotifyId,
}

/// Why a string is not a playable Spotify link.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
	#[error("this is not a Spotify link")]
	NotSpotify,
	#[error("Spotify {kind} links can't be played; use a track, album or playlist link")]
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

/// The path foobar2000 stores for a Spotify track.
pub fn track_path(id: SpotifyId) -> String {
	format!("{PATH_SCHEME}spotify:track:{}", id.to_base62())
}

/// The track id in a path made by [`track_path`]; `None` for anything else.
pub fn parse_track_path(path: &str) -> Option<SpotifyId> {
	let uri = strip_prefix_ignore_case(path.trim(), PATH_SCHEME)?;
	match parse_uri(uri) {
		Ok(Link { kind: LinkKind::Track, id }) => Some(id),
		_ => None,
	}
}

fn parse_uri(uri: &str) -> Result<Link, LinkError> {
	// librespot only accepts the lowercase scheme.
	let rest = strip_prefix_ignore_case(uri, "spotify:").ok_or(LinkError::NotSpotify)?;
	let uri = SpotifyUri::from_uri(&format!("spotify:{rest}")).map_err(|_| LinkError::InvalidId)?;
	match uri {
		SpotifyUri::Track { id } => Ok(Link { kind: LinkKind::Track, id }),
		SpotifyUri::Album { id } => Ok(Link { kind: LinkKind::Album, id }),
		SpotifyUri::Playlist { id, .. } => Ok(Link { kind: LinkKind::Playlist, id }),
		SpotifyUri::Local { .. } => Err(LinkError::Unsupported { kind: "local".to_owned() }),
		other => Err(LinkError::Unsupported { kind: other.item_type().to_owned() }),
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
	}
	let kind = parts.next().ok_or(LinkError::NotSpotify)?.to_ascii_lowercase();
	let kind = match kind.as_str() {
		"track" => LinkKind::Track,
		"album" => LinkKind::Album,
		"playlist" => LinkKind::Playlist,
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
	use super::{Link, LinkError, LinkKind, parse_link, parse_track_path, track_path};
	use crate::SpotifyId;

	const TRACK: &str = "4uLU6hMCjMI75M1A2tKUQC";
	const ALBUM: &str = "4LH4d3cOWNNsVw41Gqt2kv";
	const PLAYLIST: &str = "37i9dQZF1DXcBWIGoYBM5M";

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
	fn rejects_unsupported_kinds() {
		let unsupported = |kind: &str| Err(LinkError::Unsupported { kind: kind.to_owned() });
		assert_eq!(
			parse_link(&format!("https://open.spotify.com/artist/{TRACK}")),
			unsupported("artist")
		);
		assert_eq!(
			parse_link(&format!("https://open.spotify.com/episode/{TRACK}")),
			unsupported("episode")
		);
		assert_eq!(
			parse_link(&format!("https://open.spotify.com/show/{TRACK}")),
			unsupported("show")
		);
		assert_eq!(parse_link("spotify:local:a:b:c:180"), unsupported("local"));
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
	fn track_path_round_trips() {
		let path = track_path(id(TRACK));
		assert_eq!(path, format!("sptf://spotify:track:{TRACK}"));
		assert_eq!(parse_track_path(&path), Some(id(TRACK)));
		assert_eq!(parse_track_path(&format!("SPTF://spotify:track:{TRACK}")), Some(id(TRACK)));
		assert_eq!(parse_track_path(&format!("sptf://spotify:album:{ALBUM}")), None);
		assert_eq!(parse_track_path(&format!("https://open.spotify.com/track/{TRACK}")), None);
	}
}
