# Konvensi Kode & Git

## 1. Toolchain

```toml
# rust-toolchain.toml — pin ke stable terbaru saat scaffolding, naikkan secara sengaja
[toolchain]
channel = "1.xx.0"
components = ["rustfmt", "clippy"]
targets = ["x86_64-pc-windows-msvc", "i686-pc-windows-msvc"]
```

- Edition 2024 untuk semua crate. Build MSVC (Visual Studio Build Tools + Windows SDK).

## 2. Workspace

```toml
# Cargo.toml (root)
[workspace]
resolver = "3"
members = ["apps/*", "crates/*", "xtask"]

[workspace.dependencies]
gpui-kit = "=0.8.1"          # pin exact versi terbaru saat scaffolding; upgrade di PR khusus
windows = { version = "…", features = [ "…" ] }   # fitur dipilih per kebutuhan, jangan "semua"

[workspace.lints.rust]
unsafe_op_in_unsafe_fn = "deny"
missing_debug_implementations = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "warn"
dbg_macro = "deny"
todo = "warn"
module_name_repetitions = "allow"
missing_errors_doc = "allow"

[profile.release]
lto = "fat"
codegen-units = 1
opt-level = 3
strip = "symbols"            # simpan PDB terpisah untuk simbolisasi crash
panic = "unwind"             # wajib: sx-picker-shim memakai catch_unwind

[profile.dev.package."*"]
opt-level = 2                # dependency (GPUI) cepat saat dev
```

```toml
# .cargo/config.toml
[alias]
xtask = "run --package xtask --"

[target.'cfg(all(windows, target_env = "msvc"))']
rustflags = ["-C", "target-feature=+crt-static"]   # tanpa VC redist; penting untuk DLL shim
```

Setiap crate menulis `[lints] workspace = true`. Crate non-platform menambahkan `#![forbid(unsafe_code)]`.

## 3. Penamaan & struktur

- Crate `sx-<area>`; modul pendek dan jelas; satu konsep per file bila file > 400 baris.
- Tipe `PascalCase`, fungsi/variabel `snake_case`, konstanta `SCREAMING_SNAKE_CASE`.
- ID flag/token/command adalah string stabil bertitik (`dnd.source.folders`, `button.primary.bg_hover`,
  `file.copy`) — jangan diganti setelah rilis; tambahkan alias bila terpaksa.
- Kode, komentar, nama commit: bahasa Inggris. String UI: hanya lewat i18n.

## 4. Error handling

- Library crate: enum error per domain dengan `thiserror`; konteks berguna (path, operasi) di variant.
- `anyhow` hanya di `apps/switch-explorer` dan `xtask`.
- Tidak ada `unwrap()`; `expect("alasan invariant")` hanya untuk invariant yang benar-benar mustahil gagal.
- Error yang sampai ke pengguna diterjemahkan ke pesan yang menyebut penyebab + cara memperbaiki
  (DESIGN.md §10).
- `HRESULT` dari Win32/COM dikonversi lewat satu helper (`hr.ok()?` dengan konteks) di crate platform.

## 5. Unsafe & FFI

- Hanya di `sx-win`, `sx-shell`, `sx-integration`, `sx-ipc`, `sx-picker-shim`.
- Setiap blok `unsafe` diberi komentar `// SAFETY:` yang menjelaskan invariant yang dijamin.
- Bungkus handle dengan RAII (`Drop` memanggil `CloseHandle`, `CoTaskMemFree`, `UnregisterHotKey`, dll.);
  jangan menyebarkan handle mentah ke luar crate platform.
- String ke Win32: `HSTRING`/`PCWSTR` dari `windows`; jangan membuat buffer UTF-16 manual tanpa terminator.
- Callback dari Windows (window proc, progress routine, event hook, COM method) dibungkus `catch_unwind`;
  tidak pernah panic melewati batas FFI.

## 6. COM

