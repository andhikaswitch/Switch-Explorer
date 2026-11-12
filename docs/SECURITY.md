# Keamanan & Keselamatan Data

## 1. Model ancaman

| Aset | Ancaman | Mitigasi utama |
|---|---|---|
| File pengguna | Hilang/rusak karena bug operasi, drag tidak sengaja, crash di tengah operasi | Recycle Bin default, journal write-ahead, undo, verifikasi, Drag Guard, konfirmasi |
| Token Google | Dicuri dari disk/log | Credential Manager, access token hanya di memori, redaksi log |
| Aplikasi lain | Crash karena shim picker | Proxy + fallback, `catch_unwind`, kill switch, default mati |
| Sistem pengguna | Registry tertinggal/rusak | HKCU saja, backup, cleanup, status & perbaikan |
| Pipe IPC | Proses lain menyamar sebagai server/klien | DACL per-user, tolak klien remote, verifikasi path image |
| File tema/settings | File berbahaya dari internet | Data murni (tanpa script), batas ukuran & kedalaman, validasi |
| Rantai pasok | Dependency berbahaya | Versi dipin, `cargo-deny`, `cargo-audit`, review dependency baru |

## 2. Aturan operasi file

1. Delete default ke Recycle Bin; hapus permanen hanya dengan konfirmasi eksplisit.
2. Tidak pernah menimpa tanpa keputusan pengguna (sheet konflik).
3. Journal ditulis **sebelum** dan **sesudah** setiap item (write-ahead); crash → pemulihan saat start.
4. Move antar volume = copy → verifikasi (ukuran; hash bila `ops.verify_copy`) → hapus sumber. Tidak ada
   penghapusan sumber sebelum salinan lengkap.
5. Pindah ke Drive: file lokal hanya dibuang setelah checksum remote cocok
   (`cloud.gdrive.verify_before_delete`, hanya bisa dimatikan di Developer Mode).
6. Validasi nama (karakter terlarang, nama reserved, panjang) dan cegah path traversal pada Bulk Rename &
   Organizer (template tidak boleh menghasilkan `..` atau path absolut).
7. Junction/symlink: tidak diikuti secara rekursif kecuali diminta; deteksi loop.
8. TOCTOU: periksa ulang keberadaan/ukuran tepat sebelum eksekusi; gagal → laporkan, jangan menebak.
9. `safety.readonly_mode` dan `safety.protected_paths` diperiksa di lapisan `sx-ops`, bukan hanya di UI.

## 3. Shim picker (berjalan di proses aplikasi lain)

- Dependensi minimal (`sx-ipc`, `windows`, `serde`); tanpa GPUI/tokio; CRT statis.
- Setiap entry point COM dibungkus `catch_unwind`; panic → kembalikan `E_FAIL` lalu fallback ke dialog native.
  Profil build memakai `panic = "unwind"` agar `catch_unwind` bekerja.
- Timeout di setiap tunggu; tidak pernah memblokir aplikasi host tanpa pompa pesan.
- Protokol berversi; versi tidak cocok → dialog native.
- Verifikasi server pipe (path image harus lokasi instalasi) sebelum mengirim/menerima data.
- Penghitung kegagalan & bypass otomatis (SYSTEM-INTEGRATION.md §14).

## 4. Registry

- Hanya HKCU; semua tulis lewat `sx_integration::registry` yang membackup nilai lama.
- Setiap integrasi punya unregister + tercakup cleanup; status ditampilkan di Settings → Integrasi.
- Key Win+E dan override COM dikenal sebagai teknik persistensi malware → antivirus bisa curiga. Mitigasi:
  opt-in dengan penjelasan, binary & DLL ditandatangani, tidak ada perilaku tersembunyi.

## 5. OAuth & token

- PKCE (S256) + `state` acak diverifikasi; loopback hanya `127.0.0.1`, port acak, satu permintaan, timeout.
- Refresh token di Windows Credential Manager; access token di memori; tidak pernah ditulis ke file/log.
- Layer redaksi `tracing` menyaring header `Authorization`, parameter `code`, `access_token`,
  `refresh_token`, dan URI sesi upload.
- Disconnect = revoke + hapus kredensial. Client secret tidak di-commit.

## 6. IPC

- Pipe dengan DACL hanya SID pengguna (+ SYSTEM), `PIPE_REJECT_REMOTE_CLIENTS`.
- Siapa pun bisa membuat pipe dengan nama kita lebih dulu (squatting). Klien hanya bicara dengan server
  yang SID, session, elevasi, dan exe-nya sama dengan klien. Pipe dibuka dengan `SECURITY_IDENTIFICATION`
  supaya server tidak bisa meng-impersonate klien (ARCHITECTURE.md §12).
- Pesan maks 1 MB, JSON tervalidasi, tenggat per pesan; path dari pesan dinormalisasi & divalidasi sebelum
  dipakai. `OpenPaths` hanya menerima path absolut, maksimum 64 per pesan.
- Hanya aksi yang aman yang tersedia lewat IPC (buka path, aktifkan window, picker, upload dialog).

## 7. Parsing file konfigurasi

- Tema/settings/keymap: TOML, maks 1 MB, kedalaman ekspresi maks 16, tanpa include jaringan, tanpa script.
- Error parse tidak pernah membuat app gagal start: pakai versi valid terakhir + tampilkan error.

## 8. Privasi & log

- Tanpa telemetri. Log lokal, rotasi 7 hari; path lengkap hanya di level `debug`.
- Crash report lokal tanpa token; pengguna sendiri yang memutuskan membagikannya.
- File terbaru di Home (`home.show_recent`) dibaca dari Recent Items milik Windows; aplikasi tidak
  mencatat riwayat sendiri dan tidak menyimpan daftarnya. Folder Recent bisa diisi aplikasi mana pun,
  jadi isinya diperlakukan sebagai data tak tepercaya:
  - Target dibaca dari shortcut tanpa `Resolve` dan tanpa menyentuh targetnya; hanya target di drive
    lokal (`DRIVE_FIXED`) yang dicek keberadaannya. Shortcut ke share tidak memicu koneksi apa pun.
  - Yang ditampilkan hanya path dokumen biasa (`X:\…` atau `\\server\share\…`, tanpa `.`/`..`, bukan
    path device/verbatim). Program, skrip, dan shortcut (`exe`, `bat`, `cmd`, `ps1`, `lnk`, `url`, …)
    tidak pernah ditampilkan, karena satu klik pada baris membukanya.
  - Ikon baris selalu ikon per jenis file (dari ekstensi): file target tidak dibuka demi ikon, jadi
    target di share tidak dihubungi sebelum pengguna sendiri membukanya.
  - Nama ditampilkan tanpa karakter kontrol bidi.

## 9. Rilis & rantai pasok

- `cargo-deny` (lisensi + advisory + sumber), `cargo-audit` di CI.
- Lisensi yang diizinkan: MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, MPL-2.0. Tolak GPL/AGPL
  agar lisensi proyek tetap bebas dipilih.
- Rilis ditandatangani (code signing); checksum SHA-256 dipublikasikan; auto-update hanya menerima paket
  bertanda tangan.
