# PRD — Switch Explorer

Status: Draft v1 | Pemilik: pemilik proyek | Diperbarui: November 2026

## 1. Ringkasan

Switch Explorer adalah file explorer Windows berbasis Rust yang mengejar tiga hal sekaligus: lebih
ringan dan responsif daripada File Explorer bawaan, bisa dikustom sampai detail (perilaku *dan*
tampilan), serta terhubung ke Google Drive tanpa perlu sync client. Setiap perilaku adalah switch,
dan switch yang sering dipakai bisa dibalik lewat hotkey dengan status yang selalu terlihat.

## 2. Masalah

1. File Explorer Windows 11 terasa lambat saat membuka window, pindah ke folder besar, dan memuat
   halaman Home/Gallery.
2. **Drag tidak sengaja.** Saat scroll dengan touchpad/mouse, item pinned atau folder ikut ter-drag dan
   pindah ke folder lain tanpa disadari. Tidak ada cara mematikan drag per jenis item, apalagi cepat.
3. Kustomisasi nyaris nol: warna, perilaku, dan tata letak tidak bisa diatur.
4. Upload ke Google Drive harus lewat browser atau memasang Drive for desktop yang memakan resource
   terus-menerus.
5. Dialog Open/Save aplikasi lain tidak tahu folder favorit, tab yang sedang dibuka, atau isi Drive.

## 3. Target pengguna

| Persona | Kebutuhan utama |
|---|---|
| Raka — mahasiswa/pekerja kantor | Kirim file ke WhatsApp/Drive cepat, folder tugas rapi, tidak salah pindah folder |
| Dina — developer | Banyak folder proyek, tab & split view, copy path, buka terminal/VS Code, mode read-only |
| Kai — pecinta estetika (ricer) | Tema translucent, warna per elemen, gambar latar, berbagi file tema |
| Power user keyboard | Semua aksi via shortcut, command palette, dan Hot Switch; undo yang bisa diandalkan |

## 4. Tujuan & ukuran keberhasilan

| Tujuan | Ukuran (metode di PERFORMANCE.md) |
|---|---|
| Lebih ringan dari Explorer | Private working set 1 window ≤ 60 MB (target), lebih kecil dari proses Explorer terpisah pada skenario yang sama |
| Responsif | Cold start ≤ 300 ms; folder 10k item: baris pertama ≤ 50 ms; scroll stabil 60 fps |
| Tidak ada salah pindah | Preset "Anti salah drag" → 0 perpindahan tidak sengaja pada skenario uji scroll; semua move bisa di-undo |
| Switch cepat & jelas | Hot Switch: tombol → OSD tampil ≤ 50 ms; status drag selalu terlihat di status bar |
| Kustomisasi dalam | Semua warna UI terekspos sebagai token; ganti tema/aksen tanpa restart (< 100 ms) |
| Drive praktis | Upload ke folder Drive pilihan ≤ 3 klik dari klik kanan |
| Aman | 0 kehilangan data di test suite operasi file; Picker Takeover punya fallback & kill switch |

## 5. Non-goal (sengaja tidak dikerjakan di v1)

- Mengganti shell Windows (taskbar, desktop, Start menu).
- Sync dua arah ala Drive for desktop/OneDrive. v1 hanya upload, move, download, dan browse on-demand.
- Cross-platform. Arsitektur tidak menghalangi, tapi tidak dikerjakan.
- Plugin/script pihak ketiga (risiko keamanan; ditinjau ulang setelah v1).
- Telemetri dalam bentuk apa pun.

## 6. Ruang lingkup & prioritas

P0 = wajib agar layak dipakai harian, P1 = yang membuatnya unggul, P2 = nice-to-have.

| Area | Fitur | Prioritas | Fase |
|---|---|---|---|
| Inti | Window kustom + Mica, tab, sidebar, breadcrumb, details view tervirtualisasi | P0 | 1 |
| Inti | Navigasi back/forward/up, sort, ikon sistem, watcher | P0 | 1 |
| Inti | Copy/cut/paste/rename/delete (Recycle Bin), progress, konflik, undo | P0 | 1 |
| Inti | Context menu bertema + "Tampilkan opsi lain" (menu native) | P0 | 2 |
| Inti | OLE drag & drop dua arah dengan aplikasi lain | P0 | 2 |
| Inti | Grid/gallery + thumbnail, preview pane, Quick Look (Space) | P1 | 2 |
| Inti | Pencarian & filter, integrasi Everything | P1 | 2 |
| Inti | Split view, session restore, command palette, Home dashboard | P1 | 2 |
| Personalisasi | Tema Cerulean light/dark ikut Windows | P0 | 1 |
| Personalisasi | Aksen kustom/Windows/wallpaper, material, densitas, gambar latar | P1 | 3 |
| Personalisasi | Theme Studio + Token Inspector + file tema hot reload | P1 | 3 |
| Switch | Registry flag, Drag Guard (switch per jenis item, cooldown scroll, dll.) | P0 | 2 |
| Switch | **Hot Switch**: hotkey drag semua/file/folder/sidebar + OSD + chip status + hint saat diblokir | P0 | 2 |
| Switch | Keymap editor, hotkey global, halaman flag, preset, Location Rules | P1 | 3 |
| Developer Mode | Perf HUD, memory meter, log viewer, Token Inspector | P2 | 3 |
| Cloud | Connect Google Drive, kuota, Upload/Pindahkan ke Drive + popup pilih/buat folder | P1 | 4 |
| Cloud | Browse Drive di sidebar (on-demand, lazy) | P2 | 4 |
| Integrasi | Default untuk buka folder + Win+E, single instance, resident mode | P1 | 5 |
| Integrasi | Picker Bridge (panel di samping dialog Open/Save) | P1 | 5 |
| Integrasi | Picker Takeover (ganti dialog lewat COM proxy) | P2 eksperimental | 6 |
| Tools | Organizer (paritas FariSYNC + dry-run + undo) | P1 | 7 |
| Tools | Duplicate Finder, Bulk Rename, Space Analyzer | P2 | 7 |
| Tools | Sync folder ↔ folder/Drive | P2 | setelah v1 |