- Inisialisasi per thread lewat guard RAII (`ComApartment::sta()` / `::mta()`), sekali per thread.
- Objek STA hanya dipakai di thread pembuatnya. Lintas thread kirim data (path, PIDL terserialisasi),
  bukan pointer COM.
- UI thread adalah STA (wajib untuk OLE DnD, clipboard, context menu). Worker shell = STA dengan message pump.
- Implementasi interface COM memakai `#[implement]` dari crate `windows`.

## 7. Thread & async

- UI thread: tidak ada I/O, panggilan shell lambat, sort/filter besar, atau blocking wait.
- Helper debug `assert_not_ui_thread()` dipanggil di awal fungsi I/O berat (aktif di debug build).
- Task background harus bisa dibatalkan (cancel token) dan mengirim hasil lewat update entity.
- Tokio hanya di `sx-cloud`, lewat runtime lazy; crate lain tidak bergantung pada tokio.

## 8. GPUI / GPUI Kit

- Cek API versi terpin sebelum menulis kode (llms-full.txt, docs.rs, source di registry). Jangan menebak.
- `Entity<T>` untuk state yang bertahan; `RenderOnce` untuk potongan reusable; jangan membuat ulang state
  input di `render`.
- Fungsi `render` murni: tanpa I/O, tanpa alokasi besar; teks tampilan di-cache.
- Warna/ukuran/durasi hanya dari token tema (`theme.color(Token::RowHoverBg)` dan sejenisnya).
- Semua aksi didaftarkan di `CommandRegistry` dan dipicu lewat action GPUI agar keymap & palette konsisten.
- Langganan (`Subscription`) disimpan di struct pemilik agar tidak terlepas.

## 9. Logging

- `tracing` dengan field terstruktur (`tracing::info!(op = %id, items = n, "copy started")`).
- Level: `error` (aksi gagal), `warn` (pulih otomatis), `info` (peristiwa penting), `debug` (detail + path),
  `trace` (loop panas, hanya saat profiling).
- Token, header `Authorization`, parameter OAuth, dan URI sesi upload tidak pernah dilog (layer redaksi).

## 10. Test

- Setiap PR yang mengubah perilaku menambah/mengubah test (TESTING.md).
- Test tidak bergantung pada urutan dan tidak menyentuh folder pengguna nyata (pakai `tempfile`).
- Waktu & sistem dibungkus trait (`Clock`, `ShellServices`, `HotkeyBackend`) agar bisa dipalsukan.

## 11. Dokumentasi kode

- Item publik diberi doc comment singkat (apa & kenapa, bukan mengulang nama).
- Perubahan perilaku → perbarui dokumen di `docs/` pada PR yang sama; `CHANGELOG.md` diperbarui.

## 12. Git

- Branch: `feat/<id>-<ringkas>`, `fix/<ringkas>`, `chore/<ringkas>`, `docs/<ringkas>`.
- Commit: Conventional Commits dengan scope crate, sebut ID fitur/ADR bila ada:
  `feat(sx-settings): add hot switch toggle action (F-KEY-01)`.
- Satu PR = satu topik; kecil dan bisa direview.

**Checklist PR**
- [ ] fmt, clippy (`-D warnings`), test hijau
- [ ] Tidak ada warna/ukuran hardcode; string UI di `en` & `id`
- [ ] Flag/token/command baru terdaftar & terdokumentasi
- [ ] UI thread tidak blok; budget performa area terkait dicek di release
- [ ] Integrasi registry baru tercakup cleanup
- [ ] Dokumen & CHANGELOG diperbarui

## 13. Dependency

- Tambah dependency hanya bila jelas bermanfaat; catat alasan di PR (dan di DECISIONS.md bila besar).
- Cek ukuran dengan `cargo bloat --release --crates`; pilih fitur minimal (`default-features = false`).
- `cargo-deny` memeriksa lisensi (lihat SECURITY.md §9), advisory, dan sumber.
