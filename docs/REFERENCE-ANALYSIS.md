# Analisis Referensi — FariSYNC

Sumber: 15 screenshot dari reel pembuatnya (TS) dan caption-nya. Dokumen ini memisahkan **yang
terlihat** dari **yang disimpulkan**, lengkap dengan tingkat keyakinan.

## 1. Ringkasan

FariSYNC ("File Syncronizer") bukan file explorer penuh. Ia adalah dashboard + tool pengelola file:
ringkasan storage (cloud & disk), kategori Quick Access, Recent Files, dan fitur **Organizer** yang
mengelompokkan isi folder berdasarkan kategori/ekstensi lalu memindahkannya. Fitur lain (Syncronizer,
Double Killer, Bulk Renamer, Space Analyzer) ada di menu, sebagian berlabel *Upcoming*. Kekuatan utamanya
ada di tampilan: translucent, rounded, ungu-putih, dan ikut dark mode Windows.

## 2. Teknologi yang dipakai (inferensi)

| Observasi | Kesimpulan | Keyakinan |
|---|---|---|
| Dialog konfirmasi native berjudul "Tauri" (gambar 14) | Dibangun dengan Tauri; dialog dari plugin dialog tanpa judul kustom | Tinggi |
| Pill kecil di bawah tengah dengan logo segitiga-gunung + "134 ms" / "95 ms" / "103 ms" | Nuxt DevTools → frontend Nuxt (Vue); screenshot diambil dari dev build | Tinggi |
| Wallpaper tampak buram di balik window | Window effect Mica/Acrylic (fitur window effects Tauri atau crate window-vibrancy) | Tinggi |
| Tidak ada frame standar; tombol min/max/close digambar sendiri | Window tanpa dekorasi + title bar kustom | Tinggi |
| Setting Windows diganti ke Dark → app ikut dark seketika (gambar 4 → 5) | CSS `prefers-color-scheme` di WebView2 atau event tema Tauri | Tinggi |
| Toast progress muncul di pojok kanan bawah layar, di luar window utama, dengan pill DevTools sendiri (gambar 15) | Window WebView kedua khusus progress | Sedang |
| Kartu storage: OneDrive 2/5 GB, Mega 1/10 GB, Google Drive 100 GB/2.00 TB | Angka bulat + caption "nextnya tinggal konekin cloud" → data cloud masih placeholder | Sedang–tinggi |
| C:\ Sys 335 GB di satu screenshot, 335.01 GB di screenshot lain | Data disk asli (API kapasitas volume) | Tinggi |

## 3. Pengamatan per screenshot

| # | Yang terlihat | Catatan desain |
|---|---|---|
| 1 | Dashboard light, window sempit. Sidebar: Dashboard aktif berupa pill ungu gradien + glow. Storage (05) kartu horizontal, Quick Access (Documents 100, Audio 20, Pictures), Recent Files (178) | Hierarki jelas; kartu storage terpotong (scroll horizontal) |
| 2 | Versi lebar: 4 kartu storage termasuk "C:\ Sys", 4 kategori (+Videos), daftar PDF | Layout responsif; kolom Date hanya menampilkan tahun "2025" |
| 3 | Menu Features terbuka: Organizer, Syncronizer, lalu Double Killer/Bulk Renamer/Space Analyzer berlabel *Upcoming* (abu-abu) | Label miring ungu; scrollbar kustom ungu di tepi sidebar |
| 4 | Settings Windows → Personalization → Colors; pengguna memilih Dark | Demo ikut tema sistem |
| 5 | App berubah dark: kartu hitam pekat, teks putih, aksen ungu tetap | Dark = hampir hitam, bukan navy |
| 6 | Dark lebar; kartu Google Drive saat di-hover berubah jadi blok ungu penuh dengan teks putih | Hover state sangat kuat (fill penuh) |
| 7 | Halaman Organize: "History Organize Folder" dengan bar tersegmentasi per kategori + total size, Organize Presets, drop zone "Choose Folder" | Bar segmen = ringkasan komposisi folder yang efektif |
| 8 | Folder "Received" di-drag dari Windows Explorer ke drop zone: "Drop your files or folders here" | State drop target jelas |
| 9 | Setelah scan: "Folder 1 D:\Received", total 53.24 GB, tombol Remove merah, Select Files (By Category / By Extension), Found Files (207) + List/Grid, toast "Scan Folder Complete!" | Toast punya progress bar |
| 10 | 2 file dipilih: kartu kategori menampilkan 1/39; action bar melayang di bawah: Add Folder + Organize Files ▾ (by Category / by Extension / by Selected) | Split button di floating bar |
| 11 | Mode By Extension: grid chip `.jpg (1/44)`, `.pdf (1/24)`, `.zip (0/20)`, dst. | Chip dengan hitungan terpilih/total |
| 12 | Semua .jpg & .pdf dipilih → Selected Files (68), tombol berubah jadi "Process Now" | Label aksi berubah sesuai konteks |
| 13 | By Category: Documents 39/39 | — |
| 14 | Dialog native "File will be move to destination, Are you sure?" berjudul "Tauri" | Tidak konsisten dengan desain, grammar salah, tanpa undo |
| 15 | Toast progress di pojok layar: "Laporan Sistem Tertanam.pdf (22/39) 56.41%", 35.95 MB / 94.49 MB, Cancel | Tanpa kecepatan/ETA; window terpisah |

## 4. Alur Organizer (direkonstruksi)

