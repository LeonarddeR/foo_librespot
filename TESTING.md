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

1. Write the crashing test as `#[ignore]` and make it return at once unless the `OXIBAR_CHILD` environment variable is set. Call `testing_support::suppress_crash_dialogs()` first, so Windows shows no crash dialog.
2. In the real test, start `std::env::current_exe()` with `--exact <full test path> --ignored --nocapture` and `OXIBAR_CHILD=1`, and assert its exit code, for example `0xC0000005` for an access violation.

`cxx::catch::tests::access_violation_is_not_caught` is the example.
