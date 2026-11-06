# foo_librespot

foo_librespot is a [foobar2000](https://www.foobar2000.org) component that plays Spotify tracks, albums, playlists and podcasts.
You paste or drag a Spotify link into foobar2000, and the tracks play like any other track in your playlist.

It was created as a vibe coding experiment, for two reasons: to replace foo_spotify, which stopped working when Spotify shut down libspotify, and to see whether foobar2000 would bear components written in Rust.
It is written entirely in Rust.
It uses [librespot](https://github.com/librespot-org/librespot) to talk to Spotify, and its own `foxide2k` crate to talk to foobar2000.

## Status

Early development. Releases are pre-releases for now.

- `crates/foxide2k`: the Rust layer that talks to foobar2000, named for foobar2000 and oxide, which is what rust is. It is proven in a real foobar2000 by the `foo_oxitone` test-tone component.
- `crates/foo_librespot_core`: the Spotify side, built on librespot, with no foobar2000 knowledge. It logs in, turns links into track lists with metadata, and plays tracks as seekable PCM. Its `spotify_dev` example tries it against a real account.
- `crates/foo_librespot`: the component. It adds Spotify track, album, playlist, episode, show and artist links as playlist entries (an artist link adds their popular tracks) with their metadata and ReplayGain, plays and seeks them, and shows album art. It asks for a Spotify login the first time a link needs one.

## Requirements

- foobar2000 2.25 or later, 64-bit.
- A Spotify Premium account. librespot does not work with free accounts.

## Install

Download `foo_librespot-<version>.fb2k-component` from the [releases page](https://github.com/LeonarddeR/foo_librespot/releases).
Open the file, or add it with File > Preferences > Components > Install.
foobar2000 lists it in Components; press OK and let foobar2000 restart.
The first releases are pre-releases: they work, but have had few users so far.

## Logging in

The first time you add a Spotify link or play a Spotify track, a "Log in to Spotify" window opens.
After a moment it shows a code and a web address.
Press Open browser, or open the address yourself, and confirm the code on Spotify's page.
The window then closes by itself, and the tracks are added or the track plays.
If you close the window instead, nothing is added or played, and playing doesn't ask again until you add a link or press Log in.

foo_librespot remembers the login, encrypted for your Windows account, so you only do this once.
To log in ahead of time, or to log out, open File > Preferences > Tools > Spotify; it has the same Log in button, code and web address.

The preferences page also sets the preferred bitrate: 96, 160 or 320 kbit/s.
While a track plays, the status bar shows the codec and bitrate of the file Spotify sent.

## Podcasts

Episode and show links work like track and album links: an episode adds one entry, and a show adds all its episodes. A show meant to be heard from the start, such as a Bible-in-a-year podcast, starts with its first episode; others start with the newest.
Episodes in playlists are added too.
Each episode gets its show as artist and album, its publication date, the genre Podcast, and its description as the comment.

Spotify streams many podcasts from the podcast's own server instead of hosting them.
Such an episode is added as the podcast's own audio link, which foobar2000 plays with its own decoders; the episode's title, show and date come along.

## Troubleshooting

- **open.spotify.com links turn into entries that won't play, and the console mentions foo_youtube.** foo_youtube claims every web link by default, Spotify's included. foobar2000 asks its components in an order that changes each time it starts, so in some sessions foo_youtube sees Spotify links before foo_librespot does. In foo_youtube's preferences, add the line `deny spotify.com` to its host filter. Links in the `spotify:album:…` form work either way. foo_librespot prints this advice to the console when foo_youtube is loaded.
- **"Not logged in to Spotify."** Log in from File > Preferences > Tools > Spotify.
- **The Converter or the ReplayGain scanner says "Access denied" for Spotify tracks.** foo_librespot only lets foobar2000's player decode Spotify audio, as foo_spotify did, so it is never saved to a file. ReplayGain comes from Spotify itself, so Spotify tracks need no scanning.
- **An album or playlist adds fewer tracks than it has.** Local files and tracks Spotify doesn't offer in your country are skipped. A popup lists them after the import, and the console has the full list.
- **"Spotify refused to play a track", or a track takes up to a minute to start.** Spotify refuses the keys that decrypt its tracks for a while when many are asked for in a short time, for example when you skip quickly through a playlist. Wait a few minutes before you press Play again. A track you've played before isn't affected: foo_librespot remembers its key, encrypted in foobar2000's profile folder. Logging out deletes the remembered keys.
- **Reporting a problem.** Check "Log details to the console, for bug reports" under File > Preferences > Advanced > Tools > Spotify, make the problem happen again, and copy the console (View > Console) into your report. The details show your Spotify username and country, but never your login. Uncheck it afterwards; the console stays quieter without it.

## Please note

librespot is an unofficial Spotify client.
Spotify can change its service at any time in a way that breaks or blocks it, and then this component stops working until librespot is fixed.
That's exactly why this component has librespot in its name: if librespot breaks, we're done, and ripe for the GitHub Arctic Code Vault.

## Licence

foo_librespot is released under the MIT licence; see [LICENSE](LICENSE).
The component package also carries the licences of the Rust libraries it is built on, in `licenses.html` in the component's folder.
The `foxide2k` crate contains interface definitions transcribed from the foobar2000 SDK, which stay under the SDK's licence; see [crates/foxide2k/sdk-license.txt](crates/foxide2k/sdk-license.txt).