1. Pilih folder (tombol atau drag dari Explorer) → scan rekursif.
2. Ringkasan per kategori (jumlah & ukuran) → pilih file per kategori, per ekstensi, atau manual.
3. "Organize Files" (by Category/Extension/Selected) → konfirmasi native → file dipindahkan ke subfolder
   kategori → toast progress per file + Cancel.
4. Hasil tercatat di "History Organize Folder".

## 5. Analisis caption

| Caption | Makna | Pelajaran untuk Switch Explorer |
|---|---|---|
| "tau gitu dari awal gw pake tauri ketimbang elektron ... rust kenceng bet" | Pindah dari Electron ke Tauri dan merasakan backend Rust yang cepat | Tauri hemat ukuran installer dan backend-nya Rust, tapi UI-nya tetap WebView2 (berbasis Chromium) — RAM dan waktu startup tetap dibatasi WebView2. Switch Explorer membuat **UI-nya juga native Rust** (GPUI Kit) supaya ringan dari ujung ke ujung. |
| "aplikasi gw dah setara microsoft ... enak bet bikin tema translucent atau ala ala fluent design" | Efek translucent/Fluent mudah dibuat dengan CSS + window effect | Efek yang sama didapat secara native: backdrop DWM `MicaAltBackdrop` bawaan GPUI + token warna semi-transparan. "Setara Microsoft" yang sebenarnya butuh integrasi shell: context menu asli, thumbnail sistem, OLE drag & drop, clipboard, Recycle Bin. Itu inti Fase 2 kita. |
| "nextnya tinggal konekin cloud" | Integrasi cloud belum dibuat | Kartu cloud di screenshot kemungkinan placeholder. Cloud kita dirancang dari awal sebagai modul lazy. |

## 6. Bagaimana koneksi cloud-nya bekerja?

Tidak ada bukti alur cloud di screenshot. Drag dari Windows Explorer (gambar 8) masuk ke **drop zone
Organizer**, bukan ke Google Drive. Kalau nanti diimplementasikan, pola umumnya: login OAuth di browser
sekali → token disimpan → API dipakai untuk kuota (kartu storage) dan upload. Kemungkinan lain: membaca
drive virtual `G:\` milik Google Drive for desktop (kuota terbaca sebagai kapasitas disk).

Rancangan kita (detail di `docs/CLOUD-GOOGLE-DRIVE.md`): browser hanya terbuka **sekali** saat connect,
setelah itu semuanya di dalam Switch Explorer — klik kanan → Upload/Pindahkan ke Drive → popup berisi pohon
folder Drive → pilih atau buat folder baru → upload. Browse Drive di sidebar juga tersedia secara lazy
(hanya metadata, dimuat saat dibuka) sehingga tambahan RAM-nya kecil dan nol saat tidak dipakai.

## 7. Adopsi, perbaiki, hindari

| Adopsi | Perbaiki | Hindari |
|---|---|---|
| Item aktif sidebar yang ber-glow (kita: Tide Slash) | Dialog native "Tauri" → sheet in-app bertema + toast undo | Data placeholder yang terlihat seperti data asli |
| Kartu storage dengan bar pemakaian (kita: gauge ringkas) | Kolom tanggal hanya tahun → tanggal lengkap atau relatif | Overlay DevTools di build rilis |
| Bar tersegmentasi komposisi folder | Ukuran terpotong 2 baris ("344.83 KB") → kolom angka rata kanan, angka tabular | Window WebView tambahan untuk toast (boros RAM) |
| Chip ekstensi dengan hitungan | Progress tanpa kecepatan/ETA → MB/s, sisa waktu, pause/resume | Hover fill penuh di semua kartu (terlalu ramai) |
| Floating action bar saat ada seleksi | Organizer tanpa pratinjau → dry-run sebelum/sesudah + deteksi konflik | Konfirmasi modal untuk setiap aksi |
| Ikut dark mode Windows otomatis | Kategori "Other" sebagai keranjang sampah → deteksi tipe via magic bytes (opsional) | Palet ungu (kita: biru Cerulean) |

## 8. Pemetaan fitur

| FariSYNC | Switch Explorer | Dokumen |
|---|---|---|
| Dashboard | Home: gauge drive & cloud, kategori pintar, pinned, recent, aktivitas | FEATURES.md (F-HOME) |
| Storage (05) + Add Storage | Drive lokal (data asli) + akun cloud (kuota asli via API) | CLOUD-GOOGLE-DRIVE.md |
| Quick Access kategori + Add Category | Kategori pintar; taksonominya dipakai bersama Organizer & warna kategori | FEATURES.md (F-HOME-03) |
| Recent Files | Recent dari Recent Items Windows (tanpa tracking sendiri) | FEATURES.md (F-HOME-04) |
| Organizer | Organizer + template path tujuan + dry-run + undo | FEATURES.md (F-TOOL-01) |
| Syncronizer | Sync (setelah v1) | ROADMAP.md |
| Double Killer | Duplicate Finder | FEATURES.md (F-TOOL-02) |
| Bulk Renamer | Bulk Rename dengan pratinjau | FEATURES.md (F-TOOL-03) |
| Space Analyzer | Space Analyzer (treemap) | FEATURES.md (F-TOOL-04) |
| History | Aktivitas (journal operasi + undo per entri) | FEATURES.md (F-HIST) |
| Settings | Settings + Developer Mode + Theme Studio | DEVELOPER-MODE.md, THEMING.md |
