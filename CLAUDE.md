# read-time

Medium-style reading time estimates for text on stdin. A Rust CLI with a
matching library crate and **no dependencies** — keep it that way unless there
is a reason that survives an argument.

- `src/lib.rs` — the estimator and its tests
- `src/main.rs` — argument parsing and output formatting

## Before every commit

Run all three, in this order, and make sure they pass:

```console
cargo fmt --all
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

`cargo fmt --all` rewrites files, so run it first and include whatever it
changes in the commit. CI runs the same three checks as `cargo fmt --all
--check`, and a failure on any of them blocks the merge — and on `main`, blocks
the release. Do not commit expecting to fix formatting afterwards.

## CI and releases

- `.github/workflows/ci.yml` — format, clippy, and tests on every pull request
  and every push to `main`; tests run on Linux, macOS, and Windows.
- `.github/workflows/release.yml` — every push to `main` re-runs the three
  checks, then builds five targets and replaces the rolling `latest` release
  with the archives and a `SHA256SUMS` file.

The `latest` release is replaced wholesale on each push, so its download URLs
are stable and it never carries assets from an older commit.
