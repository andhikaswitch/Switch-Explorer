# Laporan Fase 0 — Spike & validasi GPUI Kit

Tanggal: 2 November 2026 · Spike: [`spikes/phase0/`](../../spikes/phase0/) · Gerbang: ADR-001 di
[`DECISIONS.md`](../DECISIONS.md)

## Ringkasan & rekomendasi

**Rekomendasi: GO, dengan syarat.** Dari 7 gerbang ADR-001, tidak ada yang gagal:

- 4 lulus apa adanya (1, 2, 3, 5);
- 2 lulus dengan jalan keluar di kode kita sendiri:
  - gerbang 4: loop modal lewat window host (ADR-015);
  - gerbang 6: tema dipicu notifikasi registry;
- 1 menunggu verifikasi manual (gerbang 7, IME), karena IME Microsoft menolak input sintetis dari
  skrip, termasuk pada TextBox Win32 biasa sebagai kontrol.

Tidak ada jalan keluar yang membutuhkan fork GPUI. Kriteria evaluasi Slint (≥ 2 gagal) tidak terpenuhi.

Syarat sebelum Fase 1:
1. Verifikasi manual IME Jepang dan Windows 10 22H2 (checklist di `spikes/phase0/README.md`).
2. ADR-015 (modal host) dan ADR-016 (drop target sendiri) menjadi pola wajib di `sx-shell`.
3. ThemeWatcher memakai `RegNotifyChangeKeyValue`, bukan menunggu `WM_SETTINGCHANGE`
   (ARCHITECTURE §8.4).
4. Pin `=` untuk `gpui-kit`, `gpui-base`, `gpui-component`, `gpui-kit-assets`, ditambah `Cargo.lock`.
5. Laporkan dua masalah ke upstream:
   - panic profiler saat loop modal bersarang;
   - vsync GPUI yang mengirim `RedrawWindow` tiap refresh walau tidak ada perubahan.

## Lingkungan

| Hal | Nilai |
|---|---|
| Mesin | Laptop, Intel Core 5 210H (8 core/12 thread), RAM 24 GB |
| GPU | Intel(R) Graphics (iGPU, default) + NVIDIA GeForce RTX 3050 6GB Laptop GPU |
| Layar | 1920×1200 @ 165 Hz, skala 125% |
| OS | Windows 11 25H2, build 26200; mode dark; transparansi aktif; daya AC |
| Toolchain | Rust 1.99.1 (stable, MSVC), VS 2026 Build Tools (MSVC 14.51), Windows SDK 10.0.26100 |
| GPUI | `gpui-kit` 0.8.1, `gpui-pre` 0.3.10, `gpui-component`/`gpui-base` 0.8.1, `gpui-fast` 0.1.12 |
| Profil | release: LTO fat, `codegen-units = 1`, strip, `panic = unwind`, `+crt-static` |

## Metode

- **RAM**: private working set, sama dengan kolom Task Manager. Diambil dari aplikasi sendiri
  (`PROCESS_MEMORY_COUNTERS_EX2.PrivateWorkingSetSize`) dan dicek silang dari luar lewat WMI
  `Win32_PerfFormattedData_PerfProc_Process.WorkingSetPrivate`; keduanya cocok.
- **Frame**: event profiler GPUI (`FrameEvent::Draw` / `Present`, fitur `profiler`).
  - Scroll digerakkan tiap frame berdasarkan waktu. Skenario: 60 baris/s (`slow`), 600 baris/s (`fast`),
    baris acak tiap frame (`jump`, kasus terburuk menyeret scrollbar: semua baris baru), dan 2400 px/s
    per piksel (`pixel`, khusus `uniform_list`).
  - Warm-up 2 s, lalu diukur 10 s. Dataset 100k baris sintetis (Unicode, emoji, panjang campur), 8.8 MB.
- **Startup**: pembuatan proses (`GetProcessTimes`) → present pertama; median dari 5 kali.
- **Interaksi**: diuji otomatis dengan input sintetis dan diverifikasi dari screenshot, hash file,
  clipboard, dan log.
- Skrip pengulangan: `spikes/phase0/scripts/measure.ps1`.

## Hasil per gerbang

| # | Gerbang | Hasil | Bukti |
|---|---|---|---|
| 1 | Mica Alt + title bar kustom + snap layout | **Lulus** (Win11); Win10 menunggu VM | Warna backdrop = Explorer (`rgb(10,10,8)` vs `rgb(11,10,8)`); flyout snap muncul saat hover maximize; double-click title bar maximize/restore; mode light ikut |
| 2 | Tabel 100k ≥ 60 fps (release) | **Lulus**, margin besar | Semua skenario 165 fps; p99 interval present 7.0–7.7 ms; 0 frame > 16.7 ms |
| 3 | Idle 1 window ≤ 90 MB | **Lulus** | **36.8 MB** (di bawah budget baseline 45 MB dan target 60 MB) |
| 4 | OLE drag-out ke Chrome & Explorer | **Lulus** dengan ADR-015 | Explorer: file + folder tersalin, hash cocok; Chrome: `gambar-biru.bmp — 18486 byte — image/bmp` + pratinjau |
| 5 | Menu native `IContextMenu` | **Lulus** dengan ADR-015 | Menu lengkap berikon + dark; submenu Send to dan Give access to berikon; "Copy as path" benar-benar mengisi clipboard |
| 6 | Ikut light/dark ≤ 300 ms | **Lulus** dengan pemicu registry | registry → present **6.6–16.8 ms**; lewat `WM_SETTINGCHANGE` 575–760 ms (lihat temuan 3) |
| 7 | Input teks + IME | **Menunggu verifikasi manual** | GPUI memakai IMM32 (`ImmSetCompositionWindow`/`ImmSetCandidateWindow`); otomasi tidak mungkin |

