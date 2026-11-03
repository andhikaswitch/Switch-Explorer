# Sistem Desain — Cerulean

Dokumen ini adalah sumber kebenaran tampilan Switch Explorer. Nilai warna lengkap per token ada di
`THEMING.md` (tema `cerulean`); dokumen ini menjelaskan identitas, layout, komponen, dan perilaku visual.

## 1. Konsep & prinsip

Switch Explorer terasa seperti panel kontrol yang tenang di atas air: tepi kaca yang menangkap warna
wallpaper, inti yang padat dan mudah dibaca, dan satu cahaya biru muda yang selalu menandai "kamu di
sini". Inspirasinya adalah highlight menu Persona 3 Reload — bar biru muda terang dengan teks navy —
yang dimodernkan dengan material Mica Windows 11. Tidak ada aset Persona yang dipakai; hanya palet dan
gaya highlight.

1. **File adalah pemeran utama.** Chrome (title bar, sidebar, toolbar) mundur; daftar file maju.
2. **Satu signature: Tide Slash.** Highlight Lagoon miring 12° dengan glow Cerulean, khusus untuk penanda
   lokasi/aktif. Karena bentuknya (sisi kiri terpotong miring + garis `accent.edge` + glow) hanya dipakai
   di situ, artinya selalu "kamu di sini". Gradien biru yang sama juga menyala saat kursor di atas tombol,
   item sidebar, atau kartu (§11), sebagai umpan balik sementara: pill membulat tanpa potongan dan tanpa
   glow. Yang membedakan "aktif" dari "disorot" adalah bentuk, bukan kecerahan.
3. **Kaca di tepi, padat di tengah.** Transparansi di title bar dan sidebar; area daftar ≥ 84% opak agar
   teks kecil tetap tajam.
4. **Gerak hanya menjawab aksi.** Tidak ada animasi ambient; semua menghormati "Animation effects" Windows.
5. **Perilaku native.** Shortcut, context menu, snap layout, dan drag & drop bekerja seperti kebiasaan
   pengguna Windows.
6. **Status selalu terlihat.** Setiap switch yang berubah (terutama lewat hotkey) diumumkan lewat OSD dan
   kondisinya tetap terlihat di chip status bar.

## 2. Palet inti

| Nama | Hex | Peran |
|---|---|---|
| Lagoon | `#5CE1FF` | Awal gradien highlight, tepi fokus di dark mode — biru muda dominan |
| Cerulean | `#2FA8FF` | Seed aksen, akhir gradien highlight, glow |
| Tide | `#1474D0` | Bidang solid bertulisan putih, link & fokus di light mode |
| Deep Tide | `#0B55A8` | Akhir gradien solid, tepi miring Tide Slash di light mode |
| Abyss | `#06101F` | Dasar dark mode |
| Foam | `#EAF5FF` | Tint kaca light mode |
| Ink | `#0A1A2F` | Teks utama light mode dan teks di atas highlight |

Biru muda (Lagoon/Cerulean) dipakai sebagai **cahaya**: highlight, glow, tint, seleksi. Biru yang lebih
dalam (Tide) dipakai di bidang yang membawa teks putih. UI terasa biru muda tanpa mengorbankan keterbacaan.

Kontras terverifikasi (WCAG 2.x): Ink di atas Lagoon ≈ 11:1, Ink di atas Cerulean ≈ 6.8:1, putih di atas
Tide ≈ 4.7:1, putih di atas Deep Tide ≈ 7.3:1, `fg.muted` light `#607389` di putih ≈ 4.9:1, `fg.muted`
dark `#7A8FA8` di `#0A1628` ≈ 5.5:1.

## 3. Warna semantik kunci

