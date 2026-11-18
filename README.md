<div align="center">
  <h1>AgyBuff Auth</h1>
  <p><strong>Native Rust multi-account switcher for Google Antigravity CLI and Desktop.</strong></p>
  <p>Inspired by <a href="https://github.com/deactivated0/freebuff-auth">FreeBuff Auth</a>. Ported from the architecture and command set of <a href="https://github.com/fikrilal/agy-auth">agy-auth</a>.</p>
</div>

**Development status: experimental source release.** This is a substantial Rust port, **not yet a verified, feature-complete replacement** for the original Go project. It needs Rust compilation and integration testing on a machine with Antigravity installed. See [Compatibility and remaining work](#compatibility-and-remaining-work) and [Security](SECURITY.md) before using it with real accounts.

## Goals

- Fully native Rust binaries named `agybuff-auth` and `agybuff-switch`.
- Reuse existing `agy-auth` registry, encrypted snapshots and encryption key rather than forcing account re-login.
- Fast CLI switching between multiple Antigravity accounts, CLI/desktop credential storage, per-account installation UUIDs.
- Google OAuth PKCE, including optional GCP project mode.
- Grouped quota and plan reporting, quota-aware rotation, account import/export and backups.
- Sensible release performance: optimized code generation, small I/O, bounded quota parallelism, and atomic writes.

## Build and install

**Requirements:** Rust toolchain with Cargo (1.78+), network access on first build to obtain crates, the Antigravity CLI (`agy`) for actual sessions. Linux desktops may require a running Secret Service keyring; a file fallback is supported.

```sh
cargo test
cargo build --release
cargo install --path .

agybuff-auth --help
agybuff-auth status
```

Release builds use LTO, one codegen unit, stripping, and `opt-level = 3` for throughput. For minimal binary size instead:

```sh
cargo build --profile small
```

The result is **source code**, not a precompiled executable. No actual throughput, binary-size or resource benchmarks have been measured in the generation environment.

## Quick start

```sh
agybuff-auth add --alias personal            # register current agy CLI login
agybuff-auth login --alias work               # OAuth browser login
agybuff-auth login --via-agy --alias test     # optional: launch official agy in isolated HOME
agybuff-auth list --no-usage                  # offline account listing
agybuff-auth list                             # quota-enriched listing
agybuff-auth usage --all                      # all account quota groups
agybuff-auth switch work                      # switch selected CLI / saved Desktop session
agybuff-auth switch -                         # return to previous account
agybuff-auth rotate                           # only when current quota is exhausted
agybuff-auth rotate --force                   # force switch to a healthy alternative
agybuff-switch                                # arrow-key account picker
```

For GCP Pay As You Go:

```sh
agybuff-auth login --gcp --project my-gcp-project --region global --alias work
agybuff-auth switch work --target cli
```

Run `agybuff-auth --help` for all flags and options. When switching to a Desktop account, close Antigravity Desktop first or pass `--close-desktop`; add `--relaunch` to start it again. A running `agy` process keeps the old credential until restarted.

## Command overview

| Command | What it does |
|---|---|
| `list` | View stored accounts, optional quota, `--json`, `--no-sync`, `--no-usage` |
| `add` | Snapshot the current `agy` account, optionally Desktop session |
| `login` | Google OAuth PKCE or isolated official CLI login via `--via-agy` |
| `switch` | Select by row, email, ID, alias, project or `-` for previous |
| `rotate` | Check actual quota and switch to a verified healthy alternative |
| `remove` | Unregister selected account(s); optional `--logout` and `--yes` |
| `alias` | `set`, `clear`, `list` |
| `current` / `status` | Show live account and storage health |
| `usage` | Quota groups and reset times when returned by backend |
| `config live --interval N` | Save a refresh interval; 5 through 3600 seconds |
| `import` / `export` | Import native, encrypted or standard OAuth JSON formats; export plaintext credentials |
| `import --purge` | Rebuild account metadata from encrypted snapshots |
| `clean --dry-run` | Preview orphan cleanup and old-backup pruning |
| `path` / `version` | Paths and build version |

`--json` is global. The interactive picker accepts arrow keys, Home/End, PgUp/PgDn, `j`/`k`, Enter and Escape. `agybuff-auth switch --live` currently opens the picker, but is not yet a continuously refreshed quota TUI.

## Existing agy-auth data compatibility

The default data directory is **the original** `agy-auth` directory. No migration step is needed for ordinary v2 metadata and encrypted snapshots. The program uses these existing formats:

```text
~/.config/agy-auth/registry.json
~/.config/agy-auth/encryption.key
~/.config/agy-auth/accounts/acct_<hash>.json
~/.config/agy-auth/accounts/acct_<hash>.installation_id
~/.config/agy-auth/desktop/acct_<hash>/
~/.gemini/antigravity-cli/antigravity-oauth-token
~/.gemini/antigravity-cli/settings.json
```

It reads `agy-auth-encrypted:v1:` snapshots with the original AES-256-GCM envelope: 12-byte nonce, encrypted JSON and 16-byte authentication tag, authenticated with the exact prefix as AAD. The key is base64 encoded in `encryption.key`. The registry schema and GCP-scoped account ID hashing are intentionally compatible.

**Back up your whole `agy-auth` directory and key before first use.** The program writes snapshots and metadata, so don't test it on the only copy of valuable credentials. For dry testing on a copy:

```sh
cp -a ~/.config/agy-auth /tmp/agybuff-test
AGYBUFF_AUTH_HOME=/tmp/agybuff-test AGY_AUTH_STORAGE=file agybuff-auth list --no-sync --no-usage
```

### Supported input formats (private data stays on your computer)

`import` reads the files you specify **at runtime**, not from the source tree. Supported inputs are:

- Native Antigravity CLI credential JSON: `{"token": {"access_token": "...", "refresh_token": "..."}, ...}`
- Original `agy-auth` encrypted account snapshots (`agy-auth-encrypted:v1:`), with the original `encryption.key` **supplied only on the user's machine**
- Standard OAuth token JSON with top-level `access_token` / `refresh_token` fields
- Credential JSON arrays and explicit `credentials`, `credential`, or `accounts` wrappers
- GCP Pay As You Go metadata (`auth_method`, `project_id`, `region`) in supported credential documents

To import an existing encrypted snapshot using a separate key:

```sh
agybuff-auth import /private/backup/acct_xxxxxxxxxx.json --key /private/backup/encryption.key
```

To import a standard local OAuth JSON file:

```sh
agybuff-auth import /private/credentials.json --alias personal
```

`registry.json` by itself is **metadata only**, not a credential export. Use `agybuff-auth import --purge` to reconstruct metadata from existing snapshots in the configured store. This tool never uploads your credentials, token JSON, or encryption key to the source project. Google OAuth and quota queries still contact Google's own endpoints as required for authentication and usage. Do not run these commands on other people's data without authorization.

**The ZIP contains only source code, documentation, and artificial test fixtures. No authentic user tokens, accounts, cookies, keys, or private profiles are included.**

Other environment variables: `AGY_AUTH_HOME`, `AGY_AUTH_AGY_TOKEN_FILE`, `AGY_AUTH_AGY_SETTINGS_FILE`, `AGY_AUTH_STORAGE=file`, `AGY_DESKTOP_HOME`, `AGY_DESKTOP_CONFIG_DIR`, `AGY_DESKTOP_BIN`, `AGY_AUTH_CLIENT_ID`, `AGY_AUTH_CLIENT_SECRET`, `AGY_AUTH_GCP_CLIENT_ID`, `AGY_AUTH_GCP_CLIENT_SECRET`.

### Security notes

- Credential snapshots and automatic active-credential backups use AES-256-GCM at rest.
- Registry and token writes are atomic. The Rust process holds a registry lock during mutations, although the old Go program does not honor this lock, so do not run both tools concurrently.
- File permissions on newly created secret files are `0600` and newly created managed directories `0700` on Unix.
- Keyring access is best-effort; the original `agy` plaintext fallback file is also updated on switches.
- Exported JSON files contain **plaintext access/refresh tokens**. Keep them private and delete when finished.
- The `id_token` payload is decoded only for local identification; its signature is not independently verified by this tool.
- Quota calls use the internal Code Assist backend documented by agy-auth. This is not a supported public API and may change.
- No telemetry is added. Don't upload real tokens, snapshots, or `encryption.key` in bug reports.

## Compatibility and remaining work

| Area | Current state |
|---|---|
| Registry and AES-GCM snapshot envelope | Implemented, with independent Python-generated interoperability fixture |
| Account matching, GCP IDs, import/export, aliases, backups | Implemented, not fully integration-tested |
| Keyring and CLI file fallback | Implemented, platform testing outstanding |
| Antigravity Desktop profile snapshots / restore | Implemented, platform testing outstanding |
| OAuth and client discovery from `agy` executable | Implemented, live-login validation outstanding |
| Real quota group reporting and rotation | Implemented against the original undocumented endpoints, live validation outstanding |
| Terminal picker | Implemented |
| Live updating TUI and automatic switch inside live view | **Not implemented** |
| Original updater / self-update workflow | **Not implemented** (`version --check` reports this) |
| Original internal Go development harness and all its regression tests | **Not ported** |
| Guaranteed Windows/macOS/Linux behavior | **Not verified** |

This package does not claim behavior-perfect parity, successful compilation, passing tests, or measured optimizations until those checks actually run.

## Testing and release

`cargo test` includes AES-GCM interoperability, account ID, query ambiguity, and basic state tests. GitHub Actions CI is provided for Linux, macOS and Windows, with a release workflow on tags. Since this source ZIP was created without Cargo installed or crate download access, the workflows have **not** been run here. See [Build status](BUILD_STATUS.md).

## Credits and license

- [fikrilal/agy-auth](https://github.com/fikrilal/agy-auth), reference architecture, behavior and formats (MIT).
- [deactivated0/freebuff-auth](https://github.com/deactivated0/freebuff-auth), Rust CLI concept and naming inspiration (MIT).

AgyBuff Auth source code is distributed under the MIT license. This is an unofficial project, not affiliated with Google.