Item roadmap tambahan:

| Item | Hasil |
|---|---|
| Drop dari Explorer/shell | Bawaan GPUI menerima path tapi **selalu COPY**. Target sendiri (ADR-016) membaca format sumber (`Shell IDList Array`, `CF_HDROP`, `FileGroupDescriptorW`, `FileContents`, …), menghitung efek dari modifier dan zona, dan menolak drop di luar zona. **Keputusan: milik sendiri.** |
| Chord `Ctrl+K, D` + `RegisterHotKey` | Chord dengan context `Workspace && !Input` tidak aktif di input teks (GPUI mengevaluasi `!` terhadap seluruh tumpukan context). OSD input → present 5.3 ms. Hotkey global 8.8 ms (debug). Bentrok dengan proses lain terdeteksi ("Hot key is already registered"). Hotkey saat app di background membuka window OSD tanpa merebut fokus. |
| `gpui-fast` vs default | **Tidak dipakai** (ADR-017), lihat tabel di bawah |

## Angka

### Startup (process → frame pertama dipresent)

| Varian | Median | Rentang | RAM saat frame pertama |
|---|---|---|---|
| upstream, iGPU | **194.9 ms** | 181.8–215.5 | 36.4–39.6 MB |
| gpui-fast, iGPU | 238.6 ms | 219.5–328.3 | 46.1–74.6 MB |
| upstream, RTX 3050 | 345.9 ms | 334.6–408.7 | 38.0–42.2 MB |

### Idle (window + data fixture, 20 s tanpa input)

| Varian | Private WS | CPU per 10 s |
|---|---|---|
| upstream, iGPU | **36.8 MB** | 234–344 ms (~2–3% satu core) |
| gpui-fast, iGPU | 39.9–42.1 MB | 16–47 ms |
| upstream, RTX 3050 | 36.5–36.7 MB | 250–281 ms |

### Scroll 100k baris (165 Hz)

| Varian | Tabel | p99 interval | Maks | p99 draw | Frame > 16.7 ms | RAM |
|---|---|---|---|---|---|---|
| upstream, iGPU | DataTable | 7.23–7.73 ms | 11.67 ms | 3.18–4.08 ms | 0 | 57–59 MB |
| upstream, iGPU | uniform_list | 6.96–7.32 ms | 8.34 ms | 2.09–3.05 ms | 0 | 61 MB |
| gpui-fast, iGPU | DataTable | 7.79–8.46 ms | **29.96 ms** | 4.05–6.49 ms | 1 | 75–137 MB |
| gpui-fast, iGPU | uniform_list | 7.33–7.86 ms | 11.74 ms | 2.21–5.81 ms | 0 | 72–179 MB |
| upstream, RTX 3050 | keduanya | 7.20–8.03 ms | 11.47 ms | 2.53–4.97 ms | 0 | 46–49 MB |

Data tambahan:
- RAM tambahan dengan 100k baris dimuat: +20–25 MB di iGPU, sesuai target ≤ 25 MB.
- Ukuran exe release: **21.75 MB** (upstream) dan 24.63 MB (gpui-fast).
- Window GPUI tambahan (OSD global): frame pertama 12.8 ms, **+13.5 MB** private working set.

## Temuan penting

1. **Panic profiler pada loop modal bersarang (gpui-pre 0.3.10).**
   - `DoDragDrop`/`TrackPopupMenuEx` yang dijalankan di dalam task foreground membuat GPUI mengeksekusi
     task lain *di dalam* task itu.
   - Dengan fitur `profiler`, `ThreadTimings::save_task_timing` lalu `expect` → panic di thread utama
     (`profiler.rs:602`), dan UI beku setelah menu ditutup.
   - `NativeMenu` gpui-kit memakai pola yang sama.
   - Solusinya ADR-015 (window host). Setelah itu, task tetap jalan setiap ~108 ms selama menu atau drag
     terbuka.
2. **Mica tidak punya fallback.** GPUI hanya memasang backdrop di build ≥ 22621, dan Root gpui-component
   melukis latar opak sehingga Mica tertutup sepenuhnya. Keduanya harus ditangani aplikasi
   (SYSTEM-INTEGRATION §3).
3. **`WM_SETTINGCHANGE` datang ~600 ms terlambat.** Broadcast itu melewati semua window top-level
   (VS Code, Chrome, Explorer, …) sebelum sampai ke kita. Notifikasi registry datang seketika.
