# TODO

The open work, in the order it is planned. Entries that name foo_spotify ([TheQwertiest/foo_spotify](https://github.com/TheQwertiest/foo_spotify), version 1.1.3) are things it did and foo_librespot doesn't yet. The others are ideas from what librespot offers. They stay within librespot's own Spotify connection, without Spotify's Web API.

## Next round

The cheapest wins come first.

### Small changes

- [x] Download each album cover once
  - foo_librespot downloaded the cover every time foobar2000 asked for it (`Core::cover` in `crates/foo_librespot_core/src/spotify.rs`). An album's tracks all share the album's cover, so playing a 12-track album downloaded the same image at least 12 times.
  - Downloaded covers are now kept in memory by the image's ID, up to 32 MB (`CoverCache` in `crates/foo_librespot_core/src/art.rs`), so each cover is downloaded once while foobar2000 runs. When a new cover doesn't fit, the ones read longest ago go first.
  - foo_spotify also kept covers on disk, so they showed offline. That part is ruled out.
- [x] Say when the account's explicit-content filter blocks a track
  - foo_spotify gave a reason when a track couldn't play: not available in your country, not available for your subscription type, or explicit while the account is set not to play explicit content.
  - foo_librespot gave the country reason when it added a track, but didn't check the explicit filter. librespot checks it when the track plays, in `AudioItem::get_file`, so an import added explicit tracks without a word, and playing one failed with librespot's own message.
  - An import now skips explicit tracks and episodes with the reason "explicit, and your Spotify account is set not to play explicit content" (`explicit_allowed` in `crates/foo_librespot_core/src/metadata.rs`). Playing one added earlier gives the same reason (`audio_item_error` in `crates/foo_librespot_core/src/audio/track.rs`).
- [ ] Show the artist's picture as album art
  - foo_spotify answered foobar2000's request for an artist picture with the Spotify picture of the track's first artist.
  - foo_librespot only has a front cover. `SpotifyArt::query` in `crates/foo_librespot/src/art.rs` returns nothing for any other kind of art.
  - librespot's artist metadata has the pictures, in `Artist::portraits` and `Artist::portrait_group`.
- [x] More track info from the metadata we already fetch
  - foo_librespot fetched each track's and album's full metadata from Spotify, but wrote only the title, artists, album, numbers, date, genre and ReplayGain.
  - Tracks now also get `ISRC`, `BARCODE`, `LABEL`, `COPYRIGHT`, `RELEASETYPE`, `COMPOSER`, `CONDUCTOR`, `ORCHESTRA`, `REMIXER` and `ITUNESADVISORY` (for explicit tracks), and episodes `ITUNESADVISORY`, with no extra requests (`TrackInfo` and `AlbumDetails` in `crates/foo_librespot_core/src/metadata.rs`, `fill_info` in `crates/foo_librespot/src/fields.rs`).
  - Featured artists get no field of their own: Spotify already lists them among the track's artists.
  - `LANGUAGE` is left out. Spotify mixes two-letter language codes (`en`) with three-letter ones (`zxx`, no words), where other taggers use three letters throughout (`eng`), and its data can be wrong: a German recitative in Suzuki's Bach cantatas is marked Latin.
  - An episode's metadata doesn't carry its show's publisher (checked on 2026-11-06), so there is no `PUBLISHER`; it would take a request for the show.

### New links

- [x] Add your Liked Songs
  - Spotify's web link for Liked Songs is `https://open.spotify.com/collection/tracks`, and its apps use `spotify:collection:tracks`. foo_librespot's link parser didn't know either.
  - These links, and the older `spotify:user:<user>:collection`, now add the logged-in account's Liked Songs, from librespot's `SpClient::get_context`, following every further page (`context_tracks` in `crates/foo_librespot_core/src/resolve.rs`).
  - An artist's liked songs (`spotify:user:<user>:collection:artist:<id>`) and other collections, such as podcasts, are still unsupported.
- [x] An artist's whole discography
  - An artist link added only the artist's popular tracks.
  - It now adds the whole discography, unless "Artist links add the whole discography" is unchecked on the Spotify preferences page: the artist's albums, then singles and EPs, then compilations, newest first in each group, not the albums the artist only appears on (`releases` and `resolve` in `crates/foo_librespot_core/src/resolve.rs`).
  - A recording on both an album and a single is added once, by its ISRC: the first playable copy is kept, so a copy blocked in the account's country doesn't keep out one that plays. The console says how many were left out. There is no warning before a large discography.
- [x] Search Spotify
  - librespot's `SpClient::get_context` answers searches (`spotify:search:<words>`) with one page of 20 tracks. On 2026-11-06 the first 13 to 15 matched the words and the rest drifted; for "in christ alone" the Getty original came first.
  - Spotify's search link, `https://open.spotify.com/search/<words>`, and `spotify:search:<words>` now add those 20 tracks, named after the words. A search window would need menus.

### Detailed logging

- [x] A switch for detailed logging
  - foo_spotify had Advanced settings to log its Spotify requests, the responses and debug messages.
  - foo_librespot always logged its own messages from info level and librespot's from warning level, so a user couldn't collect a detailed log for a bug report.
  - A checkbox under Advanced > Tools > Spotify, off by default, now shows both from debug level (`crates/foo_librespot/src/logging.rs`, `crates/foo_librespot/src/advanced.rs`). It keeps the `log` crate.
  - Trace never shows, because librespot logs access tokens there. librespot's dumps of raw server messages are held back, and every URL in the console loses its query string and password, so Spotify's signed download links never show whole.

### Checks by hand

Nobody has checked these yet. Any new work they show waits for a later round.

- [ ] Find out what happens when the account plays on another device
  - When Spotify took the stream away because the account played elsewhere, foo_spotify paused playback and showed "Playback has been paused because your Spotify account is being used somewhere else."
  - foo_librespot has nothing for this, and librespot has no event for it.
  - Nobody has checked yet whether Spotify ever stops a foo_librespot stream this way. Test that first; build something only if it does.
- [ ] Check for silence between tracks
  - librespot's own player starts loading the next track 30 seconds before the current one ends. foo_librespot starts fetching a track only when foobar2000 opens it.
  - Check whether an album that should play without breaks gets a short silence between Spotify tracks. If it does, foo_librespot could start fetching the next track early.
- [ ] Check Spotify's made-for-you playlists
  - Discover Weekly and the Daily Mixes are playlists owned by Spotify.
  - Check that their links add their tracks.
- [ ] Check audiobooks
  - Spotify marks an audiobook as a show (`Show::is_audiobook`) and its chapters as episodes (`Episode::is_audiobook_chapter`).
  - Check what happens when you add an audiobook link.

## Later rounds

- [ ] Follow Windows' proxy settings
  - foo_spotify had a proxy address, user name and password in Advanced preferences, under Tools > Spotify Integration > Network. A change needed a restart.
  - foo_librespot always connects directly. librespot's `SessionConfig` has a `proxy` field, which foo_librespot leaves empty.
  - foo_librespot can't use foobar2000's own proxy setting. The SDK gives components no way to read it, and foobar2000's web client can't carry librespot's connection to Spotify, which isn't ordinary web traffic.
  - The decision: follow Windows' proxy settings, as foobar2000 does when its proxy choice is "Use system proxy settings". There is no setting of our own. A proxy typed only into foobar2000's preferences is not used.
  - Windows can name the proxy outright or through an automatic configuration script; WinHTTP's `WinHttpGetIEProxyConfigForCurrentUser` and `WinHttpGetProxyForUrl` answer both. Every connection foo_librespot makes must use it: the session, the login and the cover downloads.
- [ ] Menu commands in foxide2k
  - Several ideas below need a command in a menu: the right-click menu on playlist entries, or foobar2000's main menu. Some also need to create or change foobar2000 playlists.
  - foxide2k has no bindings for menus or for foobar2000's playlist manager yet. This comes first for the items marked "needs menus".
- [ ] Bring foxide2k further in line with the foobar2000 SDK
  - foxide2k covers only the parts of the SDK that foo_librespot and foo_oxitone use: services and the component entry point, inputs and decoders, filesystems, playlist loaders, album art, preferences pages, the console, the settings store, popups and the main thread, plus Win32 dialogs. Each new feature starts by adding the SDK interfaces it needs, as menus, Advanced preferences and checkboxes on a dialog do this round.
  - Covering more of the SDK the same way, each file transcribed with the SDK notice and tested through its vtables, would make later features cheaper: menus and the playlist manager, Advanced preferences, UI elements, metadb and the play callbacks.
  - Check the transcribed files against the newest SDK (version 2026-10-01 on 2026-11-06) for interfaces that changed or were added since.
- [ ] Radio from a track (needs menus)
  - librespot's `SpClient::get_radio_for_track` gives a Spotify playlist of tracks like a given one, as Spotify's "Go to song radio" does.
  - A right-click command on a Spotify track could add that radio.
- [ ] Add your own playlists (needs menus)
  - librespot's `SpClient::get_rootlist` lists the logged-in user's playlists, with their names.
  - A command could list them and add the chosen ones as foobar2000 playlists, each named after its Spotify playlist. Today a user has to copy each playlist's link from Spotify.
  - This browses the user's library, which was out of scope when foo_librespot started.
- [ ] Play from the Spotify app on foobar2000 (Spotify Connect)
  - The `librespot-connect` crate makes a program show up as a Spotify Connect speaker, so the Spotify app on a phone can play on it.
  - In foobar2000 this would mean letting the Spotify app drive foobar2000's playback. That is a big feature and a different way of using foobar2000; consider it last.
  - It is feasible, but awkward. librespot's Connect code (`Spirc::new`) only works with librespot's own player, which decodes the audio itself and hands it to an output. foo_librespot would supply that output and pass the audio to foobar2000 as one endless stream of whatever the Spotify app picks, with the track info changing as tracks change. The Spotify tracks wouldn't be separate playlist entries.
  - Logging in comes with it. The `librespot-discovery` crate is what makes foobar2000 show up in the Spotify app's device list on the same network. Picking foobar2000 there hands it the app's login, without a browser or a code. Windows Firewall will ask once whether foobar2000 may accept network connections.

## After 1.0

Listing foo_librespot on foobar2000.org waits for a 1.0 and broader testing.

- [ ] Tell users about new versions
  - foo_spotify told Auto Check for Component Updates (foo_acfu) where its GitHub releases are, so users heard about new versions.
  - foo_librespot doesn't, so users only learn of a new version from the releases page.
  - The decision: list foo_librespot on foobar2000.org's component pages. foobar2000 now checks for updates of listed components by itself, so foo_acfu isn't needed.

## Ruled out

The maintainer ruled these out in November 2026.

- Keeping played audio in a cache on disk, as foo_spotify did, so a replayed track starts from disk.
- Keeping covers on disk, as foo_spotify did, so they show offline.
- A README line telling former foo_spotify users to turn on foobar2000's ReplayGain, now that Spotify's own volume normalisation is gone.
- Lyrics from Spotify in a `LYRICS` field (`SpClient::get_lyrics`).
- Updating a foobar2000 playlist when its Spotify playlist changes (`Playlist::revision`, `Playlist::diff`).
- Adding similar tracks when a playlist ends, as Spotify's Autoplay does (`SpClient::get_autoplay_context`).
- A native build for Windows on ARM (ARM64EC). foobar2000 for Windows on ARM runs the x64 build through emulation, and foo_librespot spends its time waiting on the network, so a native build gains nothing noticeable. librespot can't build for ARM64EC yet either: its `sysinfo` dependency pulls in `winapi` 0.3.9, which doesn't support it.
- A version for foobar2000 for Mac. It would mean porting foxide2k's C++ interop, the preferences page, the login window and the stored login, for no gain worth that work.

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
