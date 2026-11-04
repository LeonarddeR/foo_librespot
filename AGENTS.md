# Agent notes

foo_librespot is a foobar2000 2.x x64 component, written entirely in Rust, that plays Spotify links through librespot.

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

- Target `x86_64-pc-windows-msvc` and foobar2000 2.x (SDK target version 81) only.
- No C or C++ anywhere in the build.
- `oxibar` is licensed `MIT AND BSD-Source-Code`. Every file transcribed from the foobar2000 SDK starts with the SDK notice. Other crates are MIT.
- Every vtable slot is `unsafe extern "C-unwind" fn`, and every Rust-implemented slot runs inside `oxibar::boundary::guard`.
- Never throw a C++ exception inside `catch_unwind`. Wrap every call into foobar2000 that can throw in `oxibar::cxx::try_host`.
- Tests are written first. Keep code that can only be tested by hand thin.
- Commit subjects are sentence-case imperative with no prefix; the body explains why.
- Lint workflows with the kjanat actionlint fork (latest release), never `rhysd/actionlint`.
- `foo_librespot_core` must not depend on `oxibar` or know about foobar2000. The plugin talks to it only through `foo_librespot_core::Core`.
- In `foo_librespot_core`, blocking reads of Spotify audio never run on a tokio worker: `fetch_track` only awaits, and `FetchedTrack::open` and `SpotifyTrack::read` run on the caller's thread. `CoreRuntime::block_on` is never called from async code.
- Spotify credentials are never logged and never written in plain text; `Profile` stores them encrypted with DPAPI.

## Settled design decisions

Do not re-litigate these without new evidence.

- The component is written entirely in Rust, including the assembly-level C++ exception catcher. A C fallback needs the maintainer's approval.
- Spotify tracks are stored as `sptf://spotify:track:<id>`, the form foo_spotify used, so old playlists keep working.
- Login uses librespot's OAuth device flow with librespot's built-in client ID. No Spotify Web API.
- librespot is pinned to the branch of PR #1855 until that fix is released, because stock librespot exits the process on a non-Premium login.
- ReplayGain comes from Spotify's `AUDIO_FILES` extended metadata (integrated loudness and true peak), fetched in the same request as the track's other metadata, so tracks have it as soon as they are imported. The gains in the audio file's header, plus 1.5 dB, are only a fallback.
- `panic = "unwind"`, never `abort`: a panic must reach foobar2000 as a C++ exception.
- The dynamic CRT, as foobar2000 itself uses. No `+crt-static`.

## Win32 bindings

- Every Win32 item comes from the committed, generated `crates/oxibar/src/bindings.rs`. Never edit it by hand.
- `tools/bindgen/` is a standalone crate with its own lockfile, not a workspace member. To use a new API, add it to `--filter` in `tools/bindgen/oxibar.txt` and run `cargo bindgen`. CI fails if the committed output is stale.
- `windows-core` is the only Windows runtime crate; the `windows` umbrella crate is not used.
- CRT and vcruntime functions (`_CxxThrowException`, `__std_exception_copy`, `free`, …) are not in the metadata; declare them in a local `extern` block, as Rust's std does.
