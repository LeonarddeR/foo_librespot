# Testing

Tests run with [cargo-nextest](https://nexte.st), which runs every test in its own process.

```
cargo build --workspace --all-features
cargo nextest run --workspace --all-features
```

Build first: the DLL tests load the built component DLLs, and nextest does not build them.

## Writing a test

Write the test before the code, run it and watch it fail, then write the code that makes it pass.
Arrange the test in three parts:

```rust
#[test]
fn describes_the_behaviour() {
	// Arrange: set up the inputs and fakes.
	// Act: call the code under test once.
	// Assert: check the outcome.
}
```

Name the test after the behaviour it checks, not after the function it calls.

## Tests that must crash

Some behaviour can only be checked by letting a process crash, such as "`try_host` does not catch an access violation".
Run the crashing part in a child process:

1. Write the crashing test as `#[ignore]` and make it return at once unless the `FOXIDE2K_CHILD` environment variable is set. Call `testing_support::suppress_crash_dialogs()` first, so Windows shows no crash dialog.
2. In the real test, start `std::env::current_exe()` with `--exact <full test path> --ignored --nocapture` and `FOXIDE2K_CHILD=1`, and assert its exit code, for example `0xC0000005` for an access violation.

`cxx::catch::tests::access_violation_is_not_caught` is the example.

## Testing a component DLL

`foxide2k::testing` (the `testing` feature) is a fake foobar2000.
`FakeApi` plays the host: it registers services, records console output and reports a temporary profile folder.
`LoadedComponent::load` loads a built component DLL into it the way foobar2000 does: it calls `foobar2000_get_interface`, `set_library_path` and `services_init(true)`, and can then create every registered service.
Tests call the services through their vtables, so they exercise the same code paths foobar2000 does.

The DLL must be built before the tests run, because nextest does not build it:

```
cargo build --workspace --all-features
cargo nextest run --workspace --all-features
```

Set `FOO_OXITONE_DLL_PATH` to test a DLL from somewhere else, such as a CI artifact.

## Tests that create windows

Dialog tests create real, hidden windows, owned by `foxide2k::testing::hidden_window()`, and some check which control has keyboard focus.
They run one at a time, in the `windows` test group in `.config/nextest.toml`: a window of another test process that takes the foreground takes focus from theirs.
Add a new module with window tests to that group's filter.

## Testing in a real foobar2000

`tools/fb2k-portable.ps1` keeps a portable foobar2000 in `.fb2k-portable`, so your normal profile is never touched:

```
pwsh tools/fb2k-portable.ps1 -Setup
pwsh tools/fb2k-portable.ps1 -Install foo_oxitone
pwsh tools/fb2k-portable.ps1 -Run /add oxitone://440
```

Each milestone has a manual checklist in `docs/checklists/`, starting with [the phase 1 gate](docs/checklists/phase-1-foxide2k-gate.md).

To try the package a release publishes, run `cargo xtask package`. It needs cargo-about (`cargo install --locked cargo-about --features cli`) and writes `target/package/foo_librespot-<version>.fb2k-component`, which foobar2000 installs from Preferences > Components.

## Testing `foo_librespot_core`

The unit tests need no network and no Spotify account.
librespot sits behind small traits (`Connector`, `Authorizer`, `MetadataSource`) that the tests replace with fakes, and the audio tests decode the Ogg Vorbis files in `crates/foo_librespot_core/tests/fixtures`.
That folder's README has the ffmpeg commands that made them.

To try the crate against a real Spotify Premium account, use the `spotify_dev` example.
It keeps its login in `.spotify-dev`, which git ignores:

```
cargo run -p foo_librespot_core --example spotify_dev -- login
cargo run -p foo_librespot_core --example spotify_dev -- resolve https://open.spotify.com/album/<id>
cargo run -p foo_librespot_core --example spotify_dev -- decode https://open.spotify.com/track/<id> 10 .spotify-dev/track.wav
cargo run -p foo_librespot_core --example spotify_dev -- decode https://open.spotify.com/track/<id> 10 .spotify-dev/seek.wav --seek 60
```

`login` prints a page and a code; open the page, confirm, and the login is stored encrypted for your Windows account.
Set `SPOTIFY_DEV_LOG=debug` to see what librespot does.
The phase 2 checklist is in [docs/checklists/phase-2-core.md](docs/checklists/phase-2-core.md).

## Testing `foo_librespot`

The component's services run against a fake `Backend` in the unit tests, and its DLL tests load the built `foo_librespot.dll` into Foxide2k's fake foobar2000; neither needs Spotify.

To try it in the portable foobar2000, install it, start it, and log in from File > Preferences > Tools > Spotify, or add a link and use the login window that opens:

```
pwsh tools/fb2k-portable.ps1 -Install foo_librespot
pwsh tools/fb2k-portable.ps1 -Run /add spotify:album:4wPtS24asgBAG4VOmYYB9O
```

The dialogs can be checked without a keyboard through UI Automation, for example with `winapp ui list-windows -a foobar2000`, `winapp ui inspect -w <hwnd>` and `winapp ui screenshot -w <hwnd>`. Reading and capturing work while the Windows session is locked; sending keys does not.
The phase 4 checklist is in [docs/checklists/phase-4-preferences.md](docs/checklists/phase-4-preferences.md).
