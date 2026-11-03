# CLAUDE.md — Switch Explorer

Switch Explorer adalah file explorer untuk Windows yang ditulis 100% Rust. UI native di-render GPU
(GPUI Kit), backdrop Mica translucent, tema biru **Cerulean** yang bisa dikustom sampai level token,
dan filosofi **"every feature is a switch"**: setiap perilaku bisa dinyalakan/dimatikan, termasuk
lewat hotkey (**Hot Switch**, leader `Ctrl+K`) yang selalu memberi umpan balik di layar (OSD).
Target: lebih cepat dan lebih hemat RAM daripada File Explorer bawaan Windows.

## Konteks cepat
- **Identifier**: produk "Switch Explorer", binary `switch-explorer.exe`, prefix crate `sx-`, data di
  `%APPDATA%\SwitchExplorer\` dan `%LOCALAPPDATA%\SwitchExplorer\`. Daftar lengkap di `docs/NAMING.md`.
- **OS**: Windows 10 22H2+ dan Windows 11. Mica/Mica Alt hanya Win11 22H2+ (fallback: solid).
  Target utama `x86_64-pc-windows-msvc`; shim picker juga dibuild untuk `i686-pc-windows-msvc`.
- **Bahasa**: Rust stable, edition 2024. Dokumen berbahasa Indonesia; kode, komentar, log, dan pesan
  commit berbahasa Inggris. Semua string UI lewat i18n (`en` + `id`).
- **UI**: `gpui-kit` (sudah termasuk GPUI, gpui-base, gpui-component), versi dipin exact.
- **OS API**: crate `windows`. `unsafe`/FFI hanya di `sx-win`, `sx-shell`, `sx-integration`, `sx-ipc`,
  `sx-picker-shim`. Crate lain wajib `#![forbid(unsafe_code)]`.
- **Cloud**: Google Drive REST v3 (`reqwest` + rustls), OAuth 2.0 PKCE + loopback. Lazy-loaded.

## Peta dokumen — baca yang relevan saja, jangan semuanya sekaligus
| Kalau tugasnya menyangkut... | Baca dulu |
|---|---|
| Tujuan produk, prioritas, non-goal | `docs/PRD.md` |
| Crate, threading, alur data, IPC | `docs/ARCHITECTURE.md`, `docs/DECISIONS.md` |
| Layout, komponen, OSD, motion, copy UI | `docs/DESIGN.md` |
| Warna, token, tema, personalisasi | `docs/THEMING.md` |
| Perilaku fitur explorer & shortcut | `docs/FEATURES.md` |
| Feature flag, Drag Guard, Hot Switch, dev tools | `docs/DEVELOPER-MODE.md` |
| Google Drive | `docs/CLOUD-GOOGLE-DRIVE.md` |
| Registry, shell, OLE DnD, hotkey global, file picker | `docs/SYSTEM-INTEGRATION.md` |
| Budget waktu/RAM, profiling | `docs/PERFORMANCE.md` |
| Keselamatan data, token, shim | `docs/SECURITY.md` |
| Fase kerja & acceptance criteria | `docs/ROADMAP.md` |
| Strategi test & matriks kompatibilitas | `docs/TESTING.md` |
| Konvensi kode & git | `docs/CODING-STANDARDS.md` |
| Inspirasi UI (FariSYNC) | `docs/REFERENCE-ANALYSIS.md` |
| Prompt siap pakai | `docs/PROMPTS.md` |

## Perintah
```powershell
cargo run -p switch-explorer                    # dev
cargo run -p switch-explorer --release          # semua pengukuran performa WAJIB di release
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo test --workspace
cargo xtask fixtures --count 100000             # folder uji besar di %TEMP%\sx-fixtures
cargo xtask shim                                # build sx-picker-shim untuk x64 + x86
cargo xtask cleanup-registry                    # hapus SEMUA registrasi integrasi; aman kapan saja
cargo run -p switch-explorer -- --safe-mode     # tema default; integrasi & fitur eksperimental mati
```