| Token | Light | Dark | Catatan |
|---|---|---|---|
| `bg.window` | Foam 55% | Abyss 60% | Tint di atas Mica Alt |
| `bg.sidebar` | putih 30% | Abyss 35% | Bagian paling transparan |
| `bg.content` | putih 84% | `#0A1628` 86% | Inti yang legibel |
| `fg.primary` | Ink | `#EAF4FF` | |
| `fg.muted` | `#607389` | `#7A8FA8` | Teks sekunder ≥ 12 px |
| `accent.highlight` | Lagoon → Cerulean (120°) | sama | Tide Slash, tombol primer, toggle on |
| `accent.solid` | Tide → Deep Tide (135°) | sama | Checkbox, bidang solid bertulisan putih |
| `selection.bg` | Cerulean 16% | Cerulean 22% | Baris terpilih |
| `focus.ring` | Tide | Lagoon | `focus.ring_width` (2 px) sebagai border elemen: transparan (atau warna border elemen itu sendiri) saat tidak fokus, jadi tidak ada yang bergeser; dilukis di atas isi elemen (termasuk Tide Slash) dan tidak terpotong container yang meng-clip. Di atas kontrol yang di-highlight (item aktif, gradien hover) memakai `focus.ring_on_highlight` (Ink) |

Warna kategori (ikon folder kategori, chip, bar komposisi Organizer): Dokumen `#3B82F6`, Audio `#F43F5E`,
Gambar `#F59E0B`, Video `#8B5CF6`, Arsip `#10B981`, Kode `#06B6D4`, Aplikasi `#64748B`, Lainnya `#94A3B8`.

## 4. Tipografi

| Gaya | Font | Ukuran/leading | Berat | Dipakai untuk |
|---|---|---|---|---|
| Display | Saira Semi Condensed | 28/34 | 600 | Judul halaman (Home, Settings) |
| Title | Saira Semi Condensed | 20/26 | 600 | Judul seksi |
| Subtitle | Segoe UI Variable Text | 16/22 | 600 | Header kartu, judul dialog |
| Body | Segoe UI Variable Text | 14/20 | 400 | Default UI |
| Body strong | Segoe UI Variable Text | 14/20 | 600 | Item aktif, tombol, judul OSD |
| Caption | Segoe UI Variable Text | 12/16 | 400 | Metadata, status bar, baris kedua OSD |
| Numerik | Segoe UI Variable + `tnum` | 12–14 | 400 | Ukuran & tanggal, rata kanan |
| Mono | Cascadia Mono → Consolas | 12/18 | 400 | Panel developer, log |

- Segoe UI Variable adalah font sistem Windows 11 (Windows 10: Segoe UI) — nol biaya bundling, rendering
  DirectWrite yang tajam. Saira Semi Condensed (OFL) dibundel khusus untuk judul: kondensed dan tegas,
  memberi nuansa HUD game tanpa mengorbankan keterbacaan. Bisa diganti lewat token `font.display`.
- Sentence case di semua label. Tidak memakai label ALL CAPS.
- Panjang baris teks panjang (dialog, deskripsi setting) maksimal ~80 karakter.

## 5. Spasi, radius, elevasi

- **Spasi** (basis 4 px): 2, 4, 6, 8, 12, 16, 24, 32. Padding konten 12 px; jarak antar seksi 24 px.
- **Radius bertingkat sesuai hierarki** (bukan satu radius untuk semua): baris & kontrol 6 px, menu item
  8 px, kartu & panel 12 px, dialog 16 px, chip & pill penuh. Sudut window mengikuti sistem.
- **Elevasi**: e0 baris (tanpa bayangan); e1 kartu `shadow.card`; e2 menu/popover/OSD `shadow.popover`;
  e3 dialog `shadow.dialog`; glow `shadow.glow` khusus Tide Slash & tombol primer saat fokus.

## 6. Material & transparansi

| Pilihan | Pemetaan GPUI | Kapan |
|---|---|---|
| Mica Alt (default) | `WindowBackgroundAppearance::MicaAltBackdrop` | Windows 11 22H2+ |
| Mica | `MicaBackdrop` | Tint wallpaper lebih halus |
| Acrylic | `Blurred` | Efek kaca paling kuat; boros daya |
| Solid | `Opaque` | Windows 10, "Transparency effects" Windows mati, mode hemat baterai, high contrast |

