# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `foxide2k`: Rust bindings for writing foobar2000 components, including throwing and catching foobar2000's C++ exceptions from Rust.
- `foo_oxitone`: a test-tone component that proves `foxide2k` in a real foobar2000.
- `foo_librespot_core`: Spotify login through the browser, link parsing, album and playlist resolution, track metadata, album art, ReplayGain from Spotify's loudness metadata at import time, and sample-accurate decoding of Spotify audio.
- `foo_librespot`: the foobar2000 component. Adds `open.spotify.com` links and `spotify:` URIs as playlist entries with their metadata and ReplayGain, plays and seeks Spotify tracks, and shows their album art.
- `foxide2k`: filesystem, playlist loader, album art and `file_info` support.
- `foo_librespot`: a preferences page under Tools to log in, log out and choose the bitrate. It is built for screen readers: each control is named by its label, and the status line is announced without moving focus.
- `foo_librespot`: adding a Spotify link or playing a Spotify track while logged out opens a "Log in to Spotify" window. It closes by itself once the login is confirmed, and the tracks are then added or played. The browser opens only when you press Open browser.
- `foo_librespot`: the status bar shows the codec and bitrate of the file that plays.
- `foo_librespot`: podcast episode and show links. A show adds all its episodes, playlists keep theirs, and an episode Spotify doesn't host is added as the podcast's own audio link.
- `foo_librespot`: one popup per import lists the items that were skipped and why, and the console explains how to stop foo_youtube from taking Spotify links.
- `foxide2k`: preferences pages, dialogs built from in-memory templates with live regions, popups, foobar2000's settings store and main-thread callbacks.
- `foxide2k`: inputs report technical info that changes while a track plays, such as the bitrate, through foobar2000 2.25's live info. `foo_oxitone` uses it to show its bitrate.
