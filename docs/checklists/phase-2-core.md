# Phase 2 check: foo_librespot_core against a real Spotify account

This checklist proves that the Spotify side works with a real Premium account, before the foobar2000 component is built on it.
It uses the `spotify_dev` example; see [TESTING.md](../../TESTING.md).
Its login lives in `.spotify-dev`, which git ignores.

Test links, all by Keith & Kristyn Getty:

- Track "In Christ Alone": `https://open.spotify.com/track/2hQxLiI93xdjqENJuKeXjO`
- Album "In Christ Alone" (2007, 11 songs): `https://open.spotify.com/album/4wPtS24asgBAG4VOmYYB9O`
- Playlist "Give Us This Day Our Daily Hymns" by Getty Music (35 songs): `https://open.spotify.com/playlist/5cQ8xIDb1NvIsEPjHgHg55`

## Login (needs you, once)

- [x] `cargo run -p foo_librespot_core --example spotify_dev -- login` prints a page and a code.
- [x] Opening the page and confirming prints `Logged in as <your username>.`
- [x] Every later command uses the stored login without asking again.

## Resolving links

- [x] The track link prints one track: "Keith & Kristyn Getty – In Christ Alone (4:39) [In Christ Alone]".
- [x] The album link prints its 11 tracks in album order, from "There Is A Higher Throne" to "Don't Let Me Lose My Wonder".
- [x] The playlist link prints its 35 tracks in playlist order.
- A playlist with a podcast episode lists the episode as skipped, with the reason. (Dropped: episodes are added now; see [podcasts.md](podcasts.md).)
- [x] `https://open.spotify.com/intl-nl/album/<id>?si=…` works like the plain album link.

## Audio

- [x] `decode <track> 10 .spotify-dev/track.wav` prints `OGG_VORBIS_320`, the loudness values and `wrote 441000 frames`.
- [x] `ffprobe .spotify-dev/track.wav` reports 44100 Hz, 2 channels, 10.000000 s.
- The WAV sounds right: the start of the track, no noise, no clicks. (Dropped: playback in foobar2000 uses the same decoder and was checked by ear in phase 3.)
- [x] `decode <track> 10 .spotify-dev/seek.wav --seek 60` gives exactly the samples a straight decode has from 1:00: the largest difference is 0.0, against 0.39 one frame off.

## ReplayGain calibration

Three whole tracks from the album were decoded and measured with `ffmpeg -i full.wav -af ebur128 -f null -`.
ReplayGain 2 gain is `-18 - I`, where `I` is the integrated loudness in LUFS.

| Track | Spotify header gain | `I` (ffmpeg) | -18 - I | Difference | Spotify metadata `loudness_db` |
|---|---|---|---|---|---|
| In Christ Alone (`5j2qXCXCCMPBR7Gn9R2IZU`) | -7.78 | -11.4 | -6.6 | 1.18 | -11.38 |
| The Power Of The Cross (`0rOxHEaSWjiuAaHO0MCD6A`) | -7.82 | -11.6 | -6.4 | 1.42 | -11.61 |
| Speak, O Lord (`66kJLPv3KRA98W86JXkuDb`) | -7.47 | -12.3 | -5.7 | 1.77 | -12.30 |

- [x] The header difference is within 1 dB for all three, so `HEADER_OFFSET_DB` in `crates/foo_librespot_core/src/replaygain.rs` is 1.5 dB.
- [x] Spotify's extended metadata (`AUDIO_FILES`) has each track's integrated loudness and true peak, matching ffmpeg to 0.05 dB, before anything is played. That is the better source; the header values become the fallback.

## Optional: a Free account

- [ ] Logging in with a Free account prints that the account needs Premium, and the program exits normally instead of being killed.

## Result

Tested by: Claude (live runs) with Leonard's login, 2026-11-04.
foobar2000: not involved.
Outcome: passes, except listening to the WAVs and the podcast-episode playlist, which are still open.
