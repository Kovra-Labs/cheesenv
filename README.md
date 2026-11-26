# Stackyard

A cross-platform desktop app (Windows, macOS, Linux) for running a local development environment — web servers, runtimes, databases, and admin panels — with every module downloaded from a single signed registry. Free and open source (MIT).

> Status: **v0.3** — see `docs/PRD Stackyard.pdf` for the full roadmap.

## Contents

- [Features](#features)
- [Available modules](#available-modules)
- [Install](#install)
- [Development](#development)
- [Building installers](#building-installers)
  - [Linux](#linux)
  - [Windows](#windows)
  - [macOS](#macos)
  - [Building for every platform (CI)](#building-for-every-platform-ci)
- [Releases & auto-update](#releases--auto-update)
- [Registry](#registry)
  - [Keeping modules up to date](#keeping-modules-up-to-date)
- [Project structure](#project-structure)

## Features

- **Module Hub** — browse, search, filter by category, and install/uninstall per version (multiple versions side by side).
- **Secure registry** — `index.json` is verified with minisign (public key built into the app), each module manifest against the SHA-256 in the index, and every binary against its own SHA-256. Downloads are resumable.
- **Safe extraction** — rejects `../`, absolute paths, and symlinks that point outside the module folder.
- **Process supervisor** — start/stop/restart one or all services, real-time status, crash detection with automatic restart (max 3× per minute), and autostart when the app opens.
- **Port conflict detection** with a suggested free port. Every service binds to `127.0.0.1`.
- **Config from templates** — files edited by the user are backed up (`*.bak-<time>`) before being overwritten.
- **Projects on `.test` domains** — every folder in `~/Stackyard/www` is served at `<folder>.test` by nginx and Apache (`public/` is used as web root when present, `.php` goes to PHP-FPM). Stackyard adds the domains to the hosts file after one administrator prompt.
- **Built-in terminal** — tabs with a shell whose `PATH` already starts with the default runtimes and the installed modules' tools (`mysql`, `psql`, `redis-cli`, …); open one straight in a project folder.
- **Runtimes on your PATH** — make one Node.js or Python version the default and use `node`, `npm`, `python`, `pip` straight from your terminal.
- **Log viewer** with filtering and live tail.
- **System tray** — closing the window keeps services running; quitting from the tray stops them all. Only one instance runs at a time: launching the app again brings the existing window back.
- **UI** in English and Indonesian, with light, dark, and system themes.

## Available modules

| Module | Category | Versions | Platforms | Source |
|---|---|---|---|---|
| nginx | Web server | 1.30.5 | Windows, macOS, Linux (x64/arm64) | jirutka/nginx-binaries |
| Apache HTTP Server | Web server | 2.4 | Windows; Linux & macOS (x64/arm64) once built by CI¹ | Apache Lounge (Windows), Stackyard build |
| PHP-FPM | Runtime | 8.4, 8.3 | macOS, Linux | static-php-cli |
| Node.js | Runtime | 26 LTS, 24 LTS | all platforms | nodejs.org |
| Python | Runtime | 3.15, 3.14 | all platforms | python-build-standalone |
| MySQL | Database | 8.4 LTS | Linux x64, macOS 15+, Windows | mysql.com |
| MariaDB | Database | 12.3 LTS, 11.8 LTS | Linux x64, Windows | mariadb.org |
| PostgreSQL | Database | 18, 17 | all platforms | theseus-rs/postgresql-binaries |
| MongoDB | Database | 9.0, 8.0 (+ `mongosh`) | all platforms (Linux: glibc 2.35+, e.g. Ubuntu 22.04+) | mongodb.org |
| Redis | Database | 8.10 | Windows; Linux & macOS (x64/arm64) once built by CI¹ | redis-windows (Windows), Stackyard build |
| phpMyAdmin (for MySQL & MariaDB) | Panel | 5.2 | all platforms (bundled static PHP CLI) | phpmyadmin.net + static-php-cli |
| pgAdmin 4 | Panel | 9.19 | Linux, macOS arm64, Windows (no macOS Intel: `cryptography` ships no wheel for it) | PyPI (hash-pinned) |

¹ Redis and Apache publish no official Linux/macOS binaries. The **Binaries** workflow builds them from the verified official source; see [Keeping modules up to date](#keeping-modules-up-to-date).

Default ports (Linux/macOS add 8000 to ports below 1024): nginx 80 → 8080, Apache 82 → 8082, phpMyAdmin 8081, pgAdmin 5050, PHP-FPM 9000, MySQL 3306, MariaDB 3307, PostgreSQL 5432, MongoDB 27017, Redis 6379. Apache serves `~/Stackyard/www`, honours `.htaccess`, and passes `*.php` to PHP-FPM on port 9000. On Windows, Apache needs the [Visual C++ Redistributable](https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist).

MySQL, MariaDB, and PostgreSQL are initialised on first start; the root password is randomly generated and stored in the OS keychain (see **Credentials** on the Services page). MongoDB and Redis run without authentication, like their default installs, and only listen on `127.0.0.1`.

### Projects and `.test` domains

Every folder in `~/Stackyard/www` is a project, served at `<folder>.test` (folder names must be lowercase letters, digits, and dashes). Create one with **Projects → New project**, or just add or symlink a folder there; when the folder has a `public/` subfolder (Laravel, Symfony, …) that becomes the web root, and every other URL falls back to `index.php` when PHP is installed.

- **Domains** — `.test` names resolve through the system hosts file. **Register domains** writes them in a block marked `# BEGIN Stackyard` … `# END Stackyard` after one administrator prompt (pkexec on Linux, the macOS password dialog, UAC on Windows); nothing outside that block is changed, and the first change keeps a copy as `hosts.stackyard-backup`. The page also shows the lines to add by hand.
- **Web servers** — Stackyard writes `etc/<server>/<version>/vhosts/stackyard-projects.conf` and restarts a running nginx or Apache when the project list changes. Your own `*.conf` files in that `vhosts` folder are included too. An nginx or Apache installed from an older registry template lacks that include (the Projects page says so): uninstall and reinstall it from the Module Hub (data is kept).
- **Ports** — on Linux and macOS nginx listens on 8080, so projects open at `http://<folder>.test:8080/` until privileged ports are supported.

### Built-in terminal

**Terminal** opens tabs running your shell (`$SHELL` on Linux/macOS, PowerShell on Windows) in `www/` or in a project folder (**Terminal** on a project card). Its `PATH` starts with `~/Stackyard/bin` and the newest installed version of every module, so `node`, `python`, `mysql`, `psql`, `mongosh`, `redis-cli`, … work without any PATH setup. Shells keep running while you browse other pages. Copy with <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>C</kbd>, paste with <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>V</kbd>.

### Using Node.js and Python from the terminal

Installing the first version of a runtime makes it the **default**; switch with **Make default** in the Module Hub. The default version's commands (`node`, `npm`, `npx`, `python`, `python3`, `pip`, `pip3`) are placed in `~/Stackyard/bin` (symlinks on Linux/macOS, `.cmd` launchers on Windows). Add that folder to your `PATH` once — **Settings → Terminal (PATH)** shows the exact folder and a ready-to-copy line:

```bash
export PATH="$HOME/Stackyard/bin:$PATH"          # bash/zsh: ~/.bashrc or ~/.zshrc
fish_add_path ~/Stackyard/bin                    # fish
```

```powershell
[Environment]::SetEnvironmentVariable('Path', "$HOME\Stackyard\bin;" + [Environment]::GetEnvironmentVariable('Path', 'User'), 'User')
```

**Web panels are built in.** phpMyAdmin (for MySQL & MariaDB) and pgAdmin (for PostgreSQL) are not listed in the Module Hub: Stackyard downloads them in the background when it starts (the sidebar shows the progress) and updates them to the newest registry version on every start, keeping their port and data. They appear inside the card of a database (open, stop, and change the panel port); **Open** is available while the database runs. Installing a database still installs its panel if the background download has not finished or failed.

Once phpMyAdmin or pgAdmin is installed, a running database card shows an **Open phpmyadmin / Open pgadmin** button. The panel runs on `127.0.0.1`, its server list is filled in automatically from the installed database services (without passwords), and it opens in your browser. Log in with the user and password from **Credentials**. Installing pgAdmin pulls ~110 hash-pinned Python packages, so it needs internet access and takes a few minutes.

## Install

Download the installer for your OS from the [Releases page](https://github.com/pipsyrs/stackyard/releases/latest):

| OS | File | Install |
|---|---|---|
| Windows | `Stackyard_<version>_x64-setup.exe` (or `.msi`) | Run it |
| macOS | `Stackyard_<version>_universal.dmg` | Open it and drag Stackyard into Applications |
| Debian / Ubuntu | `Stackyard_<version>_amd64.deb` | `sudo apt install ./Stackyard_<version>_amd64.deb` |
| Fedora / RHEL / openSUSE | `Stackyard-<version>-1.x86_64.rpm` | `sudo dnf install ./Stackyard-<version>-1.x86_64.rpm` |
| Any Linux | `Stackyard_<version>_amd64.AppImage` | `chmod +x Stackyard_*.AppImage && ./Stackyard_*.AppImage` |

> The installers are not code-signed with an OS certificate yet. On **Windows**, SmartScreen may say "Windows protected your PC" → click **More info → Run anyway**. On **macOS**, Gatekeeper blocks the first launch → right-click the app → **Open**, or run `xattr -cr /Applications/Stackyard.app`.

## Development

### Prerequisites

| Tool | Version | Notes |
|---|---|---|
| [Node.js](https://nodejs.org/) | 22 LTS (20.19+ works) | |
| [Rust](https://rustup.rs/) | stable, 1.82+ | install with `rustup` |
| OS build dependencies | — | see [Linux](#linux), [Windows](#windows), or [macOS](#macos) below |

### Commands

```bash
npm install
npm run tauri dev        # run the app with hot reload
npm run build            # type-check + build the frontend
cd src-tauri && cargo test
```

End-to-end test against real binaries (needs internet):

```bash
npm run registry build
(cd registry/dist && python3 -m http.server 8765 --bind 127.0.0.1 &)
cd src-tauri && STACKYARD_E2E_REGISTRY=http://127.0.0.1:8765 cargo test e2e -- --ignored
```

To point the app at that local registry: `STACKYARD_REGISTRY_URL=http://127.0.0.1:8765 npm run tauri dev` (plain HTTP is only allowed for loopback).

## Building installers

**Installers must be built on the OS they target.** Tauri uses each platform's native webview and toolchain, so a Linux machine builds Linux packages, Windows builds Windows installers, and macOS builds macOS bundles. Cross-compiling is not supported for this project — use [CI](#building-for-every-platform-ci) to get all three at once.

| Target | Build on | Output |
|---|---|---|
| Linux | Linux | `.deb`, `.rpm`, `.AppImage` |
| Windows | Windows | `.exe` (NSIS), `.msi` |
| macOS | macOS | `.app`, `.dmg` |

### Updater signing key

Release builds also produce signed update artifacts (`.sig` files and the data for `latest.json`), so `tauri build` needs the updater private key:

**Maintainers** — the key lives in `~/.tauri/` (created once; **never commit it, keep an offline backup**). Export it before building:

```bash
# Linux / macOS
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/stackyard-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat ~/.tauri/stackyard-updater.password)"
```

```powershell
# Windows (PowerShell)
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content "$HOME\.tauri\stackyard-updater.key" -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = Get-Content "$HOME\.tauri\stackyard-updater.password" -Raw
```

**Contributors and forks** — skip the key and turn update artifacts off for your build. Installers still work; they just cannot be auto-updated:

```bash
npm run tauri build -- --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

`--config` also accepts a file path, which avoids shell quoting differences (handy on Windows PowerShell): save `{"bundle":{"createUpdaterArtifacts":false}}` as `no-updater.json` and run `npm run tauri build -- --config no-updater.json`.

To ship your own updates from a fork, generate a key pair with `npm run tauri signer generate -- -w ~/.tauri/stackyard-updater.key` and put the new public key in `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`.

### Linux

**1. System dependencies**

```bash
# Debian / Ubuntu
sudo apt update
sudo apt install -y build-essential curl wget file libssl-dev libxdo-dev \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev

# Fedora
sudo dnf install -y webkit2gtk4.1-devel openssl-devel curl wget file \
  libappindicator-gtk3-devel librsvg2-devel libxdo-devel
sudo dnf group install -y "c-development"

# Arch
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl \
  appmenu-gtk-module libappindicator-gtk3 librsvg xdotool
```

**2. Build**

```bash
npm install
npm run tauri build                          # .deb + .rpm + .AppImage
npm run tauri build -- --bundles deb         # one format only: deb | rpm | appimage
```

**3. Output** — `src-tauri/target/release/bundle/`

| Folder | File |
|---|---|
| `deb/` | `Stackyard_<version>_amd64.deb` |
| `rpm/` | `Stackyard-<version>-1.x86_64.rpm` |
| `appimage/` | `Stackyard_<version>_amd64.AppImage` |

**4. Install** — quit any running copy first (tray icon → **Quit**); otherwise launching the new build only brings the old window back.

```bash
sudo apt install ./src-tauri/target/release/bundle/deb/Stackyard_<version>_amd64.deb
```

Notes:
- Building the AppImage downloads `linuxdeploy` the first time, so it needs internet access.
- For maximum compatibility, build on the oldest distro you want to support (CI uses Ubuntu 22.04). A package built on a newer glibc will not run on older distros.
- ARM64 packages are built the same way on an ARM64 Linux machine.

### Windows

**1. Prerequisites**

- [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) — select the **Desktop development with C++** workload.
- WebView2 — already included in Windows 10 (1803+) and Windows 11.
- Rust via [rustup](https://rustup.rs/), using the default MSVC toolchain (`stable-x86_64-pc-windows-msvc`).
- Node.js 22 LTS.
- For the `.msi` only: the **VBSCRIPT** optional feature must be enabled (Settings → System → Optional features). The NSIS `.exe` does not need it.

**2. Build** (PowerShell)

```powershell
npm install
npm run tauri build                          # .exe (NSIS) + .msi
npm run tauri build -- --bundles nsis        # one format only: nsis | msi
```

**3. Output** — `src-tauri\target\release\bundle\`

| Folder | File |
|---|---|
| `nsis\` | `Stackyard_<version>_x64-setup.exe` |
| `msi\` | `Stackyard_<version>_x64_en-US.msi` |

**4. Install** — run the `.exe` (recommended; supports per-user install) or the `.msi`.

For Windows on ARM, build on an ARM64 machine with `rustup target add aarch64-pc-windows-msvc` and `npm run tauri build -- --target aarch64-pc-windows-msvc`.

### macOS

**1. Prerequisites**

```bash
xcode-select --install                                    # Xcode Command Line Tools
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin  # for universal builds
```

Plus Node.js 22 LTS (e.g. `brew install node@22`).

**2. Build**

```bash
npm install
npm run tauri build -- --target universal-apple-darwin   # one app for Apple Silicon + Intel
npm run tauri build                                      # current architecture only (faster)
```

**3. Output**

| Build | Folder |
|---|---|
| Universal | `src-tauri/target/universal-apple-darwin/release/bundle/` |
| Current architecture | `src-tauri/target/release/bundle/` |

Inside it: `dmg/Stackyard_<version>_<arch>.dmg`, `macos/Stackyard.app`, and `macos/Stackyard.app.tar.gz` (update artifact).

**4. Install** — open the `.dmg` and drag Stackyard into Applications. Unsigned builds need the Gatekeeper workaround from [Install](#install). Signing and notarization require an Apple Developer ID; see the [Tauri guide](https://v2.tauri.app/distribute/sign/macos/).

### Building for every platform (CI)

You don't need three machines: the **Release** workflow (`.github/workflows/release.yml`) builds Linux (Ubuntu 22.04), Windows (x64), and macOS (universal) on GitHub-hosted runners whenever a version tag is pushed. See [Releases & auto-update](#releases--auto-update) for the steps.

The **CI** workflow (`.github/workflows/ci.yml`) runs on every push and pull request: it type-checks the frontend, runs `cargo test`, and does a smoke build on all three operating systems.

### Building against your own registry

The registry URL is baked in at build time (see [Registry](#registry)). To ship a build that uses your own server:

```bash
STACKYARD_REGISTRY_URL=https://registry.example.com npm run tauri build
```

```powershell
$env:STACKYARD_REGISTRY_URL = "https://registry.example.com"; npm run tauri build
```

The build fails if the URL is not `https://` (plain `http://` is only allowed for `localhost` / `127.0.0.1` / `[::1]`).

## Releases & auto-update

Installed apps check `latest.json` on GitHub Releases at startup (can be turned off in Settings), then download the update, verify its signature, install it, and relaunch.

**One-time setup** on GitHub (repo `pipsyrs/stackyard`, matching the endpoint in `src-tauri/tauri.conf.json`): go to **Settings → Secrets and variables → Actions** and add:

- `TAURI_SIGNING_PRIVATE_KEY` — contents of `~/.tauri/stackyard-updater.key`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — contents of `~/.tauri/stackyard-updater.password`

**Every release:**

```bash
git status                 # commit everything first: CI builds the tagged commit
npm version 0.3.0          # bumps package.json + Cargo.toml, commits, and tags v0.3.0
git push --follow-tags     # triggers the Release workflow (Windows, macOS, Linux)
```

The workflow (about 15–25 minutes) creates a **draft release** with every installer plus `latest.json`. Review it, write the release notes, then click **Publish** — installed apps see the new version from that moment.

For a local build without tagging, bump the version only: `npm version 0.3.0 --no-git-tag-version`. Always bump the version before a new build; two builds with the same version number are hard to tell apart.

> If the updater key is lost, installed apps can no longer receive updates and must be reinstalled manually. Keep the key and its password in a password manager.

## Registry

Module manifests live in `registry/modules/*.json`. Template placeholders: `{{bin}}`, `{{prefix}}`, `{{conf}}`, `{{data}}`, `{{run}}`, `{{logs}}`, `{{www}}`, `{{port}}`, `{{name}}`, `{{version}}`, and `{{init}}` (a temporary folder holding the `files` of `init`/`setup`). Service commands are argument arrays (never run through a shell) and may only execute binaries inside the module's own folder.

```bash
npm run registry keygen   # once; the private key goes to ~/.stackyard-registry/ (NEVER commit it)
npm run registry hash     # fill in empty sha256 fields by downloading the assets
npm run registry build    # write registry/dist/ (index.json + .minisig + modules/)
python3.12 registry/pylock.py pgadmin4==9.19 --python 3.12 --module pgadmin  # pin pip dependencies (hashes) per platform
```

Additional manifest fields:

- `assets.<platform>.extra[]` — extra archives (`url`, `sha256`, `format`, `strip`, `dest`) extracted into a sub-folder of the module.
- `module_files` — files rendered into the module folder at install time (relative paths).
- `setup` — `files` + `commands` (`commands_windows`) run once at install time from inside the module folder (e.g. `pip install --require-hashes`). The module only counts as installed if setup succeeds.
- `shims` / `shims_windows` — `{ "<command>": "<path inside the module>" }`: the module is a runtime whose default version exposes these commands in `~/Stackyard/bin`.
- `companions` — `["<module>", …]`: installed along with this module when missing (e.g. `phpmyadmin` for `mysql`).
- `panel` — `{ "for": ["<module>", …], "path": "/..." }`: the module is a web panel for another module. Before each start, Stackyard writes `{{run}}/panel.json` (port, a random per-start token, and the target services without passwords); `path` may use `{{token}}`.

`registry/dist/` is committed, and the **Registry** workflow (`.github/workflows/registry.yml`) publishes it to GitHub Pages whenever that folder changes on `main` (default URL: `https://pipsyrs.github.io/stackyard`). One-time setup: **Settings → Pages → Source: GitHub Actions**. Signatures can be checked with `minisign -Vm index.json -p src-tauri/registry.pub`.

Users cannot change the registry URL; it is fixed at build time (default: the GitHub Pages URL above). The registry can be hosted on any static HTTPS server (Nginx, S3/R2, a CDN): serve the contents of `registry/dist/` as-is. Security relies on the minisign signature, not on the host. See [Building against your own registry](#building-against-your-own-registry). The app's update endpoint is configured separately in `src-tauri/tauri.conf.json` → `plugins.updater.endpoints`.

### Keeping modules up to date

Modules do not update themselves: the registry is static and signed. `registry/upstream.mjs` refreshes the versions of Node.js, Python, MariaDB, MongoDB, Redis, Apache, phpMyAdmin, and pgAdmin from their official sources (two newest supported lines each, one for Apache/phpMyAdmin/pgAdmin; URLs and SHA-256 sums taken from the upstream checksum files). A pgAdmin release is only taken once its wheel is checked to ship the built web UI (the 9.19 wheel on PyPI does not, so 9.18 is kept). A new pgAdmin release also relocks its pip packages with `registry/pylock.py`, which needs `python3.12` (or set `PYTHON`):

```bash
npm run registry:upstream check     # show what would change
npm run registry:upstream update    # rewrite registry/modules/*.json
npm run registry build              # sign locally, then commit registry/dist
```

The **Registry update** workflow (`.github/workflows/registry-update.yml`) runs this every Monday and opens a pull request (`registry/upstream` branch) when something changed. When a Redis or Apache version has no Linux/macOS build yet, it starts the **Binaries** workflow (`.github/workflows/binaries.yml`), which runs `registry/build/<module>.sh` on Linux x64/arm64 and macOS x64/arm64 runners, uploads the archives to a pre-release named `<module>-<version>`, and triggers the update again so the pull request picks them up. The signing key never leaves your machine: after merging, run `npm run registry build` and push `registry/dist`.

One-time setup: **Settings → Actions → General → Workflow permissions →** allow GitHub Actions to **create and approve pull requests**. You can also start a build by hand: **Actions → Binaries → Run workflow** (module + version).

## Project structure

```
src/                 Vue 3 + TypeScript frontend (views, components, i18n, store)
src-tauri/src/       Rust core
  commands.rs        IPC commands (the only entry point from the UI)
  supervisor.rs      Service processes, crash detection, auto-restart
  registry.rs        Index/manifest + minisign & SHA-256 verification
  download.rs        Resumable downloads + verification
  extract.rs         Safe zip/tar extraction
  modules.rs         Install/uninstall, config templates, setup
  panel.rs           Web panels (phpMyAdmin/pgAdmin): panel.json, tokenised URLs
  runtimes.rs        Default Node.js/Python versions: shims in ~/Stackyard/bin
  projects.rs        www/ folders → <name>.test vhosts for nginx/Apache
  hosts.rs           Managed hosts-file block + elevated helper (--hosts-sync)
  terminal.rs        Built-in terminal: shells in a PTY with Stackyard's PATH
  ports.rs logs.rs settings.rs paths.rs tray.rs updater.rs
registry/            Module manifests, signing tool (tool.mjs), upstream checker (upstream.mjs)
  build/             Build scripts for Redis/Apache (used by the Binaries workflow)
```

User data: `~/Stackyard/` → `bin/`, `modules/`, `etc/`, `data/`, `www/`, `logs/`, `certs/`, `cache/`.
