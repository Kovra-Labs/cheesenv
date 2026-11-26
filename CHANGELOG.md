# Catatan perubahan

Semua perubahan penting Stackyard dicatat di sini, dengan terjemahan bahasa
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

## [0.5.2] - 2026-11-26

### Diubah

- Logo aplikasi baru, termasuk ikon di tray sistem.

## [0.5.1] - 2026-11-26

### Diperbaiki

- Catatan perubahan kini mengikuti bahasa aplikasi dan lebih ringkas.

## [0.5.0] - 2026-11-26

### Ditambahkan

- Catatan perubahan: lihat isi pembaruan sebelum memasangnya, ringkasan "Apa
  yang baru" setelah update, dan riwayat lengkap di Pengaturan.

## [0.4.0] - 2026-11-26

### Ditambahkan

- Projects: setiap folder di `~/Stackyard/www` langsung bisa dibuka di
  `nama-folder.test`.
- Terminal bawaan dengan tab, siap memakai runtime dan tools Stackyard.
- HTTPS lokal untuk domain `.test` tanpa peringatan browser.
- Linux: project bisa dibuka tanpa `:8080` (port 80 dan 443).
- Pilih versi PHP per project (8.4 atau 8.3).
- PHP 8.4 dan 8.3 untuk Windows di Module Hub.

### Diubah

- nginx dan Apache yang sudah terpasang otomatis mendukung domain `.test`,
  tanpa install ulang.

## [0.3.0] - 2026-11-25

### Ditambahkan

- Module Node.js, Python, MariaDB, MongoDB, Redis, dan Apache di Module Hub.
- phpMyAdmin dan pgAdmin terpasang otomatis bersama database-nya.
- Node.js dan Python versi default langsung bisa dipakai dari terminal.
- Stackyard hanya berjalan satu kali; membukanya lagi menampilkan jendela yang
  sudah ada.

### Diubah

- Tampilan, warna, dan ikon aplikasi yang baru.

### Diperbaiki

- macOS: service gagal berjalan di beberapa komputer.
- pgAdmin tidak menampilkan halaman web.
- Ikon tombol stop dan ikon module yang tidak muncul.
