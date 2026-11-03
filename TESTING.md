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