- Mica hanya terlihat saat window aktif (perilaku Windows); window tidak aktif menjadi solid. Desain harus
  tetap bagus dalam dua keadaan itu.
- Token latar beralfa dikomposit menjadi opak otomatis saat material solid (THEMING.md §8).
- Gambar latar kustom (opsional, untuk ricing) dirender di bawah `bg.window` dengan blur & dim yang bisa
  diatur; didecode seukuran window, bukan resolusi penuh.

## 7. Signature: Tide Slash

- Bentuk: pill `accent.highlight` dengan sisi kiri terpotong miring `slash.angle_deg` (12°), tepi miring
  3 px `accent.edge`, glow `shadow.glow`. Teks `fg.on_highlight` (Ink).
- Dipakai **hanya** untuk: item sidebar aktif, indikator tab aktif (garis bawah miring), item terpilih di
  Theme Studio. Tidak dipakai sebagai dekorasi.
- Berpindah antar item dengan animasi slide `motion.slide` (280 ms, spring); reduce motion → langsung pindah.
- Implementasi: elemen custom-paint di atas gpui-base (path parallelogram + gradien), bukan gambar.

## 8. Layout & wireframe

### 8.1 Anatomi window

```text
┌──────────────────────────────────────────────────────────────────────────────────────┐
│ ◆   Home   Downloads ×  ╱ Received ╲ ×   +                                ─   ▢   ✕ │ 40 px
├──────────────────────────────────────────────────────────────────────────────────────┤
│  ←  →  ↑  ⟳   This PC › Data (D:) › Received ▾              [ Cari di Received   ] ⋯ │ 44 px
├─────────────────┬──────────────────────────────────────────────────────┬─────────────┤
│  Home           │ Nama ▾                    Diubah          Tipe  Ukuran │ Preview     │
│                 │ ▸ Laporan                 Kemarin 21.10   Folder       │ ┌─────────┐ │
│ Pinned          │   0887b8d1….jpg           12/05/2025      JPG    33 KB │ │  thumb  │ │
│   Kuliah        │ ▌ 1-4.pdf                 03/04/2025      PDF    84 KB │ └─────────┘ │
│   Projects      │   15-19.pdf               03/04/2025      PDF    85 KB │ 1-4.pdf     │
│                 │                                                      │ 84 KB, PDF  │
│ This PC         │                                                      │             │
│   C:  ▓▓▓▓▓░ 70%│                                                      │             │
│ ╱ D:  ▓▓▓░░ 45%╲│                                                      │             │
│ Google Drive    │                                                      │             │
│ Recycle Bin     │                                                      │             │
│ Tools           │                                                      │             │
├─────────────────┴──────────────────────────────────────────────────────┴─────────────┤
│ 207 item   2 dipilih (117 KB)   D: sisa 212 GB          Drag: sebagian   ⇅ 56%  ☰ ▦ │ 28 px
└──────────────────────────────────────────────────────────────────────────────────────┘
```

- Title bar 40 px (tab di dalam title bar, drag area di sela tab). Command bar 44 px. Status bar 28 px.
- Sidebar 248 px (min 200, maks 360, rail 56 px saat collapse). Preview pane 320 px (min 240).
- Tinggi baris: compact 28, default 32, comfortable 36.
- Item sidebar yang aktif adalah lokasi terdekat dengan folder saat ini (di sini: drive D:).

### 8.2 Home

