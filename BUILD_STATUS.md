# Build and verification status

Source-only experimental port generated 2026-11-18.

- Repository structures and format contracts from both reference projects were inspected.
- Rust source and tests are included; **the Rust toolchain was unavailable in the
  authoring environment, so no compilation or runtime tests could be executed**.
- No benchmark results are available. The configured optimizer settings are a
  best-effort starting point, not proof of faster runtime or smaller binaries.
- GitHub Actions are included for compilation, Clippy, tests, and release builds.
- Go feature-perfect parity is not yet established. See README limitations.

To validate locally:

```sh
cargo fmt --all
cargo test --all-targets
cargo clippy --all-targets
cargo build --release
./target/release/agy-auth-rust --help
```

Avoid pointing an unverified build at your only copy of valuable credentials.