## Struktur repo
```
apps/switch-explorer/   binary: startup, argumen CLI, single instance, wiring window
crates/sx-core/         tipe domain, Action, Event, CommandRegistry, error (tanpa dependensi OS)
crates/sx-settings/     schema settings, registry flag + Hot Switch, preset, keymap, persistensi TOML
crates/sx-theme/        token, parser nilai, warna OKLCH, palet, resolver, validator kontras
crates/sx-win/          wrapper Win32 aman untuk FS: enumerasi, CopyFile2, volume, path panjang
crates/sx-fs/           model direktori, sort natural, filter, watcher
crates/sx-ops/          engine operasi file, antrean, konflik, journal undo
crates/sx-shell/        COM shell: ikon, thumbnail, context menu, OLE DnD, clipboard, DWM, tema OS
crates/sx-integration/  registry (backup), default folder, Win+E, hotkey global, Picker Bridge
crates/sx-ipc/          protokol + named pipe (single instance, picker)
crates/sx-picker-shim/  cdylib COM proxy FileOpen/SaveDialog (EKSPERIMENTAL)
crates/sx-cloud/        trait CloudProvider, Google Drive, antrean transfer, kredensial
crates/sx-search/       pencarian rekursif, fuzzy, adapter Everything
crates/sx-tools/        Organizer, Duplicate Finder, Bulk Rename, Space Analyzer
crates/sx-ui/           view & komponen GPUI Kit, bridge tema, OSD, Token Inspector
locales/                en.yml, id.yml
themes/                 tema bawaan (*.toml), sumber kebenaran nilai default
xtask/                  otomasi build, fixtures, registry cleanup, packaging
```

## Aturan wajib
1. **UI thread tidak boleh blok.** I/O, enumerasi, hashing, network, dan panggilan shell yang lambat
   dijalankan di background executor atau worker STA. Fungsi render harus murah dan murni.
2. **Tidak ada nilai visual hardcode.** Warna, radius, spacing, durasi animasi, shadow → token dari
   `sx-theme`. Token baru: pakai skill `/add-theme-token`.
3. **Perilaku baru yang bisa mengubah kebiasaan user = feature flag** di registry `sx-settings`
   (skill `/add-feature-flag`). Default mengikuti tabel di `docs/DEVELOPER-MODE.md`.
4. **Switch selalu terlihat.** Flag bool/enum yang ditandai `hotswitchable` otomatis bisa diberi
   hotkey. Setiap perubahan lewat Hot Switch memperbarui chip status dan memunculkan OSD (sesuai
   `hotswitch.osd`). Tidak boleh ada perubahan switch yang diam-diam.
5. **Operasi file aman**: delete default ke Recycle Bin, semua operasi masuk journal dan bisa di-undo,
   tidak pernah overwrite tanpa keputusan user, cancel selalu dihormati.
6. **Modul opsional = lazy dan nol biaya saat mati** (cloud, tools, thumbnail, watcher, search, hot
   reload). Mematikan flag harus membebaskan memorinya.
7. **Integrasi sistem reversible**: setiap tulis registry lewat `sx_integration::registry` (dengan
   backup), punya fungsi unregister, dan tercakup `cargo xtask cleanup-registry`. Hanya HKCU, tanpa admin.
8. **Jangan menebak API GPUI Kit.** Cek versi terpin: https://gpui-kit.com/llms-full.txt,
   https://docs.rs/gpui-kit, atau source di `~/.cargo/registry/src/*/gpui-kit-*`. Compile sering.
9. **Dokumen adalah sumber kebenaran.** Perilaku berubah → perbarui dokumen terkait di PR yang sama.

## Larangan
- `unwrap()`/`expect()` di jalur produksi (kecuali invariant yang dijelaskan). Panic tidak boleh
  melewati batas FFI: setiap entry point COM/callback dibungkus `catch_unwind`.
