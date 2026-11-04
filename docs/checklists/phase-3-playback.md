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

- [ ] The console (View > Console) shows `foo_librespot: loaded`, `foo_librespot: added 11 tracks from In Christ Alone` and `added 35 tracks from Give Us This Day Our Daily Hymns`, and no errors.
- [ ] Playing "In Christ Alone" sounds right: clean, no noise, no clicks, correct speed.
- [ ] Seeking with the seek bar continues from there, without a crash.
- [ ] Next and Previous move between Spotify tracks, and the next track starts without a long gap.
- [ ] Stop during playback stops at once.
- [ ] The Album Art viewer shows the album cover.
- [ ] Properties of "In Christ Alone" shows a ReplayGain track gain of about -6.62 dB and album gain of about -8.51 dB, without the track having been played.
- [ ] With ReplayGain enabled in Playback preferences, Spotify tracks play at the same loudness as your own music.
- [ ] Ctrl+V of an `open.spotify.com` link copied from the browser adds its tracks.
- [ ] Dragging a link from the browser onto the playlist adds its tracks.
- [ ] After closing and reopening foobar2000, the entries are still there and play.
- [ ] Closing foobar2000 from the window gives no crash and no error dialog.

## Result

Tested by: …
Outcome: …
