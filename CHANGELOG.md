# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `oxibar`: Rust bindings for writing foobar2000 components, including throwing and catching foobar2000's C++ exceptions from Rust.
- `foo_oxitone`: a test-tone component that proves `oxibar` in a real foobar2000.
- `foo_librespot_core`: Spotify login through the browser, link parsing, album and playlist resolution, track metadata, album art, ReplayGain from Spotify's loudness metadata at import time, and sample-accurate decoding of Spotify audio.
