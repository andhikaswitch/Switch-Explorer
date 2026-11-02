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
- Gerbang kontras mencakup lapisan di bawah latar dan 27 kelompok pasangan, termasuk indikator non-teks
  3:1 (THEMING §11).

### Changed
- ARCHITECTURE §8.4: perubahan tema dipicu notifikasi registry, bukan `WM_SETTINGCHANGE`.
- SYSTEM-INTEGRATION §3, §9, §15: koreksi berdasarkan temuan spike (Mica tanpa fallback di bawah build
  22621, latar Root opak, manifest GPUI dipaksa, pola modal host).
- PERFORMANCE: baseline Fase 0 dan pengecualian CPU idle.
- Cerulean: `fg.muted` light lebih gelap dan dark lebih terang, `selection.border` lebih pekat,
  `drop_target.border` light memakai 700, indikator light (toggle, slider, progress, tab aktif) memakai
  `accent.solid`, chip status light memakai `pal.accent.800`/`#92400E`.
