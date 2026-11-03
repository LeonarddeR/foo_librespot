# Phase 1 gate: OxiBar in a real foobar2000

This checklist proves that a foobar2000 component written entirely in Rust works in the real player before any Spotify work starts.
If any item fails, stop: record what happened and the console output here, and don't start phase 2.

## Setup

```
pwsh tools/fb2k-portable.ps1 -Setup
pwsh tools/fb2k-portable.ps1 -Install foo_oxitone
pwsh tools/fb2k-portable.ps1 -Run /add oxitone://440
```

The portable foobar2000 lives in `.fb2k-portable`, which git ignores; your normal foobar2000 profile is not touched.
Open the console with View > Console.

## Already checked without sound (2026-11-03, foobar2000 2.26.2 x64 portable)

These were checked by starting the portable foobar2000 from the command line and reading its window through UI Automation.
No audio was played.

- [x] foobar2000 loads foo_oxitone with no error dialog.
- [x] The console shows `foo_oxitone: loaded (OxiBar 0.1.0)`.
- [x] `oxitone://440` appears in the playlist as "foo_oxitone - Test tone 440 Hz", length 0:10.
- [x] Adding `oxitone://abc` adds an entry without info, and foobar2000 keeps running: it caught the `exception_io_unsupported_format` that foo_oxitone threw.
- [x] Adding `oxitone://hostcheck` logs `foo_oxitone: caught .?AVexception_io_not_found@foobar2000_io@@ from foobar2000: Object not found`, and foobar2000 keeps running. That exception was thrown by foobar2000's own filesystem, caught by OxiBar's pure-Rust catcher and thrown back.
- [x] `foobar2000.exe /exit` closes foobar2000 cleanly, with no crash report in the profile.

Expected noise: each `oxitone://` path also logs "Error walking directory (Invalid path)", because foobar2000 first tries an unknown scheme as a folder.
No filesystem claims `oxitone://`; foo_librespot will register one for `sptf://`.

## Still to check by hand

- [ ] Preferences > Components lists foo_oxitone 0.1.0, and its About text mentions `oxitone://`.
- [ ] Playing "Test tone 440 Hz" gives a steady, clean tone for 10 seconds, then playback moves on or stops.
- [ ] Clicking the seek bar at about 0:05 continues the tone from there, without clicks or a crash.
- [ ] Pressing Stop during playback stops it without an error or a crash.
- [ ] Playing `oxitone://abc` reports that the file can't be played, and foobar2000 keeps running.
- [ ] Playing `oxitone://hostcheck` logs the `foo_oxitone: caught .?AVexception_io_not_found@foobar2000_io@@ from foobar2000: …` line again, reports the error for that entry, and keeps running.
- [ ] Closing foobar2000 from the window gives no crash and no error dialog.

## Result

Tested by: …
foobar2000 version: …
Outcome: …
