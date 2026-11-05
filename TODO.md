# TODO

## Gaps compared to foo_spotify

foo_spotify 1.1.3 ([TheQwertiest/foo_spotify](https://github.com/TheQwertiest/foo_spotify)) did the things below, and foo_librespot doesn't yet.
They are roughly in order of value to users.

- [ ] Show the artist's picture as album art
  - foo_spotify answered foobar2000's request for an artist picture with the Spotify picture of the track's first artist.
  - foo_librespot only has a front cover. `SpotifyArt::query` in `crates/foo_librespot/src/art.rs` returns nothing for any other kind of art.
  - librespot's artist metadata has the pictures, in `Artist::portraits` and `Artist::portrait_group`.
- [ ] Keep played audio in a cache on disk
  - foo_spotify kept the audio it played in a cache in the foobar2000 profile. A track played before started from disk. Preferences limited the cache by size in MB (2048 by default) and by a share of the free disk space (10% by default).
  - foo_librespot makes its librespot `Cache` without any folders (`crates/foo_librespot_core/src/session.rs`), so every play downloads the whole file again.
  - librespot's `Cache::new` takes a folder for audio files and a size limit, and `AudioFile::open` reads from it and saves to it by itself.
  - librespot stores the files as Spotify sends them, still encrypted, so a cache doesn't put decrypted audio in a file. Check this again before relying on it.
- [ ] Connect through a proxy
  - foo_spotify had a proxy address, user name and password in Advanced preferences, under Tools > Spotify Integration > Network. A change needed a restart.
  - foo_librespot has no proxy setting. librespot's `SessionConfig` has a `proxy` field, which foo_librespot leaves empty.
  - Every connection foo_librespot makes would need to use the proxy: the session, the login and the cover downloads.
- [ ] Say when the account's explicit-content filter blocks a track
  - foo_spotify gave a reason when a track couldn't play: not available in your country, not available for your subscription type, or explicit while the account is set not to play explicit content.
  - foo_librespot gives the country reason when it adds a track. It doesn't check the explicit filter then: `available_for_user` in `crates/foo_librespot_core/src/metadata.rs` knows nothing about explicit content.
  - librespot does check the filter when the track plays, in `AudioItem::get_file`. So an import adds explicit tracks without a word, and playing one fails with librespot's own message.
  - The import could skip these tracks with a plain reason, as it does for country restrictions.
- [ ] Find out what happens when the account plays on another device
  - When Spotify took the stream away because the account played elsewhere, foo_spotify paused playback and showed "Playback has been paused because your Spotify account is being used somewhere else."
  - foo_librespot has nothing for this, and librespot has no event for it.
  - Nobody has checked yet whether Spotify ever stops a foo_librespot stream this way. Test that first; build something only if it does.
- [ ] Tell users about new versions
  - foo_spotify told Auto Check for Component Updates (foo_acfu) where its GitHub releases are, so users heard about new versions.
  - foo_librespot doesn't, so users only learn of a new version from the releases page.
  - Alternatively, foobar2000's own component update check may cover it once the component is listed on foobar2000.org.
- [ ] A switch for detailed logging
  - foo_spotify had Advanced settings to log its Spotify requests, the responses and debug messages.
  - foo_librespot always logs its own messages from info level and librespot's from warning level (`crates/foo_librespot/src/logging.rs`). A user can't collect a detailed log for a bug report.
  - Detailed logging must still never show credentials: librespot logs access tokens at trace level.
- [ ] Keep covers on disk
  - foo_spotify kept album covers and artist pictures in a cache in the profile, so they showed without a network after the first time.
  - foo_librespot downloads the cover every time foobar2000 asks for it (`Spotify::cover` in `crates/foo_librespot_core/src/spotify.rs`). Without a network there is no cover.
- [ ] Tell former foo_spotify users where volume normalisation went
  - foo_spotify had Spotify's own volume normalisation, on by default.
  - foo_librespot gives each track Spotify's loudness as ReplayGain, so foobar2000's ReplayGain playback setting decides whether volume is adjusted. A user who relied on foo_spotify's default hears unadjusted volume until they set it.
  - A line in the README would do.

## Can't be done

These depend on Spotify, not on foo_librespot. Last checked in November 2026.

- Lossless audio
  - Spotify has offered lossless FLAC to Premium accounts since September 2025. The FLAC streams carry extra DRM on top of the encryption librespot can undo.
  - The librespot maintainers concluded there is "no workable way to implement this feature". Spotify told them they "cannot pursue development that circumvents their technical protections", so they locked [discussion #1578](https://github.com/librespot-org/librespot/discussions/1578) and closed [issue #1580](https://github.com/librespot-org/librespot/issues/1580) as not planned. Other projects, such as Volumio, backed off for the same reason.
  - Spotify's own Soloist player does play lossless, but it sends the sound straight to the sound card. foobar2000 never gets the audio, so Soloist can't serve as a foobar2000 input.
  - If Spotify ever offers lossless to third-party players, our side is small. Our librespot already knows the FLAC formats, and `crates/foo_librespot_core/src/audio/format.rs` already names FLAC in the status bar. The work would be a "Lossless" choice next to the bitrates, putting FLAC first among the formats we try, and turning on FLAC in the decoder.
- Free Spotify accounts
  - Spotify only gives librespot audio for Premium accounts, so foo_librespot needs Premium too. foo_spotify needed Premium as well.
- Some newer accounts can't play at all
  - [librespot issue #1649](https://github.com/librespot-org/librespot/issues/1649), "Audio key error", affects some accounts, mostly ones created from December 2024 on. On 3 November 2026 the issue was still open, and the maintainers hadn't found the cause. On those accounts, foo_librespot can't play any track.

## Done differently, no work needed

- Private mode. foo_spotify could turn on Spotify's private session, so Spotify wouldn't share what you listen to. foo_librespot downloads the audio files itself, without librespot's player or Spotify Connect, and reports no plays to Spotify. It acts as if private mode were always on. Nobody has checked whether its plays appear in Spotify's listening history.
- Fetching track info when switching playlists. foo_spotify read track info from Spotify again whenever it was needed, so it fetched a playlist's track info ahead when the playlist became active. foo_librespot stores the info in foobar2000's database when a track is added, so there is nothing to fetch ahead.
- Login. foo_spotify asked for a user name and password, plus a separate browser login for Spotify's Web API, and showed a "not authenticated" popup that opened Preferences. foo_librespot uses one device-code login and opens its login window when a link needs it. Both show the user name once logged in.
- One track at a time. libspotify could decode only one track at a time, so foo_spotify refused a second decoder. foo_librespot has no such limit.
- Stream details. foo_spotify showed the channels, sample rate and bitrate while playing. foo_librespot sets the channels and sample rate when a track is added, and shows the codec and bitrate of the file Spotify sent while it plays.
- Private and collaborative playlists. foo_spotify needed the Web API's permission to read them. foo_librespot reads playlists through the logged-in librespot session, and a private collaborative playlist imports.
