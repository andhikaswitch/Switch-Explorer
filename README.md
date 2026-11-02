# Switch Explorer

**File explorer Windows yang ditulis 100% Rust — ringan, translucent, dan setiap fiturnya adalah switch.**

Switch Explorer dibuat untuk orang yang tiap hari hidup di file explorer dan capek dengan Explorer
bawaan yang lambat, kaku, dan gampang bikin folder "kepindah" gara-gara salah drag saat scroll.

Namanya menjelaskan isinya: *explorer yang setiap fiturnya adalah switch*. Drag & drop, file
tersembunyi, mode baca-saja, sampai modul cloud — semuanya bisa dinyalakan atau dimatikan, dan yang
paling sering dipakai bisa dibalik langsung dari keyboard.

## Kenapa Switch Explorer

- **Ringan dari ujung ke ujung.** Backend *dan* UI-nya Rust (GPUI Kit, render GPU, teks DirectWrite).
  Tidak ada WebView. Modul yang dimatikan tidak memakan RAM sama sekali.
- **Every feature is a switch.** Developer Mode berisi puluhan switch: matikan drag untuk item pinned,
  beri jeda 400 ms setelah scroll sebelum drag bisa aktif, ubah aksi drop default jadi "tanya dulu",
  kunci urutan pinned, atau jadikan satu folder read-only.
- **Hot Switch.** `Ctrl+Shift+D` menyalakan/mematikan semua drag & drop. `Ctrl+K` lalu `F` untuk drag
  file, `Ctrl+K` lalu `D` untuk drag folder. Setiap perubahan muncul sebagai pop-up kecil di layar dan
  tetap terlihat di status bar, jadi kamu selalu tahu kondisinya. Semua hotkey bisa diatur ulang.
- **Tema Cerulean.** Biru muda modern terinspirasi Persona 3 Reload: highlight lagoon dengan teks navy,
  backdrop Mica translucent, otomatis ikut light/dark Windows. Bisa diubah dari level aksen sampai warna
  hover satu tombol, plus file tema yang bisa dibagikan.
- **Google Drive tanpa sync client.** Klik kanan → *Upload/Pindahkan ke Drive* → pilih atau buat folder
  tujuan di popup. File lokal baru dibuang ke Recycle Bin setelah checksum terverifikasi.
- **Bisa jadi explorer utama.** Buka folder dan Win+E diarahkan ke Switch Explorer; *Picker Bridge*
  menempel di dialog Open/Save aplikasi lain (Chrome, WhatsApp, dll.) untuk lompat ke folder favorit.
- **Tools bawaan.** Organizer (terinspirasi FariSYNC, plus dry-run & undo), Duplicate Finder,
  Bulk Rename, Space Analyzer.

## Status

Perencanaan selesai → **Fase 0 (spike & validasi)**. Lihat [docs/ROADMAP.md](docs/ROADMAP.md).

## Kebutuhan

- Windows 10 22H2+ atau Windows 11 (efek Mica: Windows 11 22H2+)
- Rust stable terbaru via `rustup`
- Visual Studio Build Tools 2022 (workload "Desktop development with C++" + Windows SDK)
- Untuk shim picker 32-bit: `rustup target add i686-pc-windows-msvc`

## Mulai (setelah scaffolding Fase 1)

```powershell
git clone <repo> switch-explorer; cd switch-explorer
cargo run -p switch-explorer                 # mode dev
cargo run -p switch-explorer --release       # cek performa
cargo test --workspace
```

## Dokumentasi

| Dokumen | Isi |
|---|---|
| [CLAUDE.md](CLAUDE.md) | Instruksi kerja untuk Claude Code (dibaca otomatis) |
| [docs/INDEX.md](docs/INDEX.md) | Peta semua dokumen + glosarium |
| [docs/PRD.md](docs/PRD.md) | Kebutuhan produk, prioritas, non-goal |
| [docs/REFERENCE-ANALYSIS.md](docs/REFERENCE-ANALYSIS.md) | Analisis FariSYNC (UI, fitur, caption) |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Arsitektur sistem |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Keputusan teknis (ADR), termasuk kenapa bukan Tauri |
| [docs/DESIGN.md](docs/DESIGN.md) | Sistem desain Cerulean, OSD, wireframe |
| [docs/THEMING.md](docs/THEMING.md) | Mesin tema & personalisasi 3 level |
| [docs/FEATURES.md](docs/FEATURES.md) | Spesifikasi fitur & shortcut |
| [docs/DEVELOPER-MODE.md](docs/DEVELOPER-MODE.md) | Feature flag, Drag Guard, Hot Switch, dev tools |
| [docs/CLOUD-GOOGLE-DRIVE.md](docs/CLOUD-GOOGLE-DRIVE.md) | Integrasi Google Drive |
| [docs/SYSTEM-INTEGRATION.md](docs/SYSTEM-INTEGRATION.md) | Default explorer, file picker, shell, hotkey global |
| [docs/PERFORMANCE.md](docs/PERFORMANCE.md) | Budget & strategi performa |
| [docs/SECURITY.md](docs/SECURITY.md) | Keamanan & keselamatan data |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Fase kerja + acceptance criteria |
| [docs/TESTING.md](docs/TESTING.md) | Strategi test & matriks kompatibilitas |
| [docs/CODING-STANDARDS.md](docs/CODING-STANDARDS.md) | Konvensi kode & git |
| [docs/PROMPTS.md](docs/PROMPTS.md) | Prompt siap pakai untuk Claude Code |
| [docs/NAMING.md](docs/NAMING.md) | Nama, identifier, checklist rename |

## Bekerja dengan Claude Code

1. Buka folder repo, jalankan `claude`. `CLAUDE.md` dan `.claude/` otomatis terbaca.
2. Mulai dari [docs/PROMPTS.md](docs/PROMPTS.md) → bagian **Prompt kickoff**.
3. Kerjakan satu fase per sesi: `/start-phase 0`, lalu `/start-phase 1`, dan seterusnya.

## Lisensi

Belum ditentukan. Rekomendasi: MIT atau Apache-2.0 (selaras dengan GPUI dan GPUI Kit yang Apache-2.0).

## Kredit & inspirasi

- FariSYNC — inspirasi dashboard & Organizer (analisis di `docs/REFERENCE-ANALYSIS.md`).
- Persona 3 Reload — inspirasi palet biru & gaya highlight; tidak memakai aset apa pun.
- GPUI (Zed Industries) dan GPUI Kit (Longbridge), Apache-2.0.
- Fluent UI System Icons (Microsoft), MIT.
