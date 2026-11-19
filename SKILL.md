---
name: maintain-agy-auth-rust
description: >
  Essential guide, operational rules, and workflows for AI agents maintaining,
  extending, debugging, and releasing agy-auth-rust (Native Rust Google Antigravity
  multi-account switcher and real-time quota dashboard).
---

# 🤖 AI Maintainer Guide for `agy-auth-rust`

This document provides AI agents with the mental model, strict invariants, project layout, and standard recipes needed to safely maintain and evolve the `agy-auth-rust` repository.

---

## 🧭 Repository Mental Model

`agy-auth-rust` is a zero-telemetry, native Rust CLI tool and interactive terminal dashboard (TUI) designed to manage Google Antigravity accounts, monitor live Gemini quota, and switch accounts with sub-millisecond execution.

### Architecture Map

```text
src/
├── main.rs          # CLI binary entry point: parses args via clap and delegates to app.rs
├── switch.rs        # agy-switch binary entry point: directly launches the interactive TUI
├── app.rs           # Core command router, CLI handlers, Desktop process manager, and dispatcher
├── tui.rs           # Terminal UI dashboard: Ratatui/Crossterm event loop, gauge rendering, keybindings
├── quota.rs         # Real-time quota fetcher (cloudcode-pa), async revalidation, bucket parsers
├── oauth.rs         # Google OAuth2 PKCE login, token refresh, and agy binary SHA-256 hash discovery
├── store.rs         # Account registry (registry.json), snapshot storage, atomic locking
├── crypto.rs        # AES-256-GCM encryption/decryption envelope (agy-auth-encrypted:v1:)
├── desktop.rs       # Antigravity Desktop IDE session profile snapshotting and restoration
├── paths.rs         # System paths resolution, POSIX 0600/0700 file permission enforcement
├── formats.rs       # Importers for native Antigravity CLI JSON, raw OAuth JSON, and bundled exports
└── model.rs         # Core data structs: Account, Registry, Credential, Config, Target
```

---

## 🔒 Non-Negotiable Invariants (AI Agents MUST Enforce)

1. **Zero Credential Leakage:**
   - **NEVER** commit real tokens, emails, refresh tokens, passwords, or cookies.
   - Only synthetic/fictional test fixtures (`tests/fixtures/go-compatible.*`) are allowed in git.
   - Keep `.gitignore` strict. Verify `git status` before committing.
2. **Zero External Telemetry:**
   - The application only contacts official Google OAuth (`accounts.google.com`, `oauth2.googleapis.com`) and Cloud Code Assist endpoints (`cloudcode-pa.googleapis.com`).
   - Do NOT introduce any third-party analytics, metrics collection, or tracking endpoints.
3. **Storage Encryption at Rest:**
   - Account snapshots on disk must ALWAYS be encrypted with AES-256-GCM authenticated encryption (`crypto::encrypt`).
   - `registry.json` stores account identifiers and metadata only—never raw refresh tokens.
4. **Atomic Disk Mutations:**
   - All file writes must use `paths::atomic_write` (write to tempfile, fsync, and atomic rename).
   - UNIX file permissions must remain strictly `0600` for files and `0700` for directories.
5. **Backwards Compatibility with Go `agy-auth`:**
   - The encrypted envelope prefix `agy-auth-encrypted:v1:` and 12-byte nonce + 16-byte tag layout must remain intact.
   - Do not break compatibility with `~/.config/agy-auth/` files.

---

## 🧪 Verification & QA Workflow

Before submitting any commit or pull request, AI agents MUST run and pass this verification pipeline:

```sh
# 1. Verify code formatting (must produce zero diff)
cargo fmt --all -- --check

# 2. Run Clippy (must have 0 warnings across all targets)
cargo clippy --all-targets

# 3. Run all unit and integration tests
cargo test --all-targets

# 4. Verify release build succeeds
cargo build --release

# 5. Verify CLI help runs properly
cargo run --bin agy-auth-rust -- --help
```

---

## 🛠️ Common Recipes for AI Agents

### 1. Adding or Modifying a CLI Command
- Subcommands are defined in `src/app.rs` under `enum CommandLine`.
- Keep flags ergonomic and include `--json` support for automated agent workflows.
- Update help text in `README.md` under [CLI Reference](README.md#-cli-reference).

### 2. Updating Quota Parsing or Model Buckets
- Quota API responses are handled in `src/quota.rs`.
- Gemini model groups are identified via `gemini_group()` and parsed into 5-hour and weekly buckets.
- If Google introduces new model IDs (e.g. Gemini 3.0 / 3.8 / Ultra), update `bucket_window()` and plan normalization in `src/quota.rs`.

### 3. TUI & Dashboard Changes (`src/tui.rs`)
- `agy-switch` is an instant interactive dashboard. Avoid blocking network operations on the main UI render loop.
- Network revalidation must use background channels or threads (`quota::revalidate_async`).
- Ensure terminal raw mode is properly cleaned up on exit (`crossterm::terminal::disable_raw_mode()`).

### 4. Updating Dependencies
- In `Cargo.toml`, maintain dependencies that do not force unnecessary C libraries.
- Linux keyring utilizes `secret-service` / `keyring` with D-Bus.
- Ensure `Cargo.lock` is updated and committed alongside `Cargo.toml`.

### 5. Managing Flakes & NixOS (`flake.nix`)
- If adding a new Rust dependency requiring system C libraries, update `buildInputs` in `flake.nix` (e.g. `pkgs.dbus`, `pkgs.openssl`).
- Keep `flake.nix` using pure `nixpkgs` (avoid adding heavy extra flake inputs).

---

## 🚀 Release Process for AI Agents

To release a new version of `agy-auth-rust`:

1. **Bump Version in `Cargo.toml` and `flake.nix`:**
   ```toml
   # Cargo.toml
   version = "0.2.0"
   ```
   ```nix
   # flake.nix
   version = "0.2.0";
   ```
2. **Run Full Verification:**
   ```sh
   cargo fmt --all -- --check
   cargo clippy --all-targets
   cargo test --all-targets
   cargo build --release
   ```
3. **Commit the Bump:**
   ```sh
   git add Cargo.toml Cargo.lock flake.nix
   git commit -m "chore(release): bump version to v0.2.0"
   git push origin master
   ```
4. **Tag and Push the Release:**
   ```sh
   git tag -a v0.2.0 -m "Release v0.2.0"
   git push origin v0.2.0
   ```
5. **Verify GitHub Actions:**
   GitHub Actions will automatically trigger `.github/workflows/release.yml`, build Linux (x86_64), macOS (Apple Silicon & Intel), and Windows (x86_64) binaries, and publish them to GitHub Releases.

---

## ⚠️ Important Gotchas to Remember

- **Multiple Binaries:** Always specify `--bin agy-auth-rust` or rely on `default-run = "agy-auth-rust"` in `Cargo.toml`.
- **Lockfile Policy in CI:** Do NOT add `cargo generate-lockfile` before `cargo build --locked`. Use the tracked `Cargo.lock`.
- **macOS Runners:** Use `macos-latest` (macOS 14/15) in GitHub Actions. Do NOT reference deprecated `macos-13`.
- **Terminal Raw Mode in Tests:** Do not call functions from `tui.rs` that enter raw terminal mode in non-interactive unit tests.
