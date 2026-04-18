# Contributing to Hiveloom

Thanks for considering a contribution. This document covers how to build, test,
and submit changes.

## Before you start

- **Discuss non-trivial changes first.** Open an issue describing the problem
  and your proposed approach before writing a large patch. For quick fixes
  (typos, small bugs), a PR is fine.
- **Read [`docs/architecture.md`](docs/architecture.md)** to get oriented in the
  codebase.

## Build and run

Requires a stable Rust toolchain (edition 2021).

```bash
git clone https://github.com/FrancescoMrn/hiveloom-rust
cd hiveloom-rust
cargo build --release
./target/release/hiveloom serve --data-dir ./data
```

For development iteration, `cargo run -- <subcommand>` is faster than
rebuilding. `--help` is available on every subcommand.

## Tests

```bash
cargo test
```

Integration tests live under `tests/` and exercise the HTTP admin API and MCP
surfaces with an in-process service. No external network or database is
required — each test spins up its own temp data directory.

## Lint and format

The CI gate runs:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked
cargo test --all-features --locked
```

Clippy currently runs informationally (warnings do not block). Please avoid
introducing new warnings in code you touch, and silence them with a focused
`#[allow(...)]` + comment if they are intentional.

Run `cargo fmt --all` before committing — the `fmt --check` step is strict and
will fail otherwise.

## Commit and PR conventions

- **One concern per commit.** Split refactors, feature work, and test additions.
- **Commit subjects in imperative mood**, prefixed by a type: `feat:`, `fix:`,
  `docs:`, `chore:`, `refactor:`, `test:`, `ci:`. Keep subjects ≤ 70 characters.
- **Reference the issue** the PR resolves in the body (not the title):
  `Closes #123`.
- **No AI-tool co-author trailers.** If you used an assistant to help write the
  change, authorship is still yours.
- **Rebase, don't merge.** Keep feature branches linear against `main` or
  `develop`.

## What runs in CI

See [`.github/workflows/ci.yml`](.github/workflows/ci.yml). A PR must pass:

- `rustfmt --check`
- `clippy` (informational)
- `cargo test`

on Ubuntu latest with stable Rust.

## Security issues

Do **not** open a public issue for vulnerabilities. See
[`SECURITY.md`](SECURITY.md) for private disclosure.

## Licensing

By submitting a PR you agree that your contributions are licensed under the
project's Apache-2.0 license (see [`LICENSE`](LICENSE) when added, or the
`license` field in `Cargo.toml`).