4. **CPU idle tidak 0.** Thread `VSyncProvider` GPUI memanggil `RedrawWindow` untuk setiap window terlihat
   pada tiap refresh (165 Hz), walau tidak ada yang berubah. Ini melanggar target "CPU idle 0%"
   (PERFORMANCE.md). Usulan perbaikan upstream: invalidasi hanya bila window kotor/beranimasi.
   `gpui-fast` tidak punya masalah ini.
5. **Manifest dipaksa GPUI**, tanpa `longPathAware` (SYSTEM-INTEGRATION §15).
6. **iGPU lebih baik untuk app ini.**
   - RTX 3050 menambah ~150 ms startup, karena inisialisasi driver.
   - Di iGPU, sumber daya GPU masuk private working set (+10 MB saat 100k).
   - GPUI memilih adapter DXGI pertama, jadi preferensi GPU per-app di Windows yang menentukan.
7. **Dua versi crate `windows`** ikut terkompilasi (0.58 lewat gpui-base/gpui-component, 0.62 lewat
   platform). Ini menambah waktu build, dan perlu dicek ulang tiap upgrade.
8. **Hit test title bar memakai posisi pointer terakhir yang diketahui GPUI.** Satu lompatan kursor
   sintetis tidak memicu snap flyout, tapi gerakan pointer biasa memicunya. Tidak berdampak ke pengguna.

## Review `windows-integration-reviewer`

Review menemukan 0 defect kritis dan 2 defect tinggi di drop target. Keduanya sudah diperbaiki dan
diuji ulang:

1. **Efek mengabaikan `*pdweffect` (efek yang diizinkan sumber).** Shift bisa menghasilkan MOVE walau
   sumber hanya mengizinkan COPY|LINK. Sekarang efek selalu diiriskan dengan izin sumber. Uji ulang:
   drag dengan Shift dari sumber COPY|LINK ditolak.
2. **Drop melaporkan MOVE/LINK ke sumber tanpa melakukan apa pun.** Sumber non-optimized bisa
   menghapus file aslinya. Sekarang spike membalas `DROPEFFECT_NONE` dan hanya melaporkan efek yang
   *akan* dilakukan ke UI. Di produksi, efek yang dibalas harus persis sama dengan yang dijalankan
   `sx-ops`.

Ikut diperbaiki:
- modal host memakai antrean thread-local (tanpa pointer di `lparam`), penjaga *busy* agar job tidak
  bersarang, dan `init()` di UI thread;
- subclass menu memakai guard RAII;
- `WM_MENUCHAR` hanya diteruskan ke `IContextMenu3`;
- uxtheme dimuat sekali;
- `tymed` dicek dan path dibaca dengan `OsString::from_wide`;
- akhir `EnumFormatEtc` dicek `== S_OK` dan `ptd` dibebaskan;
- `WM_LBUTTONUP` sintetis hanya dikirim bila tombol sudah dilepas.

Risiko yang dibawa ke produksi (Fase 1–2):
- Verb shell `delete`/`cut`/`paste`/`rename` harus dicegat dan dijalankan lewat `sx-ops` (journal +
  undo, aturan 5), bukan `InvokeCommand`.
- Panggilan shell yang bisa lambat (UNC offline) berjalan di UI thread.
- Drop target perlu guard yang memanggil `RevokeDragDrop` sebelum window hancur.
- Theme watcher dan hotkey thread belum punya jalur berhenti.
- Clipboard dan picker shim belum direview.

## Keterbatasan pengukuran (jujur)

- Satu mesin, layar 165 Hz. Angka di 60 Hz tidak diukur, meski p99 ≤ 7.7 ms memberi ruang besar.
- Scroll digerakkan lewat API scroll tiap frame, bukan event roda/touchpad nyata. Biaya penanganan
  input tidak ikut terhitung.
- Profiler aktif selama benchmark (overhead kecil).
- IME, Windows 10, dan tampilan drag image belum diverifikasi manusia. Tampilan Mica hanya
  dibandingkan secara warna dengan Explorer.
- Uji light/dark memakai toggle sintetis: tulis `AppsUseLightTheme` + broadcast seperti Settings, lalu
  dipulihkan.

## Status checklist ROADMAP Fase 0

- [x] Window Mica Alt + title bar kustom + snap layout (Win11) · [ ] fallback Windows 10 di VM
- [x] Tabel/VirtualList 100k ≥ 60 fps di release; RAM dicatat
- [x] Idle 1 window ≤ 90 MB; baseline 36.8 MB
- [x] OLE drag-out ke Chrome & Explorer
- [x] Drop dari Explorer: diputuskan drop target sendiri
- [x] Menu native `IContextMenu`, submenu Send to berikon, perintah berjalan
- [x] Light ↔ dark ≤ 300 ms (pemicu registry)
- [ ] Input teks + IME Jepang (verifikasi manual)
- [x] Chord `Ctrl+K, D` + `RegisterHotKey` global
- [x] Default vs `gpui-fast` → default
- [x] Keputusan go/no-go tertulis (laporan ini)
