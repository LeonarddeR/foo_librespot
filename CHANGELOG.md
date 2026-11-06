# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- An artist link adds the artist's whole discography: their albums, singles, EPs and compilations, each recording once. When Spotify has a release in more than one edition, such as explicit and clean, the first edition that plays in full is added, so an account that filters explicit content gets the clean one. Uncheck "Artist links add the whole discography" under File > Preferences > Tools > Spotify to add their popular tracks in your country instead, as the artist's Spotify page lists them.
- Tracks show more of what Spotify knows: ISRC, barcode, label, copyright and release type, the composer, conductor, orchestra and remixer where Spotify credits them, and ITUNESADVISORY for explicit tracks and episodes. Tracks added earlier get these when they're added again or their info is reloaded.
- A Liked Songs link, `https://open.spotify.com/collection/tracks` or `spotify:collection:tracks`, adds your Liked Songs.
- A search link, `https://open.spotify.com/search/<words>` or `spotify:search:<words>`, adds the 20 tracks Spotify finds first, named after the words.
- For developers: `foxide2k` dialogs can hold checkboxes (`Dialog::is_checked`, `Dialog::set_checked`).

### Changed

- Each cover is downloaded once while foobar2000 runs, instead of every time foobar2000 shows it. Up to 32 MB of covers are kept in memory.
- When your Spotify account is set not to play explicit content, adding a link skips its explicit tracks and episodes and says why. Playing one added earlier gives the same reason, instead of Spotify's technical error.
- A track link is added with its title and other info straight away, and asks for a login when you're logged out, like an album link.
- Importing a podcast skips the episodes Spotify blocks in your country or hasn't released yet, with the reason, as it already did for tracks. An episode that plays from the podcast's own website is still added.
- A Spotify link that can't be played, such as a local file or a profile page, now shows a popup saying why, instead of doing nothing.
- The popup after an import that skipped items is titled "Some Spotify items weren't added" instead of "foo_librespot", so a screen reader says what happened first.
- Pressing Play right after a big import waits for Spotify's request limit instead of failing at once.
- The Spotify page in Preferences says why the stored login can't be read as soon as it opens.
- The Spotify page and the login window use the font of Windows' own dialogs.

### Fixed

- Error messages say "Spotify:" once, instead of starting with "Spotify: not available on Spotify:" or "Spotify: Spotify:".
- A temporary error on Spotify's side no longer deletes your login.
- Logging out sticks, also when a track was starting at that moment.
- Cancelling the login at the very moment you confirm it no longer leaves you logged in.
- Tracks are no longer skipped as "not available in your country ()" when Spotify is slow to send your country.
- Reading many tracks while offline no longer waits 30 seconds for each of them.
- An album added before its release date is checked again when you add it later.
- Loading old foo_spotify playlists makes half as many Spotify requests.
- When Spotify doesn't send an album's details, its tracks get them the next time they're read, instead of going without their totals, label and copyright until foobar2000 restarts.

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