```text
┌ Home ─────────────────────────────────────────────────────────────────────────┐
│ Penyimpanan                                                                   │
│  C: Sistem     ▓▓▓▓▓▓▓░░░  335 dari 476 GB    D: Data   ▓▓▓▓░░░░░░  420 dari 931 GB │
│  Google Drive  ▓░░░░░░░░░  100 GB dari 2 TB   + Hubungkan akun                │
│                                                                               │
│ Kategori                                                                      │
│  [Dokumen 100] [Audio 20] [Gambar 340] [Video 12] [Arsip 18] [+ Kategori]     │
│                                                                               │
│ Pinned                                  Aktivitas terakhir                    │
│  Kuliah   Projects   Received            3 item dipindahkan ke Arsip [Urungkan]│
│                                          2 file diunggah ke Drive    [Lihat]   │
│ Terbaru                                                                       │
│  1-4.pdf          D:\Received          2 jam lalu                             │
│  Laporan.docx     Documents\Kuliah     Kemarin                                │
└───────────────────────────────────────────────────────────────────────────────┘
```

Storage ditampilkan sebagai gauge baris yang ringkas (bukan deretan kartu besar seperti FariSYNC) supaya
Home tetap padat informasi. Gauge berubah ke `gauge.fill_warning` di ≥ 85% dan `gauge.fill_danger` di ≥ 95%.

### 8.3 Popup tujuan Google Drive

```text
┌──────────────── Upload ke Google Drive ─────────────────┐
│ 3 item, 128 MB                     Akun: nama@gmail.com │
│ (•) Salin   ( ) Pindahkan — file lokal dibuang ke       │
│                 Recycle Bin setelah terverifikasi       │
│ ┌─────────────────────────────────────────────────────┐ │
│ │ My Drive › Kuliah › Semester 5        [+ Folder baru]│ │
│ │ ▸ Tugas                                             │ │
│ │ ▾ Laporan                                           │ │
│ │     Minggu-1                                        │ │
│ │ ▸ Referensi                                         │ │
│ └─────────────────────────────────────────────────────┘ │
│ Jika nama sudah ada: [Simpan keduanya ▾]                │
│ [x] Simpan sebagai tujuan favorit                       │
│                            [Batal]  [ Upload ke sini ]  │
└─────────────────────────────────────────────────────────┘
```

Tombol utama berlabel sesuai aksi: "Upload ke sini" atau "Pindahkan ke sini". Toast hasilnya memakai kata
yang sama ("3 file diunggah ke Laporan").

### 8.4 Panel Hot Switch & OSD

```text
┌ Hot Switch ────────────────────────────────── [ Cari switch…    ] ┐
│ Drag & drop                                                        │
│   Semua drag & drop                ●━━  Aktif      Ctrl+Shift+D    │
│   Drag file                        ●━━  Aktif      Ctrl+K, F       │
│   Drag folder                      ━━○  Nonaktif   Ctrl+K, D       │
│   Drag dari sidebar & pinned       ━━○  Nonaktif   Ctrl+K, S       │
│   Drag ke/dari aplikasi lain       ●━━  Aktif      Ctrl+K, E       │
│   Preset anti salah drag           ━━○  Mati       Ctrl+K, G       │
│ Tampilan & keselamatan                                             │
│   File tersembunyi                 ━━○             Ctrl+K, H       │
│   Ekstensi file                    ●━━             Ctrl+K, X       │
│   Mode baca-saja                   ━━○             Ctrl+K, R       │
│                                       [Atur hotkey…]   [Tutup]     │
└────────────────────────────────────────────────────────────────────┘

           ╭────────────────────────────────────────────────╮
           │  ⊘  Drag folder dinonaktifkan                  │   ← OSD (tengah bawah)
           │     Drag file tetap aktif. Balik lagi: Ctrl+K, D│
           ╰────────────────────────────────────────────────╯
```

### 8.5 Settings → Drag & drop

