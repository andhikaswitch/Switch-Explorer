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

### Changed
- ARCHITECTURE §8.4: perubahan tema dipicu notifikasi registry, bukan `WM_SETTINGCHANGE`.
- SYSTEM-INTEGRATION §3, §9, §15: koreksi berdasarkan temuan spike (Mica tanpa fallback di bawah build
  22621, latar Root opak, manifest GPUI dipaksa, pola modal host).
- PERFORMANCE: baseline Fase 0 dan pengecualian CPU idle.
