# foo_librespot

foo_librespot is a [foobar2000](https://www.foobar2000.org) component that plays Spotify tracks, albums, playlists and podcasts.
You paste or drag a Spotify link into foobar2000, and the tracks play like any other track in your playlist.
It replaces foo_spotify, which stopped working when Spotify shut down libspotify.

The component is written entirely in Rust.
It uses [librespot](https://github.com/librespot-org/librespot) to talk to Spotify, and its own `foxide2k` crate to talk to foobar2000.

## Status

Early development. Nothing is released yet.

- `crates/foxide2k`: the Rust layer that talks to foobar2000, named for foobar2000 and oxide, which is what rust is. It is proven in a real foobar2000 by the `foo_oxitone` test-tone component.
- `crates/foo_librespot_core`: the Spotify side, built on librespot, with no foobar2000 knowledge. It logs in, turns links into track lists with metadata, and plays tracks as seekable PCM. Its `spotify_dev` example tries it against a real account.
- `crates/foo_librespot`: the component. It adds Spotify track, album, playlist, episode and show links as playlist entries with their metadata and ReplayGain, plays and seeks them, and shows album art. You log in from its preferences page.

## Requirements

- foobar2000 2.25 or later, 64-bit.
- A Spotify Premium account. librespot does not work with free accounts.

## Logging in

Open File > Preferences > Tools > foo_librespot and press Log in.
The page then shows a code and a web address.
Press Open browser, or open the address yourself, and confirm the code on Spotify's page.
The page says "Logged in to Spotify" once that is done.
Nothing opens your browser unless you press Open browser.

If you add a Spotify link or play a Spotify track before logging in, a "Log in to Spotify" window opens with the same controls.
Once you confirm the code, the window closes and the tracks are added or the track plays.
If you close the window instead, nothing is added or played, and playing doesn't ask again until you add a link or press Log in.

The same page sets the preferred bitrate: 96, 160 or 320 kbit/s.
While a track plays, the status bar shows the codec and bitrate of the file Spotify sent.

## Podcasts

Episode and show links work like track and album links: an episode adds one entry, and a show adds all its episodes. A show meant to be heard from the start, such as a Bible-in-a-year podcast, starts with its first episode; others start with the newest.
Episodes in playlists are added too.
Each episode gets its show as artist and album, its publication date, the genre Podcast, and its description as the comment.

Spotify streams many podcasts from the podcast's own server instead of hosting them.
Such an episode is added as the podcast's own audio link, which foobar2000 plays with its own decoders; the episode's title, show and date come along.

## Troubleshooting

- **open.spotify.com links turn into entries that won't play, and the console mentions foo_youtube.** foo_youtube claims every web link by default, Spotify's included. foobar2000 asks its components in an order that changes each time it starts, so in some sessions foo_youtube sees Spotify links before foo_librespot does. In foo_youtube's preferences, add the line `deny spotify.com` to its host filter. Links in the `spotify:album:…` form work either way. foo_librespot prints this advice to the console when foo_youtube is loaded.
- **"Not logged in to Spotify."** Log in from File > Preferences > Tools > foo_librespot.
- **An album or playlist adds fewer tracks than it has.** Local files and tracks Spotify doesn't offer in your country are skipped. A popup lists them after the import, and the console has the full list.

## Please note

librespot is an unofficial Spotify client.
Spotify can change its service at any time in a way that breaks or blocks it, and then this component stops working until librespot is fixed.

## Licence

foo_librespot is released under the MIT licence; see [LICENSE](LICENSE).
The `foxide2k` crate contains interface definitions transcribed from the foobar2000 SDK, which stay under the SDK's licence; see [crates/foxide2k/sdk-license.txt](crates/foxide2k/sdk-license.txt).
