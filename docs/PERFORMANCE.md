# Performa

Target utama: terasa lebih cepat dan memakai RAM lebih kecil daripada File Explorer pada skenario yang
sama. Semua angka diukur di build `--release` pada laptop kelas menengah (NVMe, Windows 11). Budget di
bawah adalah target awal; baseline final dikunci setelah Fase 0.

## 1. Budget

| Metrik | Target | Batas keras |
|---|---|---|
| Cold start → frame interaktif pertama (Home) | ≤ 200 ms | ≤ 300 ms |
| Buka window baru (resident/instance hidup) | ≤ 80 ms | ≤ 120 ms |
| Folder 10k item: baris pertama tampil | ≤ 30 ms | ≤ 50 ms |
| Folder 10k item: listing lengkap | ≤ 150 ms | ≤ 250 ms |
| Folder 100k item: baris pertama / lengkap | ≤ 80 ms / ≤ 1.2 s | ≤ 120 ms / ≤ 2 s |
| Ganti sort 100k item | ≤ 100 ms | ≤ 200 ms |
| Scroll | 60 fps stabil (p99 frame ≤ 16.7 ms); 120 fps di layar 120 Hz | p99 ≤ 25 ms |
| Filter per ketikan (10k item) | ≤ 8 ms | ≤ 16 ms |
| Hot Switch: tombol → OSD tampil | ≤ 30 ms | ≤ 50 ms |
| Ganti tema/aksen | ≤ 50 ms | ≤ 100 ms |
| RAM idle, 1 window, Home | ≤ 60 MB | ≤ 90 MB |
| RAM tambahan folder 100k item | ≤ 25 MB | ≤ 40 MB |
| RAM resident tanpa window | ≤ 25 MB | ≤ 35 MB |
| CPU saat idle | 0% (tanpa polling) | — |
| Ukuran exe release | ≤ 30 MB | ≤ 45 MB (dikunci ulang setelah Fase 0) |

RAM = *private working set* (kolom "Memory (active private working set)" di Task Manager), diukur sama
untuk Switch Explorer dan Explorer.

**Baseline Fase 0** (spike, iGPU, 165 Hz, release; rinci di `docs/benchmarks/phase0.md`):

| Metrik | Terukur |
|---|---|
| Cold start → frame pertama | 194.9 ms |
| Idle 1 window | 36.8 MB |
| Dengan 100k baris | 57–61 MB |
| Scroll 100k, p99 | 7.0–7.7 ms |
| Ukuran exe | 21.75 MB |

**Baseline Fase 1** (aplikasi lengkap, iGPU, 165 Hz, release; rinci di `docs/benchmarks/phase1.md`):

| Metrik | Terukur |
|---|---|
| Folder 10k → baris pertama | 24,6–32,8 ms |
| `main` → frame berisi baris | 207–304 ms (median 260 ms) |
| Idle Home / folder 10k | 39,2 / 42,0 MB |
| Ukuran exe | 23,26 MB |

Aturan dari Fase 1: jangan pakai path GPUI (`paint_path`) di chrome yang selalu terlihat. Path pertama
membuat tekstur MSAA seukuran window terpakai (+~29 MB di iGPU). Bentuk khusus dirasterisasi di CPU lalu
dilukis sebagai gambar.

Budget di atas tetap berlaku. Dua catatan:
- **CPU idle 0% belum tercapai:** thread vsync GPUI mengirim `RedrawWindow` tiap refresh, terukur
  ~2–3% satu core di 165 Hz. Ini dikecualikan sampai ada perbaikan upstream.
- **Startup di dGPU 346 ms** (melewati batas keras), jadi pengukuran resmi memakai GPU default (iGPU).

## 2. Budget memori per subsistem

| Subsistem | Budget | Catatan |
|---|---|---|
| Baseline GPUI + window | ≤ 45 MB | Diukur di Fase 0 |
| Font & glyph atlas | ≤ 8 MB | |
| Cache ikon | ≤ 8 MB | `perf.icon_cache_entries` |
| Cache thumbnail | 64 MB (bisa diatur) | LRU berbasis byte, `perf.thumbnail_cache_mb` |
| Snapshot folder aktif (100k) | ≤ 12 MB | ARCHITECTURE.md §9 |
| Snapshot cache (8 folder) | ≤ 16 MB | `perf.dir_cache_count` |
| Cloud aktif | ≤ 10 MB | 0 saat mati/idle |
| Tools | Sesuai tool | 0 saat tidak dibuka |

