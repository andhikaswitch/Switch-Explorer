# Indeks Dokumentasi

Peta dokumen untuk manusia dan untuk Claude Code. Semua dokumen berbahasa Indonesia; istilah teknis
tetap berbahasa Inggris supaya cocok dengan nama di kode.

## Urutan baca

**Untuk pemilik proyek (sekali di awal):**
1. `README.md` — gambaran 2 menit.
2. `docs/REFERENCE-ANALYSIS.md` — apa yang dipelajari dari FariSYNC.
3. `docs/PRD.md` — apa yang dibangun dan apa yang sengaja tidak dibangun.
4. `docs/DECISIONS.md` — kenapa GPUI Kit (native Rust), bukan Tauri.
5. `docs/DESIGN.md` lalu `docs/THEMING.md` — tampilan dan sistem warna.
6. `docs/DEVELOPER-MODE.md` — semua switch, Drag Guard, dan Hot Switch.
7. `docs/ROADMAP.md` lalu `docs/PROMPTS.md` — urutan kerja dan cara menyuruh Claude Code.

**Untuk Claude Code (setiap tugas):** `CLAUDE.md` (otomatis) → tabel "Peta dokumen" di sana → hanya
dokumen yang relevan. Jangan memuat semua dokumen sekaligus.

## Daftar dokumen

| Dokumen | Isi | Dipakai saat |
|---|---|---|
| `PRD.md` | Masalah, persona, tujuan, prioritas, user story | Menentukan scope & prioritas |
| `REFERENCE-ANALYSIS.md` | Analisis 15 screenshot + caption FariSYNC | Butuh konteks inspirasi UI |
| `ARCHITECTURE.md` | Crate, proses, thread, alur data, persistensi, IPC | Menyentuh struktur/alur |
| `DECISIONS.md` | ADR-001 s.d. ADR-014 | Mau mengubah keputusan teknis |
| `DESIGN.md` | Identitas visual, layout, wireframe, komponen, OSD, copy | Membuat/mengubah UI |
| `THEMING.md` | Token, sintaks nilai, tema Cerulean, resolver, Theme Studio | Menyentuh warna/tema |
| `FEATURES.md` | Spesifikasi fitur ber-ID + shortcut | Mengimplementasi fitur |
| `DEVELOPER-MODE.md` | Registry flag, tabel flag, preset, Drag Guard, Hot Switch, dev tools | Menambah/mengubah perilaku |
| `CLOUD-GOOGLE-DRIVE.md` | OAuth, scope, API, UX upload, memori | Mengerjakan cloud |
| `SYSTEM-INTEGRATION.md` | Tema OS, Mica, registry, shell, OLE, hotkey global, file picker | Integrasi Windows |
| `PERFORMANCE.md` | Budget, metode ukur, teknik, anti-pattern | Hot path, cache, startup |
| `SECURITY.md` | Model ancaman, aturan keselamatan data | Ops file, token, shim, registry |
| `ROADMAP.md` | Fase 0–8, deliverable, acceptance criteria, risiko | Merencanakan kerja |
| `TESTING.md` | Piramida test, fixture, matriks kompatibilitas | Menulis test / QA |
| `CODING-STANDARDS.md` | Toolchain, lint, error, unsafe, COM, git | Menulis kode |
| `PROMPTS.md` | Prompt kickoff, per fase, per fitur, pemeliharaan | Memberi tugas ke Claude Code |
| `NAMING.md` | Nama, identifier, checklist rename | Menyentuh identifier |

## Konvensi

- ID fitur `F-AREA-nn` (FEATURES.md), flag `area.nama` (DEVELOPER-MODE.md), token `grup.nama`
  (THEMING.md), keputusan `ADR-nnn` (DECISIONS.md). Pakai ID ini di commit, PR, dan komentar TODO.
- Kata kunci normatif: **WAJIB**, **JANGAN**, **SEBAIKNYA**, **BOLEH** (setara MUST, MUST NOT,
  SHOULD, MAY).
- Kode yang menyimpang dari dokumen dianggap bug di salah satunya: putuskan mana yang benar, lalu
  perbarui yang lain di PR yang sama.
- Semua angka performa diukur di build `--release`.

## Glosarium

| Istilah | Arti |
|---|---|
| Mica / Mica Alt | Material backdrop Windows 11 yang mengambil warna wallpaper, buram, hemat daya |
| Acrylic | Material blur real-time dari apa pun di belakang window |
| Token (tema) | Nama semantik untuk nilai visual, mis. `row.selected.bg` |
| Palet (`pal.*`) | Skala warna yang digenerate dari warna aksen (OKLCH) |
| Feature flag / switch | Pengaturan yang menyalakan/mematikan perilaku tertentu |
| Drag Guard | Kumpulan switch pencegah drag tidak sengaja |
| Hot Switch | Hotkey untuk membalik switch, selalu disertai OSD dan chip status |
| OSD | On-screen display: pop-up kecil yang tidak mengambil fokus, mengumumkan perubahan switch |
| Chord | Hotkey dua langkah, mis. `Ctrl+K` lalu `D` |
| Chip status | Indikator kecil di status bar yang menunjukkan kondisi drag & drop |
| Tide Slash | Elemen signature: penanda "kamu di sini" berbentuk highlight miring 12° |
| Picker Bridge | Panel pendamping di samping dialog Open/Save aplikasi lain (aman, tanpa injeksi) |
| Picker Takeover | Mengganti dialog Open/Save lewat COM proxy (eksperimental) |
| STA | Single-Threaded Apartment COM; wajib untuk OLE DnD dan context menu shell |
| OLE DnD | Drag & drop antar aplikasi Windows lewat `IDataObject` |
| PIDL | Identifier item di shell namespace Windows |
| Resident mode | Proses Switch Explorer tetap hidup tanpa window agar buka instan |
| PKCE loopback | Alur OAuth desktop: login di browser, kode dikirim balik ke `127.0.0.1` |
| Journal | Log operasi file append-only, dasar undo dan pemulihan setelah crash |
