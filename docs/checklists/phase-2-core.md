# Phase 2 check: foo_librespot_core against a real Spotify account

This checklist proves that the Spotify side works with a real Premium account, before the foobar2000 component is built on it.
It uses the `spotify_dev` example; see [TESTING.md](../../TESTING.md).
Its login lives in `.spotify-dev`, which git ignores.

## Login (needs you, once)

- [ ] `cargo run -p foo_librespot_core --example spotify_dev -- login` prints a page and a code.
- [ ] Opening the page and confirming prints `Logged in as <your username>.`
- [ ] Running `login` again, or any other command, no longer asks: the stored login is used.

## Resolving links

- [ ] A track link prints one track with title, artist, album and length.
- [ ] An album link prints all of its tracks in album order, with the same count the Spotify app shows.
- [ ] A playlist link prints its tracks in playlist order, with the same count the Spotify app shows.
- [ ] A playlist with a podcast episode lists the episode as skipped, with the reason.
- [ ] `https://open.spotify.com/intl-nl/album/<id>?si=…` works like the plain album link.

## Audio

- [ ] `decode <track> 10 .spotify-dev/track.wav` prints the format (`OGG_VORBIS_320` on Premium), the loudness values and `wrote 441000 frames`.
- [ ] `ffprobe .spotify-dev/track.wav` reports 44100 Hz, 2 channels, 10.000 s.
- [ ] The WAV sounds right: the start of the track, no noise, no clicks.
- [ ] `decode <track> 10 .spotify-dev/seek.wav --seek 60` on a track longer than 70 s sounds like the track from 1:00.

## ReplayGain calibration

For three tracks, decode the whole track and measure it:

```
ffmpeg -i .spotify-dev/full.wav -af ebur128 -f null -
```

ReplayGain 2 gain is `-18 - I`, where `I` is the integrated loudness in LUFS that ffmpeg prints.
Compare it with the `track_gain_db` the decode printed.

| Track | Spotify track gain | -18 - I | Difference |
|---|---|---|---|
| … | … | … | … |

- [ ] The difference is the same within 1 dB for all three, and `REPLAYGAIN_OFFSET_DB` in `crates/foo_librespot_core/src/audio/header.rs` is set to it (rounded to 0.5 dB).

## Optional: a Free account

- [ ] Logging in with a Free account prints that the account needs Premium, and the program exits normally instead of being killed.

## Result

Tested by: …
Date: …
Outcome: …
