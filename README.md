<div align="center">

# ⚡ agy-auth-rust

### Native Rust Multi-Account Switcher & Real-Time Quota Dashboard for Google Antigravity

[![Rust CI](https://github.com/deactivated0/agy-auth-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/deactivated0/agy-auth-rust/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust: 1.78+](https://img.shields.io/badge/Rust-1.78%2B-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey.svg)]()
[![Security: AES-256-GCM](https://img.shields.io/badge/Security-AES--256--GCM-green.svg)]()

<p align="center">
  <strong>Switch Antigravity accounts in milliseconds. Monitor Gemini quotas in real time. Never get rate-limited mid-prompt again.</strong>
</p>

<p align="center">
  <a href="#-key-features">Key Features</a> •
  <a href="#-interactive-tui-dashboard">Terminal UI</a> •
  <a href="#-quick-start">Quick Start</a> •
  <a href="#-cli-reference">CLI Reference</a> •
  <a href="#-security--privacy-architecture">Security & Privacy</a> •
  <a href="#-installation">Installation</a> •
  <a href="#-compatibility">Compatibility</a>
</p>

---

</div>

## 📌 Overview

**agy-auth-rust** is a high-performance, native Rust CLI utility and interactive terminal dashboard designed for power users of the **Google Antigravity CLI (`agy`)** and **Antigravity Desktop IDE**.

When developing complex agentic workflows, large refactors, or running continuous AI coding loops, developers frequently hit Gemini rate limits and 5-hour quota ceilings across models like **Gemini Pro**, **Gemini Flash**, and partner models. 

`agy-auth-rust` solves this by providing:
1. **Sub-millisecond multi-account switching** for both CLI and Desktop IDE sessions.
2. **Real-time ASCII Cyber Dashboard (`agy-switch`)** displaying live remaining quotas, tiers (Free, Pro, Ultra, PayGo), and relative reset countdowns.
3. **Smart Quota-Aware Auto-Rotation** that detects exhausted accounts and seamlessly promotes the next healthiest account.
4. **Zero-Trust Local Security** protecting all credentials with AES-256-GCM authenticated encryption at rest.
5. **Drop-in Compatibility** with existing `agy-auth` (Go) registries and snapshots without re-authentication.

---

## ⚡ Key Features

- **🚀 Native Rust Speed & Efficiency**: Zero garbage collection pauses, zero Node/Electron/Python overhead, sub-millisecond execution, and minimal memory footprint.
- **🖥️ Standalone Cyber TUI (`agy-switch`)**: Fully responsive ANSI/ASCII terminal dashboard with color-coded quota meters, live countdowns, quick hotkeys, and numeric account jumping.
- **🔄 Quota-Aware Auto-Rotation (`agy-auth-rust rotate`)**: Automatically queries remaining 5-hour and weekly capacity across accounts and switches to a fresh, healthy profile when current limits are reached.
- **📊 Real-Time Antigravity Quota Tracking**: Live telemetry fetched directly from Google's Code Assist backend, tracking model quotas (Gemini 1.5/2.0/3.0 Pro, Flash, and 3P models) without opening a browser.
- **🔐 Hardened AES-256-GCM Encryption**: All stored account snapshots are encrypted locally with a 256-bit key and authenticated with AAD. Restrictive `0600`/`0700` POSIX file permissions on Unix.
- **💻 CLI & Desktop Synchronization**: Switches active credentials for the `agy` CLI (`antigravity-oauth-token`) and seamlessly syncs Antigravity Desktop IDE sessions with automatic process termination/relaunching.
- **☁️ Google Cloud (GCP) Pay-As-You-Go Ready**: Built-in support for GCP OAuth PKCE authentication, specific Google Cloud Project IDs, and regional scopes.
- **🖱️ Desktop Shortcut Integration**: Generates `.desktop` application launchers so you can trigger the switcher straight from your desktop environment, dock, or application runner (Rofi, Wofi, Albert, Spotlight).

---

## 🖥️ Interactive TUI Dashboard (`agy-switch`)

Launch the standalone dashboard at any time with `agy-switch`:

```text
┌── [ agy-switch ] ─────────────────────────────────────────── [● ACTIVE] ──┐
│                                                                            │
│  [1] work-account (Pro)                    [2] * personal-main (Free)      │
│  ID: acct_84f9b201a4                       ID: acct_3b1a8d90ce             │
│  Gemini 5h: [██████████████████░░] 88%     Gemini 5h: [██░░░░░░░░░░░░░░░░] 12%     │
│  Weekly:    [████████████░░░░░░░░] 60%     Weekly:    [██████████░░░░░░░░] 52%     │
│  Reset:     resets in 3h 41m               Reset:     resets in 48m                │
│                                                                            │
│  [3] dev-sandbox (PayGo)                   [4] research-tier (Pro)         │
│  ID: acct_e571c08d19                       ID: acct_f20a9117ba             │
│  Gemini 5h: [████████████████████] 100%    Gemini 5h: [██████████████░░░░] 74%     │
│  Weekly:    [██████████████████░░] 92%     Weekly:    [████████████████░░] 81%     │
│  Reset:     Full Quota Available           Reset:     resets in 1h 15m             │
│                                                                            │
├────────────────────────────────────────────────────────────────────────────┤
│  [Enter] Switch  [R] Refresh/Rotate  [A] Add Current  [D] Delete  [Q] Quit │
└────────────────────────────────────────────────────────────────────────────┘
```

### Dashboard Hotkeys

| Key | Action |
|---|---|
| <kbd>↑</kbd> / <kbd>k</kbd>, <kbd>↓</kbd> / <kbd>j</kbd> | Navigate accounts |
| <kbd>1</kbd> - <kbd>9</kbd> | Instant direct selection by account number |
| <kbd>Enter</kbd> | Switch to selected account immediately |
| <kbd>r</kbd> / <kbd>R</kbd> | Force refresh live quotas from backend |
| <kbd>a</kbd> / <kbd>A</kbd> | Register / snapshot current active CLI session |
| <kbd>d</kbd> / <kbd>D</kbd> | Remove account from switcher (requires confirmation) |
| <kbd>i</kbd> / <kbd>I</kbd> | Import external credential JSON |
| <kbd>Home</kbd> / <kbd>End</kbd> | Jump to first / last account |
| <kbd>q</kbd> / <kbd>Esc</kbd> | Exit dashboard |

---

## 🚀 Quick Start

### 1. Add your currently logged-in account
Register whatever account is currently signed in on your official `agy` CLI:
```sh
agy-auth-rust add --alias personal
```

### 2. Add additional accounts via OAuth PKCE
Authenticate a second or third account directly through your browser:
```sh
agy-auth-rust login --alias work
agy-auth-rust login --alias client-project
```

*Or authenticate using an isolated official `agy` subprocess:*
```sh
agy-auth-rust login --via-agy --alias research
```

### 3. Check account status & live quotas
```sh
# View all accounts with live model quotas & reset timestamps
agy-auth-rust list

# Fast offline listing (no network round-trip)
agy-auth-rust list --no-usage

# Deep dive into individual model bucket limits
agy-auth-rust usage --all
```

### 4. Switch accounts instantly
```sh
# Switch by alias, index number, email, or account ID
agy-auth-rust switch work

# Switch back to your previous account (like `git checkout -`)
agy-auth-rust switch -

# Open interactive picker
agy-switch
```

### 5. Rotate when quota is exhausted
Run this manually or integrate into your agent's fallback hooks:
```sh
# Checks current account quota; only rotates if exhausted
agy-auth-rust rotate

# Force rotate to the account with highest available quota
agy-auth-rust rotate --force
```

---

## 🛠️ CLI Reference

```text
Usage: agy-auth-rust [OPTIONS] <COMMAND>

Commands:
  list (ls)       List stored accounts with tier & remaining quota gauges
  add             Snapshot current active agy CLI or Desktop credentials
  login           Authenticate a new account via Google OAuth PKCE
  switch (use)    Switch active account for CLI and/or Desktop IDE
  rotate          Intelligently rotate away from rate-limited accounts
  remove (rm)     Remove an account from local storage
  alias           Manage human-friendly alias labels (set, clear, list)
  current         Display the currently active session profile
  status          Detailed health status of credentials, keyring, and storage
  usage           Print granular quota groups (Gemini 1.5/2.0/3.0, Flash, 3P)
  sort            Re-order accounts prioritizing active and high-quota tiers
  desktop         Generate or update Desktop application launchers (.desktop)
  config          Adjust global preferences (e.g., auto-refresh intervals)
  import          Import native, encrypted, or standard OAuth credential files
  export          Export account credentials (plaintext; keep secure!)
  clean           Prune orphaned snapshots and stale backups
  path            Print resolved configuration and storage paths
  version         Print version and check for available updates
```

### Global Flags

- `--json`: Output raw, machine-readable JSON for scripting, CI/CD, and agent tooling.
- `--no-color`: Disable ANSI color escapes.

---

## 🔒 Security & Privacy Architecture

`agy-auth-rust` adheres to a strict **Zero-Leakage & Local-First** security philosophy:

```
                  ┌──────────────────────────────────────────────┐
                  │                 Local Machine                │
                  │                                              │
                  │   ~/.config/agy-auth/                        │
                  │   ├── encryption.key   [Mode: 0600]          │
                  │   ├── registry.json    [Account Metadata]    │
                  │   └── accounts/                              │
                  │       └── acct_*.json  [AES-256-GCM Encrypted│
                  │                         Token Snapshots]     │
                  └───────┬──────────────────────────────┬───────┘
                          │                              │
                          ▼ (Direct HTTPS)               ▼ (Direct HTTPS)
              ┌───────────────────────┐      ┌───────────────────────────┐
              │ Google OAuth Endpoint │      │ Google Cloud Code Assist  │
              │ accounts.google.com   │      │ cloudcode-pa.googleapis   │
              │ oauth2.googleapis.com │      │ (Live Quota Telemetry)    │
              └───────────────────────┘      └───────────────────────────┘
```

1. **Zero External Servers**: Telemetry is non-existent. No external third-party API or analytics server is ever contacted.
2. **Encrypted at Rest**: Account snapshots are encrypted using **AES-256-GCM** with a locally generated 256-bit cryptographic key and authenticated with associated data (AAD).
3. **Atomic File Operations**: All token and registry writes utilize tempfiles with immediate atomic renaming to prevent corruption during unexpected shutdowns.
4. **POSIX Permission Enforcement**: Newly created storage directories default to `0700` and credential files to `0600` on Unix systems.
5. **No Secrets in Source**: Test fixtures contain purely synthetic dummy values (`fixture-access`, `fixture-refresh`). Your personal credentials and account details remain solely on your local machine.

---

## 📦 Installation

### 📋 Prerequisites
- **Official Google Antigravity CLI (`agy`)** installed and authenticated.
- **Rust toolchain (1.78+)** if building via `cargo` ([Install Rust](https://rustup.rs/)).

---

### 🐧 Linux (All Major Distributions)

#### 1. Install System Dependencies
`agy-auth-rust` interfaces with your desktop's Secret Service keyring over D-Bus (with automatic encrypted file fallback). Install the basic build and D-Bus development headers:

<details open>
<summary><b>Select your Linux distribution:</b></summary>

- **Ubuntu / Debian / Linux Mint / Pop!_OS:**
  ```sh
  sudo apt update
  sudo apt install -y build-essential pkg-config libdbus-1-dev
  ```

- **Fedora / RHEL / CentOS / Rocky Linux / AlmaLinux:**
  ```sh
  sudo dnf install -y gcc pkgconf-pkg-config dbus-devel
  ```

- **Arch Linux / Manjaro / EndeavourOS:**
  ```sh
  sudo pacman -S --needed base-devel dbus
  ```

- **openSUSE (Tumbleweed / Leap):**
  ```sh
  sudo zypper install -y gcc pkg-config dbus-1-devel
  ```

- **Alpine Linux:**
  ```sh
  sudo apk add build-base pkgconf dbus-dev
  ```

- **Void Linux:**
  ```sh
  sudo xbps-install -Sy base-devel dbus-devel
  ```

- **NixOS:**
  See the dedicated [NixOS & Nix Flakes](#-nixos--nix-flakes) section below.

</details>

#### 2. Install via Cargo (One-Liner)
If you have Rust installed:
```sh
cargo install --git https://github.com/deactivated0/agy-auth-rust.git
```

#### 3. Or Install Precompiled Binaries
```sh
# Download and install latest Linux x86_64 binaries directly to /usr/local/bin
curl -s https://api.github.com/repos/deactivated0/agy-auth-rust/releases/latest \
  | grep "browser_download_url.*x86_64-unknown-linux-gnu.tar.gz" \
  | cut -d : -f 2,3 \
  | tr -d \" \
  | wget -qi - -O - \
  | sudo tar -xz -C /usr/local/bin
```

#### 4. Add Desktop Launcher (Optional)
Register `agy-switch` into your desktop launcher (GNOME, KDE Plasma, XFCE, Rofi, Wofi, Albert):
```sh
agy-auth-rust desktop
```

---

### ❄️ NixOS & Nix Flakes

`agy-auth-rust` provides first-class support for Nix Flakes (`flake.nix`) for both NixOS and non-NixOS Linux / macOS with Nix.

#### 1. Quick Ad-Hoc Execution (No Installation Required)
Run binaries ephemerally via `nix run`:
```sh
# Run agy-auth-rust CLI directly
nix run github:deactivated0/agy-auth-rust -- list

# Launch interactive switcher TUI
nix run github:deactivated0/agy-auth-rust#agy-switch
```

#### 2. Declarative NixOS System Configuration
Add `agy-auth-rust` to your system's `flake.nix`:
```nix
{
  description = "NixOS configuration";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    agy-auth-rust.url = "github:deactivated0/agy-auth-rust";
  };

  outputs = { self, nixpkgs, agy-auth-rust, ... }: {
    nixosConfigurations.myhostname = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux"; # or "aarch64-linux"
      modules = [
        ({ pkgs, ... }: {
          environment.systemPackages = [
            agy-auth-rust.packages.${pkgs.system}.default
          ];
        })
      ];
    };
  };
}
```
Rebuild your system:
```sh
sudo nixos-rebuild switch --flake .#myhostname
```

#### 3. Declarative Home Manager Configuration
Add to your `home.nix`:
```nix
{ inputs, pkgs, ... }: {
  home.packages = [
    inputs.agy-auth-rust.packages.${pkgs.system}.default
  ];
}
```

#### 4. Imperative Installation via Nix Profile
```sh
nix profile install github:deactivated0/agy-auth-rust
```

---

### 🍎 macOS (Apple Silicon & Intel)

#### 1. Install Dependencies
Ensure Apple Command Line Tools are installed:
```sh
xcode-select --install
```

#### 2. Install via Cargo
```sh
cargo install --git https://github.com/deactivated0/agy-auth-rust.git
```

#### 3. Or Install Precompiled Binaries

- **Apple Silicon (M1 / M2 / M3 / M4 - aarch64):**
  ```sh
  curl -s https://api.github.com/repos/deactivated0/agy-auth-rust/releases/latest \
    | grep "browser_download_url.*aarch64-apple-darwin.tar.gz" \
    | cut -d : -f 2,3 \
    | tr -d \" \
    | wget -qi - -O - \
    | sudo tar -xz -C /usr/local/bin
  ```

- **Intel Macs (x86_64):**
  ```sh
  curl -s https://api.github.com/repos/deactivated0/agy-auth-rust/releases/latest \
    | grep "browser_download_url.*x86_64-apple-darwin.tar.gz" \
    | cut -d : -f 2,3 \
    | tr -d \" \
    | wget -qi - -O - \
    | sudo tar -xz -C /usr/local/bin
  ```

---

### 🪟 Windows

#### Option 1: Install via PowerShell (Quick Setup)
Run PowerShell as Administrator or regular user:
```powershell
# Create target directory
$installDir = "$env:LOCALAPPDATA\Programs\agy-auth-rust"
New-Item -ItemType Directory -Force -Path $installDir | Out-Null

# Download latest release zip
$url = (Invoke-RestMethod -Uri "https://api.github.com/repos/deactivated0/agy-auth-rust/releases/latest").assets | Where-Object { $_.name -like "*windows-msvc.zip" } | Select-Object -ExpandProperty browser_download_url
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\agy-auth-rust.zip"

# Extract binaries
Expand-Archive -Path "$env:TEMP\agy-auth-rust.zip" -DestinationPath $installDir -Force
Remove-Item "$env:TEMP\agy-auth-rust.zip"

# Add to User PATH if not already present
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$installDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$installDir", "User")
    $env:Path += ";$installDir"
}

Write-Host "agy-auth-rust and agy-switch successfully installed!" -ForegroundColor Green
```

#### Option 2: Install via Cargo
```powershell
cargo install --git https://github.com/deactivated0/agy-auth-rust.git
```

---

### 🛠️ Building from Source (All Platforms)

```sh
# Clone repository
git clone https://github.com/deactivated0/agy-auth-rust.git
cd agy-auth-rust

# Build optimized release binaries
cargo build --release

# The compiled binaries will be located at:
# Linux/macOS: target/release/agy-auth-rust and target/release/agy-switch
# Windows:     target/release/agy-auth-rust.exe and target/release/agy-switch.exe
```

For ultra-compact binary footprint (stripping symbols and `opt-level = "z"`):
```sh
cargo build --profile small
```

---

## 🔄 Compatibility with Go `agy-auth`

`agy-auth-rust` is a 100% wire-compatible and storage-compatible port of `agy-auth`.

It directly reads and writes the standard configuration paths:
- Configuration Directory: `~/.config/agy-auth/`
- Registry File: `~/.config/agy-auth/registry.json`
- AES-256-GCM Encryption Key: `~/.config/agy-auth/encryption.key`
- Account Snapshots: `~/.config/agy-auth/accounts/acct_<hash>.json`
- Desktop Profiles: `~/.config/agy-auth/desktop/acct_<hash>/`
- Active CLI Token: `~/.gemini/antigravity-cli/antigravity-oauth-token`

You can switch between Go `agy-auth` and `agy-auth-rust` seamlessly without migrating accounts or re-authenticating.

---

## ⚙️ Environment Variables

Customize storage locations and behavior via environment variables:

| Variable | Description | Default |
|---|---|---|
| `AGY_AUTH_RUST_HOME` | Root configuration & storage directory | `~/.config/agy-auth` |
| `AGY_AUTH_STORAGE` | Storage backend (`keyring` or `file`) | `keyring` (fallback to file) |
| `AGY_AUTH_AGY_TOKEN_FILE` | Target location of the live CLI token file | `~/.gemini/antigravity-cli/antigravity-oauth-token` |
| `AGY_AUTH_AGY_SETTINGS_FILE` | Target location of settings file | `~/.gemini/antigravity-cli/settings.json` |
| `AGY_DESKTOP_HOME` | Antigravity Desktop configuration directory | OS-dependent config dir |
| `AGY_AUTH_CLIENT_ID` | Override Google OAuth Client ID | Discovered from `agy` binary |
| `AGY_AUTH_CLIENT_SECRET` | Override Google OAuth Client Secret | Discovered from `agy` binary |

---

## 🤝 Contributing

Contributions, bug reports, and suggestions are welcome!

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Ensure formatting and tests pass:
   ```sh
   cargo fmt --all -- --check
   cargo clippy --all-targets
   cargo test --all-targets
   ```
4. Commit your changes (`git commit -m 'feat: add amazing feature'`)
5. Push to the branch (`git push origin feature/amazing-feature`)
6. Open a Pull Request

---

## 📄 License

Distributed under the **MIT License**. See [`LICENSE`](LICENSE) for more information.

*Disclaimer: `agy-auth-rust` is an independent open-source project and is not affiliated with, sponsored by, or endorsed by Google LLC.*
