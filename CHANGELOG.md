# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- File > Spotify > Open playlist… lists your Liked Songs and the playlists in your Spotify library, including ones you follow, and opens the one you pick as a playlist tab named after it, filled as pasting its link would. If it already has a tab, that tab comes to the front.
- Right-click Spotify tracks > Spotify > Add to playlist adds them to Liked Songs or to one of your Spotify playlists you can change. Tracks already in it aren't added again. A checkmark shows the playlists that have all of them; choosing a checked one asks, then removes them.
- Right-click a Spotify track > Spotify > Open album, Open artist… or Track radio opens the track's album, its artist or Spotify's radio for it, and Open show opens a podcast episode's show. Each opens in a playlist tab named after it, such as "In Christ Alone Radio", filled as pasting its link would. If it already has a tab, that tab comes to the front. For a track with several artists, Open artist… opens a window that lists them. With several Spotify items selected, the commands are greyed out.
- File > Spotify > Playing track has the same commands for the track or episode that is playing, greyed out when nothing from Spotify plays.
- Keys can be bound to Open album, Open artist…, Track radio and Open show in File > Preferences > Keyboard Shortcuts, for the selected or the playing track. A key pressed where its command doesn't apply shows a message saying why. When nothing plays, foobar2000 doesn't call a right-click command bound for the playing track, so its key does nothing; the File > Spotify > Playing track commands, bound to keys, say why instead, for example "Play a Spotify track first."
- A checkbox under File > Preferences > Advanced > Tools > Spotify, "Log details to the console, for bug reports", shows foo_librespot's and librespot's debug messages in the console, for a bug report. It's off by default and takes effect at once. It never shows your login or other secrets, and links to Spotify's audio servers in any console line leave out their signed part.
- For developers: foxide2k can add branches and checkboxes to Advanced preferences (`foxide2k::advconfig`).
- For developers: foxide2k can add popups and commands to foobar2000's main menu (`foxide2k::mainmenu`).
- For developers: foxide2k can add right-click menu items whose submenus are built as the menu opens, with checkmarks and greyed-out items (`foxide2k::contextmenu`), and read the paths of the tracks foobar2000 passes (`foxide2k::metadb::HandleList`).
- For developers: foxide2k's `MenuNode` names are `Arc<str>`, so a menu and the command behind it share one copy.
- For developers: foxide2k can create and switch playlist tabs, keep a value with a playlist across restarts, and load links into a tab the way pasting them does (`foxide2k::playlist`).
- For developers: foxide2k can ask a yes/no question in foobar2000's own message window (`foxide2k::popup::ask`), and dialogs can hold list boxes (`ControlClass::ListBox` and the `Dialog::list_*` methods).
- For developers: foxide2k reads a track's stored fields (`metadb::Track::meta`) and the playing track (`playback::now_playing`).
- For developers: foxide2k can add right-click submenus (`contextmenu::ContextMenuGroup`), and right-click commands that can be bound to keys, including ones shown only in the keyboard shortcut list (`contextmenu::ContextMenuCommand`).
- For developers: foxide2k's main-menu commands can be hidden or greyed out each time the menu opens (`MainMenuCommand::shown`), and a main-menu command can build its entry, a command or a submenu, each time the menu opens (`mainmenu::MainMenuDynamicCommand`), and be hidden or greyed out too (`MainMenuDynamicCommand::shown`).
- For developers: foxide2k dialogs can hold a standard Windows list view with a checkbox on each row (`ControlClass::ListView`, the `Dialog::check_list_*` methods, `Dialog::is_row_checked` and `Dialog::set_row_checked`).

### Changed

- A track you've played before starts without asking Spotify for its key again, so it plays even while Spotify is refusing keys for a while. The keys, not the audio, are kept encrypted in foobar2000's profile folder, and logging out deletes them.

### Fixed

- An audiobook link adds its chapters from the first, without Spotify's 5-minute sample.

## [0.2.1] - 2026-11-06

### Added

- An artist link adds the artist's whole discography: their albums, singles, EPs and compilations, each recording once. When Spotify has a release in more than one edition, such as explicit and clean, the first edition that plays in full is added, so an account that filters explicit content gets the clean one. Uncheck "Artist links add the whole discography" under File > Preferences > Tools > Spotify to add their popular tracks in your country instead, as the artist's Spotify page lists them.
- Tracks show more of what Spotify knows: ISRC, barcode, label, copyright and release type, the composer, conductor, orchestra and remixer where Spotify credits them, and ITUNESADVISORY for explicit tracks and episodes. Tracks added earlier get these when they're added again or their info is reloaded.
- A Liked Songs link, `https://open.spotify.com/collection/tracks` or `spotify:collection:tracks`, adds your Liked Songs.
- A search link, `https://open.spotify.com/search/<words>` or `spotify:search:<words>`, adds the 20 tracks Spotify finds first, named after the words.
- For developers: `foxide2k` dialogs can hold checkboxes (`Dialog::is_checked`, `Dialog::set_checked`).
- For developers: `foxide2k::playback::stop` stops foobar2000's playback.

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

- When Spotify refuses the key that decrypts a track, playback stops and a popup says why. Before, foobar2000 went on to the next track at once, Spotify refused that key too, and so on through the playlist: a burst of requests that kept Spotify refusing. When Spotify refuses because many tracks were asked for in a short time, foo_librespot waits and asks again for about a minute before it gives up, and asks for one key at a time.
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
