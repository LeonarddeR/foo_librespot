# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-11-04

### Added

- Paste, drag or add a Spotify track, album, playlist, episode or show link, in the `open.spotify.com` or `spotify:` form. Its tracks are added to the playlist with their title, artists, album, track and disc numbers and totals, date, genre and album art.
- Spotify tracks play and seek like any other track.
- ReplayGain from Spotify's own loudness measurements, there as soon as a track is added, so foobar2000's ReplayGain settings work without scanning.
- Podcasts. An episode link adds the episode, and a show link adds all its episodes, from the first one for shows meant to be heard in order. An episode Spotify doesn't host is added as the podcast's own audio link.
- The first link or track that needs a login opens a "Log in to Spotify" window with a code to confirm on Spotify's page. It closes by itself once you confirm, and the login is remembered, encrypted for your Windows account.
- A Spotify page under File > Preferences > Tools, to log in or out and to choose the bitrate: 96, 160 or 320 kbit/s. While a track plays, the status bar shows the codec and bitrate of the file Spotify sent.
- The login window and the preferences page work with screen readers: each control is named by its label, and login progress is announced without moving focus.
- After each import, one popup lists what couldn't be added and why. The console has the full list, and advice when foo_youtube takes Spotify links.
- Spotify audio is decoded only for playback. The Converter, the ReplayGain scanner and similar tools get "Access denied", so Spotify audio never ends up in a file.
- Released as a `.fb2k-component`, with the licence notices of everything it is built on.
- For developers: `foxide2k`, Rust bindings for writing foobar2000 components, including foobar2000's C++ exceptions.
- For developers: `foo_librespot_core`, the Spotify side, usable without foobar2000.
- For developers: `foo_oxitone`, a test-tone component that proves `foxide2k` in a real foobar2000.
