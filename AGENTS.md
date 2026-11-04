# Agent notes

foo_librespot is a foobar2000 2.25+ x64 component, written entirely in Rust, that plays Spotify tracks, albums, playlists and podcasts through librespot.

## Commands

| Task | Command |
|---|---|
| Build | `cargo build --workspace --all-features` |
| Test | `cargo nextest run --workspace --all-features` (build first; see TESTING.md) |
| Format | `cargo fmt --all` |
| Lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| Lint workflows | `actionlint` |
| Supply chain | `cargo deny check` |
| All hooks | `prek run --all-files` |

## Conventions

- Target `x86_64-pc-windows-msvc` and foobar2000 2.25 and later only. The SDK target version stays 81, the newest the SDK defines.
- No C or C++ anywhere in the build.
- `foxide2k` is licensed `MIT AND BSD-Source-Code`. Every file transcribed from the foobar2000 SDK starts with the SDK notice. Other crates are MIT.
- Every vtable slot is `unsafe extern "C-unwind" fn`, and every Rust-implemented slot runs inside `foxide2k::boundary::guard`.
- Never throw a C++ exception inside `catch_unwind`. Wrap every call into foobar2000 that can throw in `foxide2k::cxx::try_host`.
- Tests are written first. Keep code that can only be tested by hand thin.
- Commit subjects are sentence-case imperative with no prefix; the body explains why.
- Lint workflows with the kjanat actionlint fork (latest release), never `rhysd/actionlint`.
- `foo_librespot_core` must not depend on `foxide2k` or know about foobar2000. The plugin talks to it only through `foo_librespot_core::Core`.
- In `foo_librespot_core`, blocking reads of Spotify audio never run on a tokio worker: `fetch_track` only awaits, and `FetchedTrack::open` and `SpotifyTrack::read` run on the caller's thread. `CoreRuntime::block_on` is never called from async code.
- Spotify credentials are never logged and never written in plain text; `Profile` stores them encrypted with DPAPI.
- `foo_librespot` reaches Spotify only through its `Backend` trait; its tests install a fake backend, so every service is tested through its vtables without Spotify.

## Settled design decisions

Do not re-litigate these without new evidence.

- The component is written entirely in Rust, including the assembly-level C++ exception catcher. A C fallback needs the maintainer's approval.
- Spotify tracks are stored as `sptf://spotify:track:<id>`, the form foo_spotify used, so old playlists keep working. Episodes Spotify hosts are stored as `sptf://spotify:episode:<id>`.
- foo_librespot decodes Spotify audio only for playback (`input_flag_playback`), as foo_spotify did, and refuses everything else with "Access denied": the Converter, the ReplayGain scanner and similar tools. Decrypted audio never reaches a file. Refusing by default is the maintainer's decision; there is no switch to allow conversion, because foobar2000 can't tell a scanner from a converter, so any switch would turn the component into a ripper.
- An episode Spotify doesn't host is added as the podcast's own audio link (`Episode::external_url`), which foobar2000 plays itself (the maintainer's decision).
- Login uses librespot's OAuth device flow with librespot's built-in client ID. No Spotify Web API.
- The browser never opens by itself (the maintainer's decision). Only the "Open browser" button opens Spotify's login page; a link that needs a login opens the login window, not the browser.
- The preferences page and the login window are standard Win32 dialogs built from in-memory templates. Each label is created right before its control, creation order is tab order, and the status line is a polite live region.
- librespot is pinned to the branch of PR #1855 until that fix is released, because stock librespot exits the process on a non-Premium login.
- ReplayGain comes from Spotify's `AUDIO_FILES` extended metadata (integrated loudness and true peak), fetched in the same request as the track's other metadata, so tracks have it as soon as they are imported. The gains in the audio file's header, plus 1.5 dB, are only a fallback.
- `panic = "unwind"`, never `abort`: a panic must reach foobar2000 as a C++ exception.
- The dynamic CRT, as foobar2000 itself uses. No `+crt-static`.
- foobar2000 2.25 and later only (the maintainer's decision). Foxide2k uses the interfaces 2.25 introduced, such as `input_decoder_v5::poll_live_info`, and leaves the ones they replace, such as `get_dynamic_info`, unanswered.

## Win32 bindings

- Every Win32 item comes from the committed, generated `crates/foxide2k/src/bindings.rs`. Never edit it by hand.
- `tools/bindgen/` is a standalone crate with its own lockfile, not a workspace member. To use a new API, add it to `--filter` in `tools/bindgen/foxide2k.txt` and run `cargo bindgen`. CI fails if the committed output is stale.
- `windows-core` is the only Windows runtime crate; the `windows` umbrella crate is not used.
- CRT and vcruntime functions (`_CxxThrowException`, `__std_exception_copy`, `free`, …) are not in the metadata; declare them in a local `extern` block, as Rust's std does.