```text
┌ Settings ─────────────────────────────────────────────────────────────────────┐
│ Umum             │ Drag & drop                              [ Cari switch… ] │
│ Tampilan         │ Preset: (•) Seperti Explorer ( ) Anti salah drag ( ) Kustom │
│ Keyboard         │                                                            │
│ Drag & drop    ◀ │ Semua drag & drop              ●━━   Ctrl+Shift+D          │
│ File & operasi   │ Drag file                      ●━━   Ctrl+K, F             │
│ Integrasi        │ Drag folder                    ━━○   Ctrl+K, D             │
│ Akun             │ Drag dari sidebar & pinned     ━━○   Ctrl+K, S             │
│ Tools            │ Jeda setelah scroll            [ 400 ] ms                  │
│ Performa         │ Jarak minimal sebelum drag     [  14 ] px                  │
│ Developer        │ Aksi saat drop                 [ Tanya dulu ▾ ]            │
│ Tentang          │ Konfirmasi pindah folder       ●━━                         │
└───────────────────────────────────────────────────────────────────────────────┘
```

Setiap baris switch menampilkan hotkey-nya (klik untuk merekam hotkey baru) dan badge stabilitas jika
bukan stabil. Halaman ini digenerate dari metadata registry flag.

### 8.6 Theme Studio

```text
┌ Theme Studio ─────────────────────────────────────────────────────────────────┐
│ [Cepat] [Token] [File]                     Pratinjau: (•) Light  ( ) Dark      │
│ ┌ Token ─────────────────────┐ ┌ Pratinjau ─────────────────────────────────┐ │
│ │ Cari: hover                │ │ ╱ Kuliah ╲     [ Upload ke sini ] ← hover   │ │
│ │ ▾ Tombol                   │ │ ▌ 1-4.pdf      84 KB                        │ │
│ │   button.primary.bg        │ │   15-19.pdf    85 KB                        │ │
│ │ ▸ button.primary.bg_hover ●│ │ [Toggle ●━━]  [Checkbox ☑]  [Fokus ▭]       │ │
│ │   button.primary.fg        │ └────────────────────────────────────────────┘ │
│ │ ▸ Navigasi                 │ ┌ button.primary.bg_hover ───────────────────┐ │
│ │ ▸ Daftar & grid            │ │ Nilai: [ #E5484D         ]  override  Reset│ │
│ └────────────────────────────┘ │ Terapkan ke: (•) Mode ini  ( ) Keduanya    │ │
│                                │              ( ) Adaptasi otomatis         │ │
│                                │ Kontras dengan teks: 5.1:1 — lulus AA      │ │
│                                └────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────────────────────────────────┘
```

### 8.7 Window mode picker

```text
┌ Pilih file untuk diunggah — Google Chrome ────────────────────────────────────┐
│ ←  →  ↑   Documents › Kuliah ▾                         [ Cari di Kuliah     ] │
├────────────────┬──────────────────────────────────────────────────────────────┤
│ Pinned         │ ▌ Laporan Akhir.pdf     Kemarin        PDF       2.1 MB       │
│ Tab terbuka    │   Lampiran.zip          3 hari lalu    ZIP        14 MB       │
│ Google Drive   │                                                              │
├────────────────┴──────────────────────────────────────────────────────────────┤
│ Nama file: [ "Laporan Akhir.pdf"                   ]  [ Semua file (*.*) ▾ ]   │
│                                                     [Batal]   [ Buka ]         │
└───────────────────────────────────────────────────────────────────────────────┘
```

Judul window menyebut aplikasi peminta. Label tombol mengikuti dialog asli (Buka/Simpan/Pilih folder).

### 8.8 Responsif

- Lebar < 720 px: sidebar otomatis jadi rail 56 px.
- Lebar < 560 px: preview pane disembunyikan; kotak cari menjadi ikon.
- Kolom details menyusut berurutan: Tipe → Diubah; Nama dan Ukuran selalu terlihat.
- Lebar kolom Diubah, Tipe, dan Ukuran bisa diseret di tepi kirinya pada header (kursor resize kolom, garis
  `divider` saat disorot), 56–480 px; Nama mengambil sisanya. Lebar berlaku per tab selama sesi; mode
  tampilan per folder (Fase 2) akan menyimpannya.

## 9. Umpan balik status: OSD, chip, hint

