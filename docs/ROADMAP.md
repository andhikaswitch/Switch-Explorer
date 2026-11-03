# Roadmap

Estimasi kasar untuk developer solo yang bekerja bersama Claude Code. Setiap fase selesai bila semua
acceptance criteria tercentang. Fase boleh digeser, tetapi Fase 0 wajib lolos sebelum Fase 1.

| Fase | Fokus | Estimasi |
|---|---|---|
| 0 | Spike & validasi teknologi | 1–2 minggu |
| 1 | Fondasi & explorer MVP | 3–5 minggu |
| 2 | Paritas Explorer + Drag Guard + Hot Switch | 4–6 minggu |
| 3 | Personalisasi & Developer Mode | 2–4 minggu |
| 4 | Google Drive | 2–4 minggu |
| 5 | Integrasi sistem (explorer utama, Picker Bridge) | 2–3 minggu |
| 6 | Picker Takeover (eksperimental) | 3–5 minggu |
| 7 | Tools (Organizer dkk.) | 3–6 minggu |
| 8 | Polish & rilis v1.0 | 2–3 minggu |

## Fase 0 — Spike & validasi

**Tujuan:** membuktikan GPUI Kit memenuhi kebutuhan sebelum membangun di atasnya (ADR-001).
**Deliverable:** `spikes/phase0/` (app minimal, boleh dibuang) + laporan `docs/benchmarks/phase0.md`.

Hasil: [`docs/benchmarks/phase0.md`](benchmarks/phase0.md). Rekomendasi: **GO dengan syarat**.

- [x] Window Mica Alt + title bar kustom + snap layout saat hover tombol maximize (Win11 22H2+).
- [ ] Di Windows 10, fallback solid tanpa crash. Kode memilih `Opaque` di build < 22621; perlu verifikasi
      di VM.
- [x] Tabel/VirtualList 100k baris: scroll ≥ 60 fps di release; RAM dicatat (p99 7.7 ms @165 Hz, 57–61 MB).
- [x] Idle 1 window ≤ 90 MB private working set; baseline 36.8 MB.
- [x] OLE drag-out file dari window GPUI ke Chrome (area upload) dan ke Explorer berhasil (ADR-015).
- [x] Drop dari Explorer ke window: path diterima; diputuskan drop target milik sendiri (ADR-016).
- [x] Menu native `IContextMenu` tampil, submenu "Send to" berikon, perintah berjalan (ADR-015).
- [x] Windows light ↔ dark → app ikut ≤ 300 ms tanpa restart (7–17 ms lewat pemicu registry).
- [ ] Input teks + IME (Microsoft IME Jepang) bekerja. Otomasi tidak mungkin; menunggu verifikasi manual.
- [x] Hotkey chord `Ctrl+K, D` dan `RegisterHotKey` global bekerja berdampingan dengan loop GPUI.
- [x] Bandingkan default vs fitur `gpui-fast` (fps & RAM) → default (ADR-017).
- [x] Keputusan go/no-go tertulis di laporan; tidak ada kriteria yang gagal.

## Fase 1 — Fondasi & explorer MVP

- [x] Workspace + semua crate kosong sesuai ARCHITECTURE.md; `xtask` (fixtures, cleanup-registry); CI
      (fmt, clippy, test) di `windows-latest`.
- [x] `sx-settings`: `define_flags!`, snapshot, persistensi `settings.toml` (atomic), lapisan sesi, `--set`.
- [x] `sx-theme`: parser, OKLCH, palet, resolver, validator; `themes/cerulean.toml` dari THEMING.md §5;
      CI memeriksa kontras tema bawaan (termasuk sapuan 36 aksen).
- [x] Window: title bar + tab, Mica Alt, ikut light/dark sistem, single instance + `OpenPaths`.
- [x] Sidebar: Home (placeholder), Pinned (pin/unpin lewat bintang di command bar, `pins.json`), This PC +
      gauge, Recycle Bin (buka di Explorer dulu).
- [x] Details view tervirtualisasi: kolom Nama/Diubah/Tipe/Ukuran, sort, folders-first, natural sort.
- [x] Navigasi back/forward/up, breadcrumb, address bar (path, relatif, `%VAR%`, `~`, `shell:`),
      type-to-select.
- [x] Ikon sistem + cache; watcher. Catatan: watcher memuat ulang folder secara debounce (250 ms) sambil
      mempertahankan daftar dan seleksi, belum diff inkremental per entri.
