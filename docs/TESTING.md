# Strategi Test

## 1. Piramida

| Lapisan | Alat | Cakupan |
|---|---|---|
| Unit & property | `cargo test`, `proptest`, `insta` | Logika murni: sort natural, filter, parser tema, palet, kontras, flag & Location Rules, state machine Drag Guard, keymap |
| Integrasi | `tempfile`, `assert_fs` | `sx-ops` di folder sementara, `sx-fs` enumerasi & watcher, IPC, kredensial (mock) |
| UI | `#[gpui_kit::test]`, `TestWindowExt` | Render komponen, input keyboard/pointer, fokus, aksesibilitas, OSD |
| Kontrak cloud | `wiremock` | Semua endpoint & jalur error Drive |
| Manual/QA | Checklist di bawah | Integrasi Windows, DnD antar aplikasi, picker, visual |
| Performa | `criterion`, `--bench-open`, Perf HUD | Budget di PERFORMANCE.md |

## 2. Unit & property (wajib)

- Natural sort: paritas dengan `StrCmpLogicalW` pada korpus (angka, Unicode, spasi, ekstensi).
- Validasi nama file: karakter terlarang, nama reserved, panjang.
- Parser nilai tema: property test (tidak panic untuk input acak), siklus terdeteksi, error berisi posisi.
- Kontras: semua tema bawaan lulus pasangan di THEMING.md §11; snapshot `ResolvedTheme` Cerulean.
- Adaptasi otomatis light↔dark menghasilkan nilai valid & lulus kontras pada kasus umum.
- Flag: default, parse/serialize, key tak dikenal dipertahankan, migrasi schema, Location Rules (yang paling
  spesifik menang).
- Drag Guard: tabel kasus untuk setiap kombinasi switch, cooldown scroll (dengan `Clock` palsu), hold delay,
  ambang, modifier, seleksi campuran, bypass tombol kanan.
- Hot Switch: toggle/cycle/grup/preset/pilihan (simpan & kembalikan nilai), persisten vs sesi, dependensi
  master.
- Keymap: parse `keymap.toml`, unbind, deteksi konflik, aturan AltGr.

## 3. Integrasi

- `sx-ops`: copy/move/delete/rename pada fixture (Unicode, path > 260, read-only, file terkunci, file 0 byte,
  file besar), konflik dengan setiap kebijakan, cancel di tengah, pause/resume, undo/redo, pemulihan journal
  setelah "crash" yang disimulasikan (proses dihentikan di tengah operasi).
- `sx-fs`: enumerasi 100k (jumlah & atribut benar), watcher (tambah/hapus/rename/overflow → rescan).
- IPC: single instance, pesan berukuran maksimum, versi tidak cocok, klien palsu ditolak.

## 4. UI

- Navigasi keyboard di daftar, breadcrumb, sidebar; fokus terlihat.
- Hot Switch: kirim `ctrl-k d` → flag berubah → elemen OSD muncul dengan teks benar → chip status berubah;
  chord diabaikan saat fokus di input teks; toggle beruntun memperbarui OSD yang sama.
- Theme switch: semua window ter-refresh; tidak ada warna hardcode (lint: cari literal warna di `sx-ui`).
- Peran & nama AccessKit pada kontrol utama.

## 5. Matriks kompatibilitas picker

Isi ✔ / ✖ / catatan per versi Windows (Win10 22H2, Win11 23H2, Win11 24H2+).

| Aplikasi | Jenis dialog | Bridge | Takeover | Catatan |
|---|---|---|---|---|
| Chrome / Edge (upload) | IFileOpenDialog (mungkin di proses utilitas) | | | |
| Firefox | IFileOpenDialog | | | |
| WhatsApp Desktop (Store) | periksa saat uji | | | Aplikasi Store; perilaku bisa berbeda |
| Telegram Desktop | dialog native via Qt | | | |
| Discord / VS Code (Electron) | IFileOpenDialog | | | |
| Office "Save As" | dialog kustom + IFileDialogCustomize | | | Diharapkan fallback |
| Notepad (dropdown encoding) | IFileDialogCustomize | | | Diharapkan fallback |
| Aplikasi 32-bit (mis. 7-Zip x86) | IFileOpenDialog via WOW64 | | | Butuh shim x86 |
| Aplikasi elevated | — | ✖ (UIPI) | diharapkan native | Verifikasi |
| Aplikasi UWP/WinUI (FileOpenPicker) | broker | ✖ | ✖ | Di luar cakupan |
| Java Swing (JFileChooser) | dialog sendiri | ✖ | ✖ | Di luar cakupan |

Uji wajib Takeover: buka/batal/pilih banyak/simpan dengan timpa/simpan dengan ekstensi default/kill switch/
DLL hilang (pastikan pesan & pemulihan jelas)/aplikasi host ditutup saat picker terbuka.

## 6. Matriks drag & drop

| Arah | Explorer | Desktop | Chrome (area upload) | WhatsApp | Discord | VS Code | Outlook (lampiran) |
|---|---|---|---|---|---|---|---|
| Switch Explorer → app | | | | | | | — |
| App → Switch Explorer | | | — | | | | (virtual files) |

Untuk setiap sel: salin, pindah, shortcut (Alt), drag kanan + menu, efek kursor benar, drag image tampil,
undo setelah drop. Ulangi dengan preset "Anti salah drag" dan dengan masing-masing switch sumber dimatikan
(hint muncul, tidak ada item yang diam-diam tertinggal).

## 7. Hot Switch & keyboard (manual)

- Semua binding default di FEATURES.md bekerja; OSD muncul ≤ 50 ms dan tidak mengambil fokus.
- Hotkey global saat app di background dan saat resident tanpa window; OSD muncul di monitor aktif.
- Layout keyboard dengan AltGr (Jerman, Polandia): mengetik karakter AltGr di kotak cari/rename tidak memicu
  binding `Ctrl+Alt+…`.
- Screen reader (Narrator) mengumumkan perubahan switch.

## 8. QA visual & tema

Untuk setiap komponen di DESIGN.md §11: light, dark, high contrast, transparansi Windows mati, Windows 10
(solid), DPI 100/125/150/200%, text scale 100/150%, window aktif vs tidak aktif (Mica), reduce motion.
Ambil screenshot pembanding untuk Home, folder, Settings, Theme Studio, OSD, popup Drive.

## 9. Performa

- `criterion`: sort/filter 100k, parser & resolver tema, generator palet.
- `--bench-open` pada fixture 1k/10k/100k → median 5 kali dibandingkan budget.
- Profil memori dengan memory meter setelah: start, buka 100k, scroll penuh dengan thumbnail, minimize 60 s,
  matikan cloud.

## 10. Checklist rilis

- [ ] Semua test hijau; clippy bersih; `cargo-deny` & `cargo-audit` bersih.
- [ ] Budget performa terpenuhi (laporan di `docs/benchmarks/`).
- [ ] Matriks §5–§7 terisi untuk Win10 & Win11; regresi = blocker.
- [ ] Install → aktifkan semua integrasi → uninstall → registry bersih (bandingkan ekspor HKCU sebelum/sesudah).
- [ ] Binary & DLL ditandatangani; checksum dipublikasikan.