- Hapus permanen tanpa konfirmasi eksplisit user.
- Menyimpan token/secret di file, log, settings, atau crash report. Refresh token hanya di Windows
  Credential Manager; access token hanya di memori.
- Menulis HKLM, meminta elevasi untuk fitur inti, atau inject DLL lewat hook global.
- Menambah dependency berat tanpa catatan di `docs/DECISIONS.md` (cek dampak dengan `cargo bloat`).
- Telemetri/analytics apa pun yang mengirim data keluar.

## Pola kode singkat
- Error: `thiserror` di library crate, `anyhow` hanya di `apps/switch-explorer` dan `xtask`.
- Log: `tracing` dengan field terstruktur. Jangan pernah log token; path user hanya di level `debug`.
- Threading: UI thread = COM STA. Thumbnail/ikon/data context menu = worker STA khusus dengan message
  pump. Network = runtime tokio lazy milik `sx-cloud` saja.
- State UI: `Entity<Model>` untuk state yang bertahan, `RenderOnce` untuk potongan reusable. Komponen
  signature (nav sidebar, tab, gauge drive, breadcrumb, OSD) dibangun di atas gpui-base; komponen
  kompleks (DataTable, VirtualList, Dock, Input) dari gpui-component, warnanya dari bridge tema.
- Semua aksi terdaftar di `CommandRegistry` (id stabil, label i18n, binding default). Context menu,
  palette, keymap editor, dan Hot Switch membaca registry yang sama.
- Backdrop: `WindowBackgroundAppearance::{MicaAltBackdrop, MicaBackdrop, Blurred, Opaque}`.
- Flag dibaca dari snapshot murah (`cx.features().dnd_source_folders`), bukan dari file per frame.

## Alur kerja per tugas
1. Baca dokumen relevan → tulis rencana singkat: file yang disentuh, risiko, cara verifikasi.
   Untuk tugas besar, minta persetujuan rencana dulu.
2. Implementasi dalam langkah kecil; `cargo check` setelah tiap langkah.
3. Tambah/ubah test (unit, integrasi, atau UI test `#[gpui_kit::test]`).
4. Jalankan fmt + clippy + test. Perubahan hot path → ukur di release.
5. Perbarui dokumen dan `CHANGELOG.md` bila perilaku berubah.
6. Tutup dengan ringkasan: apa yang selesai, apa yang belum, langkah verifikasi manual untuk saya.

## Definition of Done
- Build bersih tanpa warning clippy; test hijau; tidak ada `todo!()` di jalur yang bisa dijangkau.
- Mematuhi budget `docs/PERFORMANCE.md` untuk area yang disentuh.
- Light & dark benar; fokus keyboard terlihat; string ada di `en` dan `id`.
- Flag/token/command baru terdaftar & terdokumentasi; integrasi baru tercakup cleanup registry.

## Skill, agent, dan rules proyek
- Skill: `/start-phase <n>`, `/add-feature-flag`, `/add-theme-token`, `/perf-audit`.
- Agent: `windows-integration-reviewer` (wajib setelah menyentuh Win32/COM/registry/OLE/hotkey),
  `ux-theme-reviewer` (setelah menyentuh UI atau tema).
- Aturan per path dimuat otomatis dari `.claude/rules/`.
- Skill resmi GPUI Kit (opsional, sangat disarankan): lihat https://gpui-kit.com/skills.

## Status
- Fase aktif: **2 — Paritas Explorer + Drag Guard + Hot Switch** (lihat `docs/ROADMAP.md`). Perbarui baris ini saat
  pindah fase. Fase 1 selesai (`docs/benchmarks/phase1.md`).
- Fase 0 selesai dengan keputusan GO (`docs/benchmarks/phase0.md`). Verifikasi manual IME Jepang dan
  Windows 10 masih terbuka (ADR-020). Pola wajib dari spike: ADR-015 sampai ADR-018.
