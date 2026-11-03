# foo_librespot

foo_librespot is a [foobar2000](https://www.foobar2000.org) component that plays Spotify tracks, albums and playlists.
You paste or drag a Spotify link into foobar2000, and the tracks play like any other track in your playlist.
It replaces foo_spotify, which stopped working when Spotify shut down libspotify.

The component is written entirely in Rust.
It uses [librespot](https://github.com/librespot-org/librespot) to talk to Spotify, and its own `oxibar` crate to talk to foobar2000.

## Status

Early development. Nothing is released yet.

## Requirements

- foobar2000 2.x, 64-bit.
- A Spotify Premium account. librespot does not work with free accounts.

## Please note

librespot is an unofficial Spotify client.
Spotify can change its service at any time in a way that breaks or blocks it, and then this component stops working until librespot is fixed.

## Licence

foo_librespot is released under the MIT licence; see [LICENSE](LICENSE).
The `oxibar` crate contains interface definitions transcribed from the foobar2000 SDK, which stay under the SDK's licence; see [crates/oxibar/sdk-license.txt](crates/oxibar/sdk-license.txt).