### 9.1 OSD Hot Switch
- **Posisi:** tengah bawah window aktif, 72 px di atas status bar. Untuk hotkey global saat app di
  background: tengah bawah monitor aktif, 96 px dari tepi bawah work area. Diatur `hotswitch.osd_position`.
- **Ukuran & bentuk:** lebar 280–420 px, tinggi 56 px (dua baris), radius 16, `osd.bg`, `osd.border`,
  `shadow.popover`.
- **Isi:** ikon 24 px dalam lingkaran (aktif: isi `osd.state_on` + ikon `osd.icon_on`; nonaktif:
  `osd.state_off` + ikon dicoret `osd.icon_off`). Baris 1 Body strong: "Drag folder dinonaktifkan" /
  "Drag folder diaktifkan". Baris 2 Caption: dampak + hotkey, mis. "Drag file tetap aktif. Balik lagi:
  Ctrl+K, D".
- **Kelompok & preset:** "Drag ke/dari aplikasi lain dinonaktifkan (2 switch)", "Preset anti salah drag
  aktif (9 switch diubah)".
- **Perilaku:** muncul ≤ 50 ms setelah tombol; tampil `hotswitch.osd_ms` (1800 ms); toggle berulang
  memperbarui OSD yang sama dan mereset timer (tidak menumpuk); hover menahan; klik membuka panel Hot
  Switch. Tidak pernah mengambil fokus.
- **Gerak:** masuk fade + naik 8 px dalam `motion.base`; keluar fade 120 ms; reduce motion → fade saja.
- **Aksesibilitas:** diumumkan ke screen reader sebagai live region "polite".

### 9.2 Chip status
- Di status bar, kanan: ikon drag + teks pendek. Tiga keadaan: "Drag: aktif" (`statuschip.on`),
  "Drag: sebagian" (`statuschip.partial`, ikon setengah), "Drag: mati" (`statuschip.off`, ikon dicoret).
- Tooltip: daftar sub-switch beserta status dan hotkey. Klik: panel Hot Switch sebagai popover.
- Badge title bar (`hotswitch.titlebar_badge`): pill kecil "Drag mati" di dekat tombol window ketika
  master drag nonaktif, agar tetap terlihat walau status bar disembunyikan.

### 9.3 Hint saat diblokir
- Saat pengguna mencoba drag item yang switch-nya nonaktif: toast kecil dekat kursor "Drag folder sedang
  nonaktif." dengan tombol [Aktifkan] dan hotkey-nya. Maksimal sekali per 30 detik per switch
  (`dnd.blocked_hint`).
- Seleksi campuran (file + folder) dengan salah satu nonaktif: drag ditolak seluruhnya, hint menyebut
  alasannya. Tidak ada item yang diam-diam tertinggal.

## 10. Suara & copy UI

Aturan: sentence case, kata kerja aktif, nama aksi konsisten dari tombol sampai toast ("Pindahkan" →
"dipindahkan"), error tidak meminta maaf dan selalu menyebut penyebab + cara memperbaiki, layar kosong
mengajak bertindak. Nama lokasi sistem (mis. Recycle Bin) diambil dari display name shell agar sama
dengan bahasa Windows pengguna.

| Situasi | Bahasa Indonesia | English |
|---|---|---|
| Hapus permanen | "Hapus 3 item secara permanen? Item tidak masuk Recycle Bin." [Hapus permanen] [Batal] | "Permanently delete 3 items? They won't go to the Recycle Bin." |
| Setelah pindah | "3 item dipindahkan ke Arsip." [Urungkan] | "Moved 3 items to Archive." [Undo] |
| OSD | "Drag folder dinonaktifkan" / "Drag file tetap aktif. Balik lagi: Ctrl+K, D" | "Folder dragging turned off" / "File dragging is still on. Toggle: Ctrl+K, D" |
| Hint diblokir | "Drag folder sedang nonaktif." [Aktifkan] | "Folder dragging is off." [Turn on] |
| File terkunci | "Tidak bisa memindahkan "Laporan.xlsx" karena sedang dibuka di Excel. Tutup file lalu coba lagi." | "Can't move "Laporan.xlsx" because it's open in Excel. Close it and try again." |
| Folder kosong | "Folder ini kosong. Seret file ke sini atau tekan Ctrl+Shift+N untuk membuat folder." | "This folder is empty. Drag files here or press Ctrl+Shift+N to create a folder." |
| Verifikasi Drive gagal | "Checksum "video.mp4" tidak cocok setelah diunggah. File lokal tetap aman." [Unggah ulang] | "Checksum for "video.mp4" didn't match after upload. Your local file is safe." [Upload again] |
| Butuh admin | "Butuh izin administrator untuk menyalin ke Program Files." [Lanjutkan sebagai admin] | "Administrator permission is needed to copy to Program Files." |

