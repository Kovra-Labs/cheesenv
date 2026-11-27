# Catatan perubahan

Semua perubahan penting CheesEnv dicatat di sini, dengan terjemahan bahasa
Inggris di [`CHANGELOG.en.md`](CHANGELOG.en.md). Aplikasi menampilkan versi
yang sesuai bahasa di Pengaturan: sebelum pembaruan dipasang, sekali sesudahnya
("Apa yang baru"), dan kapan saja di Pengaturan → Riwayat perubahan.

Nomor versi memakai format `besar.medium.minor`
([Semantic Versioning](https://semver.org/lang/id/)):

- **besar**: perubahan yang membuat data, konfigurasi, atau cara pakai lama
  tidak cocok lagi (`npm version major`).
- **medium**: fitur baru yang tetap cocok dengan versi sebelumnya
  (`npm version minor`).
- **minor**: perbaikan bug dan penyesuaian kecil (`npm version patch`).

Cara menulis:

- Catat di bagian **Belum dirilis** di kedua file, dengan kategori
  `Ditambahkan`, `Diubah`, `Diperbaiki`, atau `Dihapus` (juga `Keamanan` dan
  `Usang` bila perlu). Di `CHANGELOG.en.md` kategorinya `Added`, `Changed`,
  `Fixed`, `Removed`, `Security`, `Deprecated`.
- Satu poin = satu perubahan yang terasa oleh pengguna, satu kalimat pendek,
  tanpa detail teknis.
- `npm version …` memindahkan isinya ke versi baru beserta tanggalnya.

## [Belum dirilis]

## [0.1.0] - 2026-11-27

### Ditambahkan

- Rilis pertama CheesEnv: kelola web server, runtime, dan database lokal di
  Windows, macOS, dan Linux.
- Module Hub: nginx, Apache, PHP, Node.js, Python, MySQL, MariaDB,
  PostgreSQL, MongoDB, dan Redis, plus phpMyAdmin dan pgAdmin yang terpasang
  otomatis bersama database-nya.
- Projects: setiap folder di `~/CheesEnv/www` langsung bisa dibuka di
  `nama-folder.test`, dengan versi PHP per project.
- HTTPS lokal untuk domain `.test` tanpa peringatan browser.
- Linux: project bisa dibuka tanpa `:8080` (port 80 dan 443).
- Terminal bawaan dengan tab; Node.js dan Python versi default juga bisa
  dipakai dari terminal biasa.
- Ikon di tray sistem; menutup jendela membiarkan service tetap berjalan.
- Pembaruan otomatis dengan catatan perubahan: lihat isinya sebelum memasang,
  ringkasan "Apa yang baru" setelahnya, dan riwayat lengkap di Pengaturan.