## 7. User story utama

1. **Anti salah drag.** Sebagai Raka, saat scroll daftar pinned dengan touchpad, saya tidak mau folder
   ikut ter-drag. *Diterima jika:* dengan preset "Anti salah drag", menekan item dalam 400 ms setelah
   scroll terakhir tidak memulai drag, dan item pinned tidak bisa di-drag keluar sidebar.
2. **Matikan drag folder dalam sekejap.** Sebagai Raka, sebelum menelusuri folder panjang saya menekan
   `Ctrl+K` lalu `D`. *Diterima jika:* drag folder nonaktif ≤ 50 ms, OSD menampilkan "Drag folder
   dinonaktifkan" beserta info bahwa drag file tetap aktif, chip status bar berubah ke "Drag sebagian",
   pengaturan tersimpan, dan saat saya mencoba drag folder muncul hint cara mengaktifkannya lagi.
3. **Pindahkan ke Drive.** Sebagai Raka, saya klik kanan 3 PDF → "Pindahkan ke Google Drive" → pilih
   "Kuliah/Semester 5" atau buat folder baru → file terupload, diverifikasi, lalu file lokal masuk
   Recycle Bin. *Diterima jika:* checksum cocok sebelum file lokal dihapus; gagal = file lokal utuh.
4. **Ubah satu warna.** Sebagai Kai, saya ingin warna hover tombol primer jadi merah tanpa mengubah warna
   lain. *Diterima jika:* Token Inspector menunjukkan `button.primary.bg_hover`, saya ubah, dan perubahan
   langsung berlaku di mode yang saya pilih (mode ini / keduanya / adaptasi otomatis).
5. **Lompat dari dialog upload.** Sebagai Dina, saat dialog "Open" Chrome muncul, saya ingin langsung ke
   folder tab Switch Explorer yang aktif. *Diterima jika:* `Ctrl+G` atau klik di Picker Bridge
   memindahkan dialog ke folder itu.
6. **Ikut dark mode.** Windows dark → Switch Explorer ikut dark tanpa restart. *Diterima jika:*
   diterapkan ≤ 300 ms setelah setting Windows berubah.
7. **Undo yang bisa dipercaya.** Sebagai power user, saya salah memindahkan folder; `Ctrl+Z`
   mengembalikannya. *Diterima jika:* undo bekerja untuk move/rename/copy/delete-ke-Recycle-Bin dalam
   sesi, dan dari halaman Aktivitas setelah restart.
8. **Hemat RAM sesuai kebutuhan.** Sebagai pengguna laptop 8 GB, saya mematikan modul yang tidak dipakai.
   *Diterima jika:* memory meter menunjukkan modul yang dimatikan tidak lagi memakan memori.

## 8. Batasan & asumsi

- Tanpa hak admin; semua integrasi ditulis di HKCU.
- Windows 10 didukung dengan fitur visual terbatas (tanpa Mica, material solid).
- Google Drive memakai OAuth client milik pengguna sendiri untuk build pribadi (CLOUD-GOOGLE-DRIVE.md).
- GPUI Kit masih pra-1.0: versi dipin, upgrade terjadwal di PR khusus.

## 9. Risiko produk

| Risiko | Mitigasi |
|---|---|
| Scope terlalu besar untuk developer solo | Fase ketat; P2 boleh digeser; Fase 0 punya kriteria go/no-go |
| Picker Takeover merusak aplikasi lain | Eksperimental, default mati, fallback otomatis, kill switch, cleanup |
| API GPUI berubah | Pin versi exact, upgrade di PR khusus, UI test |
| Verifikasi Google untuk scope penuh | Mode pribadi (client sendiri) + mode terbatas `drive.file` |
| Antivirus curiga pada perubahan registry Win+E/COM | Opt-in eksplisit, binary ditandatangani, penjelasan di UI |
| Hotkey bentrok dengan aplikasi/keyboard layout lain | Semua bisa diatur ulang, deteksi konflik, chord hanya di luar input teks |

## 10. Pertanyaan terbuka

- Lisensi (MIT atau Apache-2.0).
- Distribusi: installer saja, atau juga portable dan Microsoft Store?
- Apakah Sync dua arah masuk v1.x atau v2?