## 11. Komponen & state

| Komponen | Default | Hover | Pressed/aktif | Fokus | Disabled |
|---|---|---|---|---|---|
| Item sidebar | `nav.item.fg` | `nav.item.hover.bg` (gradien highlight) + `nav.item.hover.fg`; ditekan `nav.item.pressed.bg` | Tide Slash (`nav.item.active.*`) | `focus.ring` | `fg.disabled` |
| Tab | `tab.fg` | `tab.hover.bg` | `tab.active.bg` + indikator miring | `focus.ring` | — |
| Segmen breadcrumb | `breadcrumb.fg` | `breadcrumb.hover.bg` | `breadcrumb.current.fg` | `focus.ring` | — |
| Baris daftar | transparan | `row.hover.bg` | `row.selected.bg` + `row.selected.border` | ring di dalam baris | `fg.disabled` |
| Tile grid | transparan | `tile.hover.bg` | `tile.selected.bg` | ring | — |
| Tombol primer | `button.primary.bg` | `…bg_hover` | `…bg_pressed` | ring + glow | opasitas 40% |
| Tombol sekunder | `button.secondary.bg` + border | `…bg_hover` (gradien highlight) + `…fg_hover` | `…bg_pressed` | ring | opasitas 40% |
| Tombol ikon/ghost | transparan, `button.ghost.fg` | `button.ghost.bg_hover` (gradien highlight) + `…fg_hover` | `…bg_pressed` | ring | opasitas 40% |
| Kartu (drive, Home) | `surface.card` + `shadow.card` | `surface.card_hover` (gradien highlight) + `card.hover.fg` | — | ring | — |
| Tombol bahaya | `button.danger.bg` | `…bg_hover` | gelap 5% | ring | opasitas 40% |
| Toggle | off: border `toggle.off.border` | +`hover.bg` | on: `toggle.on.bg`, thumb `toggle.on.thumb` | ring | opasitas 40% |
| Checkbox | border `checkbox.border` | +`hover.bg` | `checkbox.checked.bg` + `checkbox.check` | ring | opasitas 40% |
| Input | `surface.input` + `border.input` | border lebih kuat | — | `border.input_focus` | `fg.disabled` |
| Menu konteks | `surface.overlay`, `shadow.popover` | `hover.bg` | `pressed.bg` | ring | `fg.disabled` |
| Dialog/sheet | `surface.overlay`, `scrim`, `shadow.dialog` | — | — | fokus terperangkap | — |
| Toast | `toast.bg`, `toast.fg` | tahan timer | — | dapat difokus via F6 | — |
| OSD | §9.1 | tahan timer | klik → panel | tidak bisa difokus | — |
| Drop target | — | `drop_target.bg` + `drop_target.border` | — | — | `drop_blocked.border` |
| Scrollbar | `scrollbar.thumb` | `scrollbar.thumb_hover` | — | — | — |

