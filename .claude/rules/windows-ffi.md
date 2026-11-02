---
paths:
  - "crates/sx-win/**"
  - "crates/sx-shell/**"
  - "crates/sx-integration/**"
  - "crates/sx-ipc/**"
  - "crates/sx-picker-shim/**"
---
# Aturan Win32, COM, dan registry

- Setiap blok `unsafe` wajib komentar `// SAFETY:` yang menjelaskan invariant.
- Handle dibungkus RAII; tidak ada handle mentah di API publik crate.
- COM diinisialisasi per thread lewat guard; objek STA hanya dipakai di thread pembuatnya. UI thread = STA.
- Semua callback dari Windows (window proc, hook, progress routine, method COM) dibungkus `catch_unwind`.
  Panic tidak boleh melewati FFI.
- `HRESULT` dikonversi lewat helper bersama dengan konteks; jangan abaikan hasil.
- Registry: hanya HKCU, hanya lewat `sx_integration::registry` (backup otomatis), setiap fitur punya
  unregister dan tercakup `cargo xtask cleanup-registry`.
- Hotkey global: lepaskan dengan `UnregisterHotKey` saat binding dihapus; laporkan kegagalan pendaftaran.
- `sx-picker-shim`: tanpa GPUI/tokio, dependensi minimal, timeout di setiap tunggu, pompa pesan saat menunggu,
  fallback ke dialog asli di setiap error. Jangan pernah memanggil `CoCreateInstance` untuk CLSID dialog
  dari dalam shim (rekursi) — pakai `DllGetClassObject` milik `comdlg32.dll` System32.
- Rujukan: `docs/SYSTEM-INTEGRATION.md`, `docs/SECURITY.md`.
- Setelah selesai, minta review agent `windows-integration-reviewer`.
