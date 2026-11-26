# Catatan perubahan

Semua perubahan penting Stackyard dicatat di file ini. Aplikasi menampilkannya
sebelum pembaruan dipasang (semua versi yang terlewat) dan sesudahnya
("Apa yang baru"), dan riwayat lengkapnya ada di Pengaturan.

Nomor versi memakai format `besar.medium.minor`
([Semantic Versioning](https://semver.org/lang/id/)):

- **besar** — perubahan besar yang bisa membuat data, konfigurasi, atau cara
  pakai lama tidak cocok lagi (`npm version major`).
- **medium** — fitur baru yang tetap cocok dengan versi sebelumnya
  (`npm version minor`).
- **minor** — perbaikan bug dan penyesuaian kecil (`npm version patch`).

Tulis setiap perubahan di bagian **Belum dirilis**, di bawah salah satu
kategori: `Ditambahkan`, `Diubah`, `Diperbaiki`, `Dihapus` (juga `Keamanan`
dan `Usang` bila perlu). `npm version …` memindahkan isinya ke versi baru
beserta tanggalnya.

## [Belum dirilis]

### Ditambahkan

- Catatan perubahan di aplikasi: daftar perubahan setiap versi tampil sebelum
  pembaruan dipasang, muncul sekali sebagai "Apa yang baru" setelah
  pembaruan, dan bisa dibuka kapan saja di Pengaturan → Riwayat perubahan.

## [0.4.0] - 2026-11-26

### Ditambahkan

- Projects di domain `.test`: setiap folder di `~/Stackyard/www` otomatis bisa
  dibuka di `nama-folder.test` lewat nginx atau Apache, dengan `public/`
  sebagai web root (Laravel, Symfony). Domain didaftarkan ke file hosts dengan
  satu kali izin administrator.
- Terminal bawaan dengan tab, PATH yang sudah berisi runtime dan tools
  Stackyard, dan bisa dibuka langsung di folder project.
- HTTPS lokal: tombol "Aktifkan HTTPS" membuat CA lokal yang hanya berlaku
  untuk `.test` dan `localhost`, sehingga project bisa dibuka lewat `https://`
  tanpa peringatan browser.
- Linux: nginx bisa memakai port 80 dan 443 setelah satu kali izin
  administrator, jadi alamat project tidak perlu `:8080` lagi.
- Versi PHP per project (8.4 atau 8.3); beberapa versi bisa berjalan
  bersamaan.
- PHP 8.4 dan 8.3 untuk Windows di Module Hub.

### Diubah

- nginx dan Apache yang terpasang dengan konfigurasi lama otomatis mendapat
  dukungan domain `.test` (konfigurasi lama disimpan sebagai cadangan), tanpa
  perlu install ulang.

## [0.3.0] - 2026-11-25

### Ditambahkan

- Module Node.js, Python, MariaDB, MongoDB, Redis, dan Apache di Module Hub.
- phpMyAdmin dan pgAdmin terpasang dan diperbarui otomatis bersama
  database-nya.
- Versi default Node.js dan Python langsung tersedia di PATH.
- Aplikasi hanya berjalan satu kali; membuka lagi akan menampilkan jendela
  yang sudah ada.

### Diubah

- Tampilan baru dengan palet warna dan ikon aplikasi yang diperbarui.

### Diperbaiki

- macOS: service gagal berjalan bila folder sementara (TMPDIR) terlalu
  panjang.
- pgAdmin memakai versi 9.18 karena paket 9.19 tidak menyertakan tampilan
  web.
- Ikon tombol stop dan ikon module yang tidak muncul.