- [x] Buka file (`ShellExecuteEx`); copy/cut/paste (interop clipboard Explorer), delete ke Recycle Bin, rename F2,
      folder baru; progress toast + panel Transfer dasar; sheet konflik; undo/redo + journal + pemulihan crash.
      Catatan: copy memakai `std::fs::copy` (CopyFileEx) dengan progress per item; `CopyFile2` dengan
      progress byte dan pause menyusul (ADR-021).
- [x] i18n `en` + `id`; status bar (jumlah, seleksi, sisa ruang).
- [x] Budget: cold start ≤ 300 ms, folder 10k baris pertama ≤ 50 ms, RAM idle dicatat
      (`docs/benchmarks/phase1.md`).

Belum termasuk Fase 1, dicatat untuk Fase 2 (temuan review): ~~navigasi keyboard di luar daftar (F6, fokus
tombol/tab/sidebar)~~, ~~tooltip tombol ikon~~, nama tampilan shell untuk folder pinned, ~~collapse
breadcrumb panjang~~, ~~kolom yang bisa diubah lebarnya~~, ~~progress taskbar~~ (yang dicoret sudah selesai).

## Fase 2 — Paritas Explorer + Drag Guard + Hot Switch

- [ ] Context menu bertema + "Tampilkan opsi lain" (menu native, forwarding pesan), menu background & sidebar.
- [ ] `CommandRegistry`; command palette (`Ctrl+Shift+P`); quick open (`Ctrl+P`).
- [ ] OLE drag & drop dua arah (termasuk virtual files) + drag internal ke folder/sidebar/breadcrumb/tab.
- [ ] **Drag Guard** lengkap (DEVELOPER-MODE.md §5) + preset "Anti salah drag" + test state machine.
      Progres: aturan §5.3 dan state machine §5.4 di `sx_core::drag` dengan tabel test; preset bisa di-toggle
      lewat Hot Switch. Belum: sambungan ke DnD.
- [ ] **Hot Switch**: `Ctrl+Shift+D`, chord `Ctrl+K` (F, D, S, E, G, L, H, X, T, R, K); OSD ≤ 50 ms; chip status;
      badge title bar; hint saat diblokir; persisten/sesi; chord tidak aktif di input teks.
      Progres: semua binding kecuali `K` (panel), OSD (ganti di tempat, hover menahan, fade), chip "Drag: …"
      dengan tooltip, badge title bar, persisten/sesi lewat `hotswitch.persist` (ditulis di thread sendiri),
      chord mati di input teks, `view.show_extensions` kini berlaku di daftar. Belum: panel Hot Switch, hint
      saat drag diblokir (menunggu DnD), pengukuran OSD ≤ 50 ms, OSD `system` untuk hotkey global.
- [ ] Grid/gallery + thumbnail dua pass; preview pane (gambar/teks + spike preview handler); details pane;
      Quick Look (`Space`).
- [ ] Filter instan + pencarian rekursif + integrasi Everything.
      Progres: mesin pencarian rekursif di `sx-search` (operator `ext:`, `size:`, `modified:`, `kind:`,
      wildcard, frasa berkutip; hasil streaming, batal, link tidak diikuti, folder tak terbaca dilewati).
      Belum: kotak cari + kolom Path di UI, filter instan, Everything.
- [ ] Split view, session restore, mode tampilan per folder, checkbox seleksi, floating action bar.
- [ ] Home dashboard (gauge, kategori pintar, pinned, terbaru, aktivitas); halaman Aktivitas; Recycle Bin view.
- [ ] Properties (`Alt+Enter`), copy path, buka terminal/VS Code, mode baca-saja, elevasi via `IFileOperation`.
- [ ] Semua shortcut di FEATURES.md bekerja.

## Fase 3 — Personalisasi & Developer Mode

- [ ] Settings lengkap (digenerate dari metadata) + onboarding singkat saat pertama jalan.
- [ ] Developer Mode (unlock 7× klik versi), halaman semua switch dengan filter stabilitas & pencarian.
- [ ] Preset/profil (simpan, ekspor, impor); Location Rules + indikator di status bar.
- [ ] Keymap editor: rekam, konflik (internal/sistem/Explorer/AltGr), reset, impor/ekspor `keymap.toml`;
      scope global via `HotkeyService`.
