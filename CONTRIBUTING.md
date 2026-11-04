# Contributing

Thank you for helping. A few rules keep the code base easy to work on.

## Code

- Format with `cargo fmt --all`. The prek hooks do this for you; install them with `prek install`.
- Run `cargo lint` and fix any warnings. It runs clippy on every crate, target and feature, as CI does.
- Comments explain why the code does something, never what it does.
- Every bug fix comes with a regression test that fails without the fix.
- New behaviour is developed test-first. See [TESTING.md](TESTING.md).

## Pull requests

- One change per pull request.
- Explain what changes and why, and how you tested it. Say which setups you did not test.
- CI must pass.

## AI-assisted contributions

AI-assisted contributions are welcome if you understand the change, have tested it and have read all of the code.
Otherwise the pull request is closed.

## Licence

By contributing you agree that your contribution is released under the MIT licence.
