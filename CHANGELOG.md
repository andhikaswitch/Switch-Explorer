# Changelog

Format mengikuti [Keep a Changelog](https://keepachangelog.com/id-ID/1.1.0/) dan
[Semantic Versioning](https://semver.org/lang/id/).

## [Unreleased]
### Added
- Dokumentasi perencanaan: PRD, arsitektur, ADR, sistem desain Cerulean, mesin tema, spesifikasi fitur,
  Developer Mode + Drag Guard + Hot Switch, integrasi Google Drive, integrasi sistem Windows, budget
  performa, keamanan, roadmap, strategi test, konvensi kode, prompt Claude Code.
- Spike Fase 0 (`spikes/phase0/`) dan laporan `docs/benchmarks/phase0.md`: Mica Alt + title bar kustom,
  tabel 100k, OLE drag-out, drop target sendiri, menu native, Hot Switch (chord + hotkey global + OSD),
  ikut light/dark, skrip benchmark.
- ADR-015 (loop modal lewat window host), ADR-016 (drop target OLE sendiri), ADR-017 (tanpa `gpui-fast`),
  ADR-018 (OSD overlay, diusulkan).
- Fondasi Fase 1: workspace + toolchain terpin, `sx-core` (Location, Selection, CommandRegistry),
  `sx-settings` (registry flag, Hot Switch, preset, store TOML), `sx-theme` (OKLCH, parser nilai, palet,
  resolver berlapis, gerbang kontras), `sx-win` (enumerasi cepat, sort natural OS), `sx-fs` (snapshot
  direktori), `xtask` (fixtures, cleanup-registry aman, shim).
- Window pertama (Fase 1): tab di title bar, command bar dengan breadcrumb/alamat, sidebar (Home,
  Pinned, drive + gauge, Recycle Bin), details view tervirtualisasi dengan sort, seleksi keyboard,
  type-to-select, buka file/folder, status bar; ikut light/dark dan transparansi Windows; i18n en + id.
- Tombol, item sidebar, dan kartu menyala dengan gradien biru muda saat disorot (DESIGN §11).
- Tide Slash berbentuk sungguhan (sisi kiri miring 12° + garis `accent.edge`) untuk item sidebar aktif;
  indikator tab aktif miring.
- Single instance: peluncuran berikutnya meneruskan path ke window yang sudah terbuka (named pipe per user).
- Operasi file: Delete ke Recycle Bin (dengan konfirmasi), hapus permanen dengan konfirmasi, rename inline
  F2, folder baru, copy/cut/paste yang interop dengan File Explorer, sheet konflik (Ganti, Lewati, Simpan
  keduanya, Ganti jika lebih baru), progress + batal, toast Urungkan, undo/redo, journal write-ahead,
  laporan operasi yang terputus, panel Transfer (`Ctrl+J`), salin path (`Ctrl+Shift+C`).
- Watcher folder: daftar diperbarui sendiri saat isi folder berubah.
- Ikon sistem di daftar (cache terbatas `perf.icon_cache_entries`).
- Pin/unpin folder di sidebar; nama `shell:` dan `~` di address bar.
- ADR-021 (engine operasi file Fase 1), ADR-022 (watcher muat ulang debounce).
- Hot Switch: `Ctrl+Shift+D` dan chord `Ctrl+K` (F, D, S, E, G, L, H, X, T, R) membalik switch seketika
  dengan OSD di atas status bar, chip "Drag: aktif/sebagian/mati" (tooltip berisi semua switch drag dan
  hotkey-nya), dan badge "Drag mati" di title bar. Preset "anti salah drag" bisa dinyalakan/dimatikan dan
  mengembalikan nilai sebelumnya. Perubahan disimpan sesuai `hotswitch.persist`.
- Ekstensi nama file bisa disembunyikan (`view.show_extensions`, `Ctrl+K`, `X`).
- OSD Hot Switch menyebut dampaknya ("Drag file tetap aktif. Balik lagi: Ctrl+K, D"), naik + fade saat
  muncul dan fade saat hilang (tanpa animasi bila `perf.animations` mati), tidak meneruskan klik ke daftar di
  belakangnya, dan tampil di atas dialog. Toast tetap tampil selama di-hover atau difokus.
- Progress operasi di tombol taskbar (`ops.taskbar_progress`).
- Lebar kolom Diubah, Tipe, dan Ukuran bisa diseret di header (per tab, per sesi).
- Navigasi keyboard di luar daftar: `Tab`/`Shift+Tab` ke tab, tombol nav, breadcrumb, dan item sidebar;
  `F6`/`Shift+F6` antar area; `Enter`/`Space` mengaktifkan; cincin fokus hanya untuk fokus dari keyboard.
  Klik mouse tidak memindahkan fokus dari daftar.
- Gerbang kontras mencakup lapisan di bawah latar dan 27 kelompok pasangan, termasuk indikator non-teks
  3:1 (THEMING §11).
- Filter instan di kotak cari (`Ctrl+F`):
  - Folder aktif difilter per ketikan, dan bagian nama yang cocok ditulis tebal.
  - Status bar menulis "x dari y item".
  - Bila tidak ada yang cocok, muncul pesan beserta tombol "Kosongkan pencarian".
  - `Esc` mengosongkan filter; filter disimpan per tab.
  - Di window sempit, kotak cari menjadi ikon.
  - Token baru `match.fg`, `size.search_width`, dan `border.input_hover`.
  - Kotak alamat dan kotak cari memakai cincin fokus standar saat fokus.
- Pencarian rekursif (`Enter` di kotak cari, `Ctrl+E`, `F3`):
  - Mencari di folder dan semua subfoldernya dengan operator `ext:`, `size:`, `modified:`, `kind:`,
    wildcard, dan frasa berkutip.
  - Hasil tampil sambil berjalan, dengan kolom Folder, lalu diurutkan saat selesai.
  - Mengetik lagi membatalkan pencarian yang berjalan, lalu menjalankannya ulang setelah jeda ketik.
  - Hasil bisa dibuka, di-rename, dihapus, dan disalin.
  - `Esc` mengembalikan daftar folder.

### Changed
- ARCHITECTURE §8.4: perubahan tema dipicu notifikasi registry, bukan `WM_SETTINGCHANGE`.
- SYSTEM-INTEGRATION §3, §9, §15: koreksi berdasarkan temuan spike (Mica tanpa fallback di bawah build
  22621, latar Root opak, manifest GPUI dipaksa, pola modal host).
- PERFORMANCE: baseline Fase 0 dan pengecualian CPU idle.
- Cerulean: `fg.muted` light lebih gelap dan dark lebih terang, `selection.border` lebih pekat,
  `drop_target.border` light memakai 700, indikator light (toggle, slider, progress, tab aktif) memakai
  `accent.solid`, chip status light memakai `pal.accent.800`/`#92400E`.
- Cerulean: `border.input` light `#72869E` (3:1 di atas command bar).
- Path Win32 selalu verbatim dan dinormalisasi; path relatif ditolak; nama lossy memakai nama asli
  (SYSTEM-INTEGRATION §15).
- Pesan error menyebut penyebab + solusi (drive belum siap, jaringan, tidak ada aplikasi, file dipakai).

### Fixed
- Single instance: peluncuran berikutnya hanya meneruskan path ke Switch Explorer milik user yang sama
  (SID, sesi, elevasi, dan exe yang sama). Pipe yang "diduduki" proses lain ditolak, dan nama pipe kini per
  sesi dan per elevasi.
- Undo delete/Replace memulihkan item persis yang di-recycle (slot `$R` dari shell), tidak meninggalkan
  entri hantu `$I` di Recycle Bin, dan tidak pernah menimpa file baru: item kembali sebagai "nama (2)", dan
  redo me-recycle item itu.
- Operasi yang berhenti di tengah (error, batal, item ditolak Recycle Bin) tetap bisa di-undo untuk bagian
  yang sudah jalan. Item yang dihapus permanen setelah peringatan shell disebut di toast.
- Cut-paste mengosongkan clipboard hanya setelah semua item pindah, dan hanya bila isinya belum diganti
  (salinan baru di app lain tidak lagi hilang).
- Clipboard, cek konflik paste, dan penomoran "New folder" tidak lagi berjalan di UI thread.
- Drive USB yang sedang dibuka bisa di-eject: watcher melepas handle saat Windows meminta. Folder yang
  gagal dimuat tidak lagi ditahan watcher.
- Restore folder yang namanya sudah dipakai lagi tidak pernah digabung (kembali sebagai "nama (2)"), dan
  restore menolak slot Recycle Bin milik item lain. Menjawab No di peringatan hapus permanen tidak lagi
  dilaporkan sebagai "dihapus".
- Clipboard berjalan berurutan di satu thread; undo yang ditolak mode baca-saja tetap tersedia; undo/redo
  yang selesai setelah operasi baru tidak menulis ulang riwayat.
- Instance elevated tidak bisa dikendalikan proses non-elevated lewat pipe.
- Navigasi keyboard: Tab dan F6 kini benar-benar sampai ke kontrol (sebelumnya tidak ada tab stop). Fokus
  pulih ke area atau daftar saat elemen yang fokus hilang (tab ditutup, tombol jadi nonaktif). Klik tombol
  mouse apa pun tidak memindahkan fokus. Cincin fokus terlihat di item aktif sidebar dan tetap tampil saat
  mouse bergerak. Tombol dialog, toast Urungkan, dan kartu drive bisa dijangkau keyboard; Enter di tombol
  dialog mengklik tombol itu.
- Ikon dimuat di worker STA yang memompa pesan, item yang terlihat lebih dulu; ikon lama tanpa alpha tidak
  lagi kehilangan piksel hitamnya.
