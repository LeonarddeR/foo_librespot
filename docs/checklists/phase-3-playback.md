# Phase 3 check: foo_librespot in a real foobar2000

This checklist proves that foo_librespot adds and plays Spotify links in foobar2000 2.26 x64, before the preferences page and polish of phase 4.
It uses the portable foobar2000 from `tools/fb2k-portable.ps1`, so your normal profile is not touched.

## Setup

```
pwsh tools/fb2k-portable.ps1 -Install foo_librespot
```

There is no login page yet. Log in once with the dev tool, straight into the portable profile:

```
cargo run -q -p foo_librespot_core --example spotify_dev -- --profile .fb2k-portable\profile\foo_librespot login
```

If you already logged in with the dev tool, copy `.spotify-dev\credentials.bin` and `.spotify-dev\device_id` into `.fb2k-portable\profile\foo_librespot` instead: the login is encrypted for your Windows account, so it works for both.

Test links, all by Keith & Kristyn Getty:

- Album "In Christ Alone" (11 tracks): `https://open.spotify.com/album/4wPtS24asgBAG4VOmYYB9O`
- Playlist "Give Us This Day Our Daily Hymns" (35 tracks): `spotify:playlist:5cQ8xIDb1NvIsEPjHgHg55`
- Track "In Christ Alone": `https://open.spotify.com/track/2hQxLiI93xdjqENJuKeXjO`

## Already checked without sound (2026-11-04, foobar2000 2.26.2 x64 portable)

Checked by starting the portable foobar2000 from the command line and reading its window through UI Automation.

- [x] foobar2000 loads foo_librespot (4.2 MB release DLL) with no error dialog.
- [x] `/add https://open.spotify.com/album/4wPtS24asgBAG4VOmYYB9O` adds the album's 11 tracks in album order, each shown as "Keith & Kristyn Getty - In Christ Alone 1.01 There Is A Higher Throne 4:49" and so on: album artist, album, disc and track number, title and length, before any track was played.
- [x] `/add spotify:playlist:5cQ8xIDb1NvIsEPjHgHg55` adds the playlist's 35 tracks in playlist order, ending with "The Power Of The Cross" and "Speak O Lord".

## Still to check by hand

- [x] The console (View > Console) shows `foo_librespot: loaded`, `foo_librespot: added 11 tracks from In Christ Alone` and `added 35 tracks from Give Us This Day Our Daily Hymns`, and no errors.
- [x] Playing "In Christ Alone" sounds right: clean, no noise, no clicks, correct speed. (2026-11-04: plays.)
- [x] Seeking with the seek bar continues from there, without a crash.
- [x] Next and Previous move between Spotify tracks, and the next track starts without a long gap.
- [x] Stop during playback stops at once.
- [x] foobar2000 shows the album cover: Properties > Artwork of "In Christ Alone" lists the front cover (the album picture, 636×640 JPEG, 119 KB), with back cover and artist "not present". (Checked from a window capture on 2026-11-04.)
- [x] Properties > ReplayGain of "In Christ Alone" shows track gain -6.62 dB, track peak 1.012673, album gain -8.51 dB, album peak 1.072389, without the track having been played. (Checked from a window capture on 2026-11-04.)
- [x] With ReplayGain enabled in Playback preferences, Spotify tracks play at the same loudness as your own music. (2026-11-04, Track mode with Apply gain and both preamps at 0 dB.)
- [x] Ctrl+V of an `open.spotify.com` link copied from the browser adds its tracks.
- [ ] Dragging a link from the browser onto the playlist adds its tracks. (Needs a sighted tester: dragging can't be done with a screen reader.)
- [x] After closing and reopening foobar2000, the entries are still there and play.
- [x] Closing foobar2000 from the window gives no crash and no error dialog.

## Found

- With foo_youtube installed (as in the maintainer's own profile), an `open.spotify.com` album link becomes one dead `fy+https://open.spotify.com/…` entry: foo_youtube claims the link first and refuses it ("[DRM] The requested site is known to use DRM protection"). `spotify:` URIs are not affected. To fix before release.
- The console shows `foo_librespot: loaded (Foxide2k 0.1.0)` and `foo_librespot: added 35 tracks from Keith & Kristyn Getty - Give Us This Day Our Daily Hymns`.

## Result

Tested by: …
Outcome: …

Note: `pwsh tools/fb2k-portable.ps1 -ImportSettings` copies your own keyboard shortcuts, layout and components into the portable copy, which is much easier to use with a screen reader than its fresh default profile.
