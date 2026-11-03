# Nama & Identifier

## 1. Nama terpilih: Switch Explorer

*Explorer yang setiap fiturnya adalah switch.* Nama ini langsung menjelaskan dua hal: ini file explorer, dan
pembedanya adalah switch — Developer Mode, Drag Guard, dan Hot Switch yang bisa dibalik dari keyboard.

**Tagline:** "Every feature is a switch." / "Setiap fitur adalah switch."

**Catatan pencarian:** kata "Switch" sering diasosiasikan dengan Nintendo Switch. Mitigasi:
- Selalu tulis lengkap "Switch Explorer" (jangan disingkat "Switch").
- Tambahkan "for Windows" di listing toko/website; repo bernama `switch-explorer`.
- Logo khas: toggle switch yang menyatu dengan bentuk folder, palet Cerulean; hindari merah ala Nintendo.
- Sebelum rilis publik, cek ketersediaan nama di GitHub, Microsoft Store, domain, serta basis data merek
  (DJKI untuk Indonesia, serta USPTO/EUIPO bila rilis global).

Nama lain yang dipertimbangkan: Switchyard, TideSwitch, Switchboard (sudah dipakai sebagai nama aplikasi
Settings di elementary OS), BlueSwitch, Swytch.

## 2. Identifier

| Hal | Nilai |
|---|---|
| Nama produk | Switch Explorer |
| Package binary | `switch-explorer` (folder `apps/switch-explorer/`) |
| Executable | `switch-explorer.exe` |
| Prefix crate | `sx-` (`sx-core`, `sx-ui`, …) / modul Rust `sx_core`, `sx_ui` |
| Data roaming | `%APPDATA%\SwitchExplorer\` |
| Data lokal | `%LOCALAPPDATA%\SwitchExplorer\` |
| Portable | `portable.flag` → `.\data\` |
| Named pipe | `\\.\pipe\switch-explorer.<SID>.<session>[.admin].v1` |
| Mutex single instance | `Local\SwitchExplorer.SingleInstance.v1` |
| AppUserModelID | `SwitchExplorer.App` |
| Verb registry | `switchexplorer` (`Directory\shell`, `Drive\shell`), `switchexplorer.open` (menu Explorer) |
| DLL shim | `sx_picker_shim_x64.dll`, `sx_picker_shim_x86.dll` di `%LOCALAPPDATA%\SwitchExplorer\shim\` |
| Target kredensial | `SwitchExplorer/gdrive/<email>` |
| Ekstensi file tema | `.sxtheme` |
| Variabel lingkungan build | `SX_GDRIVE_CLIENT_ID`, `SX_GDRIVE_CLIENT_SECRET` |
| Folder fixture | `%TEMP%\sx-fixtures` |
| Target log | `switch_explorer` |

## 3. Checklist jika nama diganti lagi

1. Ganti string produk di `locales/*.yml`, judul window, halaman Tentang, manifest, metadata installer.
2. Ganti identifier di tabel §2 (pipe, mutex, AUMID, verb, folder data, kredensial, DLL, ekstensi tema).
3. Sediakan migrasi: salin `%APPDATA%\SwitchExplorer\` ke folder baru; jalankan unregister dengan nama lama
   sebelum register dengan nama baru (registry!).
4. Prefix crate boleh tetap (`sx-`) bila tidak ingin rename besar.
5. Cari sisa nama lama: `rg -i "switch.?explorer|switchexplorer|sx_picker"` lalu perbarui `docs/` & `CLAUDE.md`.