- [ ] Theme Studio (Cepat/Token/File), adaptasi otomatis light↔dark, panel kontras + perbaikan otomatis.
- [ ] Token Inspector; hot reload tema; impor tema GPUI Kit; tema bawaan `abyss`, `glacier`, `high-contrast`.
- [ ] Sumber aksen Windows/kustom/wallpaper; material & intensitas; gambar latar; densitas; skala font.
- [ ] Perf HUD, memory meter, log viewer, halaman Config & Integrasi.

## Fase 4 — Google Drive

- [ ] Connect/disconnect (PKCE loopback, Credential Manager, revoke); kartu akun & kuota.
- [ ] Upload/Pindahkan ke Drive + popup pohon folder (lazy), buat folder baru, tujuan favorit, kebijakan konflik.
- [ ] Upload resumable dialirkan dari disk + MD5 + resume setelah restart + verifikasi sebelum hapus lokal.
- [ ] Panel Transfer: pause/resume/cancel/retry, concurrency, batas bandwidth.
- [ ] Browse Drive di sidebar, buka/unduh, drag dua arah (Drag Guard berlaku).
- [ ] Runtime lazy + idle shutdown; memory meter menunjukkan 0 saat idle; test mock untuk semua jalur error.

## Fase 5 — Integrasi sistem

- [ ] Default folder handler + Win+E (registry dengan backup, status, perbaiki, cleanup).
- [ ] Resident mode + tray + startup opsional; buka window ≤ 80 ms.
- [ ] Entri menu Explorer & Send To → Drive.
- [ ] Picker Bridge (deteksi, panel, `Ctrl+G`, navigasi via UIA, pilih file, file Drive).
- [ ] Explorer redirect (eksperimental).
- [ ] Matriks uji kompatibilitas Bridge (TESTING.md §5) terisi.

## Fase 6 — Picker Takeover (eksperimental)

- [ ] `sx-picker-shim` x64 + x86 (proxy `comdlg32`, interface sesuai SYSTEM-INTEGRATION.md §14).
- [ ] Window mode picker (Open/Save/Folder) dengan semantik `FOS_*`.
- [ ] Fallback di setiap langkah, kill switch (4 jalur), penghitung kegagalan, diagnostik picker.
- [ ] Matriks uji Takeover terisi; aplikasi bermasalah masuk daftar pengecualian bawaan.
- [ ] Tetap default mati & hanya muncul di Developer Mode.

## Fase 7 — Tools

- [ ] Organizer (paritas FariSYNC + template + dry-run + undo + preset).
- [ ] Duplicate Finder, Bulk Rename, Space Analyzer.
- [ ] Kompres/ekstrak ZIP; cari isi file (beta).

## Fase 8 — Polish & rilis v1.0

- [ ] Audit aksesibilitas (keyboard, AccessKit, kontras, high contrast, text scale).
- [ ] Audit performa terhadap semua budget; audit keamanan; `cargo-deny`/`cargo-audit` bersih.
- [ ] Installer + portable zip; auto-update bertanda tangan (ikuti panduan Packaging/Auto Update GPUI Kit);
      uninstaller menjalankan cleanup registry lebih dulu.
- [ ] Dokumentasi pengguna singkat; changelog; halaman Tentang dengan kredit lisensi.

## Setelah v1

Sync satu/dua arah, tag file, OneDrive/Dropbox/rclone, entri menu Windows 11 tingkat atas (package identity),
perangkat MTP, kemungkinan port macOS/Linux.

## Register risiko

| Risiko | Dampak | Kemungkinan | Mitigasi |
|---|---|---|---|
| API GPUI/GPUI Kit berubah | Sedang | Tinggi | Pin exact, upgrade di PR khusus, UI test |
| OLE DnD/menu native sulit di loop GPUI | Tinggi | Sedang | Dibuktikan di Fase 0 |
| Preview handler (child HWND) bentrok dengan backdrop | Rendah | Sedang | Spike Fase 2; fallback preview bawaan saja |
| Picker Takeover merusak aplikasi | Tinggi | Sedang | Eksperimental, proxy, fallback, kill switch |
| Verifikasi Google | Sedang | Tinggi (untuk rilis publik) | Mode pribadi + mode `drive.file` |
| Antivirus menandai integrasi registry | Sedang | Sedang | Opt-in, tanda tangan kode, transparansi |
| Scope creep | Tinggi | Tinggi | Fase ketat, P2 boleh digeser |
| Bentrok hotkey dengan aplikasi/layout lain | Rendah | Sedang | Semua bisa diatur, deteksi konflik |