- **Biru modern saat disorot.** Tombol, item sidebar, dan kartu berubah menjadi gradien highlight
  biru muda dengan teks Ink saat kursor di atasnya (seperti FariSYNC), lebih terang dari item aktif dan
  tanpa glow. Ikon, keterangan, dan gauge di dalamnya ikut berganti warna. Baris daftar, tab,
  breadcrumb, dan header kolom tetap memakai hover halus (`*.hover.bg`) agar daftar panjang tidak
  berkedip saat kursor lewat. Tombol jendela (minimize/maximize/close) mengikuti standar Windows.
- Empty state: ilustrasi garis tipis monokrom + satu kalimat ajakan + satu tombol.
- Loading: skeleton baris statis (tanpa shimmer bergerak) setelah 150 ms; sebelum itu tidak menampilkan
  apa-apa agar tidak berkedip.
- Item "cut" ditampilkan dengan opasitas `row.cut.opacity`.

## 12. Gerak

| Token | Nilai | Dipakai untuk |
|---|---|---|
| `motion.fast` | 100 ms | Hover, pressed |
| `motion.base` | 160 ms | Toggle, buka menu, OSD masuk |
| `motion.slow` | 240 ms | Pane, dialog |
| `motion.slide` | 280 ms, spring | Tide Slash berpindah |
| easing standar | cubic-bezier(0.2, 0, 0, 1) | Default |
| easing keluar | cubic-bezier(0.4, 0, 1, 1) | Menutup |

Reduce motion (Windows "Animation effects" mati atau `perf.animations = off`): semua durasi 0, kecuali
fade opasitas 80 ms untuk OSD/toast agar tidak terasa patah.

## 13. Ikon

- Chrome UI: Fluent UI System Icons 20 px (Regular; Filled untuk item aktif). Menimpa aset ikon default
  GPUI Kit lewat mekanisme asset.
- Ikon file: dari sistem (sama dengan Explorer, termasuk ikon aplikasi seperti PSD/Office).
- Ikon folder bertema (`view.themed_folder_icons`): folder dua tone (`icon.folder.back` /
  `icon.folder.front`), glyph folder khusus (Downloads, Pictures, …) tetap ditampilkan. Folder kategori di
  Home memakai warna kategori.
- Ikon switch di OSD/panel: drag-hand (semua), file (drag file), folder (drag folder), sidebar (drag
  sidebar), panah keluar-masuk (aplikasi lain), perisai (preset), gembok (pinned), mata (hidden),
  titik-ekstensi (extension), gambar (thumbnail), pensil-dicoret (read-only).

## 14. Aksesibilitas

- Kontras teks ≥ 4.5:1, komponen UI & fokus ≥ 3:1; divalidasi otomatis untuk setiap tema (THEMING.md §11).
- Semua fungsi bisa dijangkau keyboard; urutan fokus logis; fokus selalu terlihat.
- Peran, nama, dan status lewat AccessKit (GPUI Kit). OSD diumumkan sebagai live region.
- Mengikuti text scale Windows (`UISettings.TextScaleFactor`), high contrast (tema HC otomatis, material
  solid), dan reduce motion.
- Target klik minimal 24×24 px (kontrol inti 32 px).
- Warna tidak pernah menjadi satu-satunya penanda: status switch juga ditunjukkan teks ("Aktif/Nonaktif")
  dan bentuk ikon (dicoret).

## 15. Do & don't

| Do | Don't |
|---|---|
| Pakai Tide Slash (bentuk miring) hanya untuk lokasi/aktif; gradien boleh menyala saat disorot | Menempel gradien biru sebagai dekorasi diam di mana-mana |
| Biarkan daftar file padat dan opak | Membuat area daftar transparan sampai teks berbayang |
| Umumkan setiap perubahan switch (OSD + chip) | Mengubah perilaku diam-diam |
| Radius bertingkat sesuai hierarki | Satu radius & satu bayangan untuk semua elemen |
| Copy spesifik: penyebab + cara memperbaiki | "Terjadi kesalahan." tanpa konteks |
| Ikuti material & tema sistem | Memaksa transparansi saat Windows mematikannya |
| Sheet in-app bertema untuk konfirmasi | MessageBox native berjudul nama framework |
