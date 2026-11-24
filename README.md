# Stackyard

Aplikasi desktop lintas platform (Windows, macOS, Linux) untuk mengelola environment development lokal: web server, runtime, database, dan layanan pendukung — semua module diunduh dari satu registry bertanda tangan. Gratis dan open source (MIT).

> Status: **v0.1 (Core)** — lihat `docs/PRD Stackyard.pdf` untuk roadmap lengkap.

## Fitur v0.1

- Pusat module: jelajah, cari, filter kategori, install/uninstall per versi (multi-versi berdampingan).
- Registry aman: `index.json` diverifikasi minisign (kunci publik tertanam), manifest module diverifikasi SHA-256 dari index, setiap binary diverifikasi SHA-256; unduhan bisa di-resume.
- Ekstraksi arsip aman (tolak `../`, path absolut, symlink keluar folder).
- Process supervisor: start/stop/restart per service atau semua, status real-time, deteksi crash + restart otomatis (maks. 3×/menit), auto-start saat aplikasi dibuka.
- Deteksi bentrok port + saran port alternatif. Semua service bind `127.0.0.1`.
- Config dibuat dari template; file yang diubah pengguna dicadangkan (`*.bak-<waktu>`) sebelum ditimpa.
- Log viewer dengan filter dan tail real-time.
- System tray; menutup jendela tidak mematikan service. Keluar dari tray menghentikan semua service.
- UI Bahasa Indonesia & Inggris, tema terang/gelap/sistem.

Module di registry contoh: **nginx 1.30.5** (Windows/macOS/Linux, x64/arm64), **PHP-FPM 8.4 / 8.3** (macOS/Linux), **MySQL 8.4 LTS** (Linux x64, macOS 15+, Windows) dan **PostgreSQL 18 / 17** (semua platform). Database diinisialisasi otomatis saat start pertama; password root dibuat acak dan disimpan di keychain OS (lihat tombol **Kredensial** di halaman Service).

## Pengembangan

Prasyarat: Node 20+, Rust stable, dan [dependency sistem Tauri](https://v2.tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev        # jalankan aplikasi (hot reload)
npm run build            # typecheck + build frontend
cd src-tauri && cargo test
```

Uji end-to-end dengan binary asli (butuh internet):

```bash
npm run registry build
(cd registry/dist && python3 -m http.server 8765 --bind 127.0.0.1 &)
cd src-tauri && STACKYARD_E2E_REGISTRY=http://127.0.0.1:8765 cargo test e2e -- --ignored
```

Untuk mencoba registry lokal di aplikasi, isi **Pengaturan → URL registry** dengan `http://127.0.0.1:8765` (HTTP hanya diizinkan untuk loopback).

## Build installer

Kunci tanda tangan update ada di `~/.tauri/` (dibuat sekali; **jangan di-commit, cadangkan offline**).

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/stackyard-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat ~/.tauri/stackyard-updater.password)"
npm run tauri build                         # semua format untuk OS saat ini
npm run tauri build -- --bundles appimage   # hanya satu format
```

Hasil ada di `src-tauri/target/release/bundle/`:

| OS | Installer | Auto-update |
|---|---|---|
| Linux | `.AppImage` | Ya |
| Linux | `.deb`, `.rpm` | Tidak (update lewat paket baru) |
| Windows | `.exe` (NSIS), `.msi` | Ya |
| macOS | `.dmg` universal, `.app.tar.gz` | Ya |

Installer Windows/macOS dibuat di CI (tidak bisa di-build dari Linux).

## Rilis & update otomatis

Aplikasi yang sudah terpasang memeriksa `latest.json` di GitHub Releases saat dibuka (bisa dimatikan di Pengaturan), lalu mengunduh, memverifikasi tanda tangan, memasang, dan membuka ulang dirinya sendiri.

Sekali saja di GitHub (repo `stackyard/stackyard`, sesuai endpoint di `src-tauri/tauri.conf.json`):
**Settings → Secrets and variables → Actions**, tambahkan:
- `TAURI_SIGNING_PRIVATE_KEY` = isi `~/.tauri/stackyard-updater.key`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = isi `~/.tauri/stackyard-updater.password`

Setiap rilis:

```bash
npm version 0.2.0          # ubah versi di package.json + Cargo.toml, commit, dan tag v0.2.0
git push --follow-tags     # workflow Release mem-build Windows, macOS, Linux
```

Workflow membuat **draft release** berisi installer + `latest.json`. Periksa, isi catatan rilis, lalu klik **Publish**: saat itu juga semua aplikasi terpasang akan melihat versi baru.

> Kalau kunci update hilang, aplikasi yang sudah terpasang tidak bisa menerima update lagi (harus instal ulang manual). Simpan kunci + password di password manager.

## Registry

Sumber manifest ada di `registry/modules/*.json`. Placeholder template: `{{bin}}`, `{{prefix}}`, `{{conf}}`, `{{data}}`, `{{run}}`, `{{logs}}`, `{{www}}`, `{{port}}`, `{{name}}`, `{{version}}`. Perintah service ditulis sebagai array argumen (tidak pernah lewat shell) dan hanya boleh menjalankan executable di dalam folder module itu sendiri.

```bash
npm run registry keygen   # sekali saja; kunci privat di ~/.stackyard-registry/ (JANGAN di-commit)
npm run registry hash     # isi sha256 yang kosong dengan mengunduh asset
npm run registry build    # tulis registry/dist/ (index.json + .minisig + modules/)
```

Isi `registry/dist/` dipublikasikan ke GitHub Pages (URL default: `https://stackyard.github.io/registry`). Tanda tangan kompatibel dengan `minisign -Vm index.json -p src-tauri/registry.pub`.

## Struktur

```
src/                 Frontend Vue 3 + TypeScript (views, i18n, store)
src-tauri/src/       Core Rust
  commands.rs        Command IPC (satu-satunya pintu dari UI)
  supervisor.rs      Proses service, crash detection, auto-restart
  registry.rs        Index/manifest + verifikasi minisign & SHA-256
  download.rs        Unduhan dengan resume + verifikasi
  extract.rs         Ekstraksi zip/tar.gz yang aman
  modules.rs         Install/uninstall, template konfigurasi
  ports.rs logs.rs settings.rs paths.rs tray.rs
registry/            Manifest module + tool tanda tangan
```

Data pengguna: `~/Stackyard/` → `modules/`, `etc/`, `data/`, `www/`, `logs/`, `certs/`, `cache/`.
