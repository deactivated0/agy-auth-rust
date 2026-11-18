# Security and credential handling

**Never submit real credentials or profile data to source control, issues,
CI artifacts, pull requests, or releases.** The source ZIP contains none.

## What the program reads on your computer

- Antigravity CLI credential files and the OS keyring entry (`gemini` / `antigravity`).
- `agy-auth` account registry, encrypted snapshots, and encryption key.
- Optional Antigravity Desktop session profile data, which can contain cookies.
- User-provided JSON files during an explicit `import` operation.

These are only accessed when the compiled tool is run locally. They are not
included in builds or source packages. Import `--key` names an external file.
Never copy that file into this repository.

## Stored files and exports

- Account snapshots and active-credential backups are AES-256-GCM encrypted.
- The encryption key is generated locally and stored in the user's own
  configuration directory with restrictive permissions on Unix.
- The registry holds account identifiers, aliases, and email metadata, not tokens.
- Antigravity's own fallback credential file is plaintext by design, because
  the official CLI expects to read it. Do not assume it is encrypted.
- `export` explicitly writes **plaintext** secrets to a user-selected directory.
  Keep that directory private and securely delete its contents when finished.
- Desktop session profiles may contain cookies. The tool never packages these.

## Network operations

Google OAuth and quota checks communicate with Google's authentication and
Code Assist endpoints. No custom analytics or telemetry is added. The quota
API is undocumented and subject to change; failures cannot safely be treated
as available quota.

## Operational precautions

- Back up `~/.config/agy-auth` and the encryption key before first use.
- Do not run the Go original and this experimental Rust port concurrently.
- Test against a copied data directory with `AGY_AUTH_RUST_HOME` (or `AGYBUFF_AUTH_HOME`) first.
- Never paste refresh tokens, access tokens, cookies, or keys into a bug report.
- The supplied interoperability fixtures were generated from fictional test
  values only. They cannot authenticate to any service.

This port has not been compiled or live-integrated in the authoring environment.