Memory meter (Developer Mode) menampilkan angka-angka ini saat runtime.

## 3. Cara membandingkan dengan Explorer secara adil

1. Explorer: aktifkan Folder Options → View → "Launch folder windows in a separate process" agar window
   Explorer punya proses sendiri.
2. Buka folder fixture yang sama (mis. 10k item), mode details, tunggu 10 detik idle.
3. Catat private working set kedua proses (Task Manager atau Process Explorer/VMMap).
4. Waktu: `switch-explorer.exe --bench-open <path>` mencetak timestamp (start proses → frame pertama →
   baris pertama → listing lengkap). Untuk Explorer gunakan rekaman layar 120 fps atau WPR/WPA.
5. Ulangi 5 kali, pakai median. Simpan laporan di `docs/benchmarks/` (dibuat oleh `/perf-audit`).

## 4. Teknik

**Startup**
- Single-instance check sebelum inisialisasi UI; hanya settings/keymap/tema yang dibaca sinkron.
- Tab non-aktif tidak dienumerasi sampai dibuka; modul opsional lazy (ARCHITECTURE.md §13).
- Prewarm ikon ekstensi umum saat idle, bukan saat start.

**Listing**
- Enumerasi batch lewat `GetFileInformationByHandleEx(FileFullDirectoryInfo)` (buffer 64 KB): satu
  panggilan sistem memberi banyak entri lengkap dengan atribut/ukuran/waktu — tanpa stat per file.
- Batch pertama dirender segera; sisanya mengalir.
- Model ringkas (arena nama, indeks u32), kunci sort & lowercase dihitung sekali.

**Render**
- Daftar & grid tervirtualisasi (hanya baris terlihat + buffer kecil).
- Fungsi render murni: tanpa I/O, tanpa format string mahal per frame; teks tampilan di-cache per baris.
- `cx.notify()` hanya pada entity yang berubah; coalesce update watcher & progress (≤ 1 per frame,
  progress ≤ 10 Hz).
- Evaluasi fitur `gpui-fast` (retained rendering) di Fase 0.

**Ikon & thumbnail**
- Ikon per ekstensi dari system image list (sekali per ekstensi).
- Thumbnail dua pass: cache Windows dulu (instan), ekstraksi kemudian; hanya untuk item terlihat; dibatalkan
  saat item keluar layar; didecode seukuran tampilan.

**Memori**
- Cache punya budget & LRU; minimize > 60 s → buang thumbnail; notifikasi low-memory Windows → kosongkan
  cache.
- Runtime jaringan dimatikan saat idle; tools dibongkar saat ditutup.

**Binary**
- Profil release: LTO fat, `codegen-units = 1`, strip simbol (PDB disimpan terpisah untuk crash).
- Cek dependency baru dengan `cargo bloat`; hindari crate besar untuk fungsi kecil.

## 5. Anti-pattern (dilarang)

- I/O, panggilan shell, atau `std::fs` di fungsi render atau di UI thread.
- Sort/filter besar di UI thread.
- Alokasi `String` per entri saat enumerasi (pakai arena).
- Timer polling (gunakan event: watcher, notifikasi sistem).
- Cache tanpa batas.
- Clone vektor besar per frame.
- Logging di loop panas pada level `info`+.
- Menunggu future secara blocking di UI thread.

## 6. Alat ukur

| Alat | Untuk |
|---|---|
| Perf HUD & FPS Monitor GPUI Kit | FPS, frame time, RAM saat runtime |
| `tracing` + `tracing-tracy` (feature `profiling`) | Timeline span detail |
| Fitur `profiler` GPUI Kit | Profil render |
| Windows Performance Recorder/Analyzer | CPU, I/O, startup sistem |
| Process Explorer, VMMap, RAMMap | Rincian memori |
| `criterion` | Benchmark sort, filter, parser tema, palet |
| `cargo bloat` | Ukuran binary per crate/fungsi |
| `cargo xtask fixtures` | Folder uji: 1k, 10k, 100k, 1M (flat), pohon dalam (kedalaman 30), nama panjang, Unicode (Indonesia/Jepang/emoji), path > 260 karakter, gambar asli untuk thumbnail |

## 7. CI

GitHub Actions `windows-latest`: fmt, clippy, test, benchmark smoke (tren saja, tidak memblokir karena VM
CI berisik), dan peringatan ukuran binary bila melewati budget.
