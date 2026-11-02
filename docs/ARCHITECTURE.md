# Arsitektur — Switch Explorer

Dokumen ini menjelaskan bentuk sistem: proses, crate, thread, state, alur data, persistensi, dan IPC.
Alasan di balik pilihan teknologi ada di `DECISIONS.md`.

## 1. Gambaran besar

```text
┌──────────────────── switch-explorer.exe (single instance) ───────────────────────┐
│ UI thread (COM STA)                                                              │
│   GPUI Kit windows ── sx-ui views ── state (Entity) ◀── hasil/event ───────┐     │
│        │ Action (klik, keymap, Hot Switch, palette)                        │     │
│        ▼                                                                   │     │
│ Background executor: enumerasi, sort, filter, hash, search, tools ─────────┤     │
│ Worker STA ×2: thumbnail, ikon, data context menu, property store ─────────┤     │
│ Worker ops (CopyFile2) + STA ops-shell (IFileOperation, Recycle Bin) ──────┤     │
│ Watcher: ReadDirectoryChangesW, hanya folder yang sedang terbuka ──────────┤     │
│ HotkeyService: RegisterHotKey untuk Hot Switch global (opsional) ──────────┤     │
│ [lazy] sx-cloud: runtime tokio 2 thread, reqwest, OAuth loopback ──────────┘     │
│ IPC server: \\.\pipe\switch-explorer.<SID>.v1                                    │
└───────────────▲──────────────────────────────────▲───────────────────────────────┘
                │ OpenPaths                          │ PickRequest / PickResult
   ┌────────────┴────────────┐        ┌──────────────┴──────────────────────────┐
   │ switch-explorer.exe     │        │ Proses lain (Chrome, WhatsApp, Office)  │
   │ kedua: dari registry    │        │ memuat sx_picker_shim_*.dll             │
   │ verb/Win+E → teruskan   │        │ (Picker Takeover, eksperimental)        │
   └─────────────────────────┘        └─────────────────────────────────────────┘
 Picker Bridge: SetWinEventHook (out-of-context) + UI Automation dari proses utama.
 Browser default: hanya saat connect Google Drive (OAuth PKCE → http://127.0.0.1:<port>).
```

## 2. Prinsip arsitektur

1. **Satu proses, banyak window.** Single instance; proses kedua hanya meneruskan argumen lalu keluar.
2. **UI thread suci.** Hanya input, layout, dan render. Tidak ada pekerjaan > 4 ms di UI thread.
3. **Lazy everything.** Modul opsional dibuat saat pertama dipakai dan dibongkar saat dimatikan/idle.
4. **Alur satu arah.** Action → controller/model → task → hasil → model → render.
5. **Platform di pinggir.** Logika domain (sort, filter, tema, flag, rencana operasi) murni Rust dan bisa
   dites tanpa Windows; Win32/COM hanya di crate platform.
6. **Reversible.** Perubahan sistem tercatat dan bisa dibatalkan; operasi file tercatat di journal.

## 3. Workspace & crate

| Crate | Tanggung jawab | Bergantung ke |
|---|---|---|
| `apps/switch-explorer` | `main`, argumen CLI, single instance, wiring service, membuka window | semua `sx-*` |
| `sx-core` | Tipe domain (`Location`, `EntryKind`, `Selection`), `Action`, `Event`, `CommandRegistry`, error | — |
| `sx-settings` | Schema settings, `define_flags!`, metadata Hot Switch, preset, Location Rules, keymap, TOML, migrasi | `sx-core` |
| `sx-theme` | `define_tokens!`, parser nilai, OKLab/OKLCH, generator palet, resolver, validator kontras, hot reload | `sx-core` |
| `sx-win` | Wrapper Win32 aman: enumerasi cepat, `CopyFile2` + progress, `MoveFileExW`, info volume, path panjang | `sx-core` |
| `sx-fs` | `DirSnapshot`, kunci sort natural, filter, watcher (debounce) | `sx-core`, `sx-win` |
| `sx-ops` | Rencana & eksekusi operasi, antrean, konflik, journal, undo/redo | `sx-core`, `sx-win`, `sx-shell` |
| `sx-shell` | Ikon, thumbnail, context menu native, OLE DnD, clipboard, properties, preview handler, DWM, watcher tema OS | `sx-core` |
| `sx-integration` | Registry (backup/restore), default folder, Win+E, explorer redirect, entri menu Explorer, Send To, `HotkeyService`, Picker Bridge | `sx-core`, `sx-shell`, `sx-ipc` |
| `sx-ipc` | Tipe pesan, framing, named pipe server/client, verifikasi identitas | `sx-core` (tipe saja) |
| `sx-picker-shim` | cdylib COM proxy `FileOpenDialog`/`FileSaveDialog` | `sx-ipc` (tanpa GPUI/tokio) |
| `sx-cloud` | `CloudProvider`, Google Drive, OAuth, antrean transfer, kredensial | `sx-core`, `sx-settings`, `sx-ops` |
| `sx-search` | Pencarian rekursif, operator query, fuzzy (nucleo), adapter Everything | `sx-core`, `sx-fs` |
| `sx-tools` | Organizer, Duplicate Finder, Bulk Rename, Space Analyzer (masing-masing di balik flag) | `sx-core`, `sx-fs`, `sx-ops` |
| `sx-ui` | Workspace, view, komponen, bridge tema, OSD, Theme Studio, Token Inspector | semua di atas kecuali shim |
| `xtask` | Fixtures, build shim, cleanup registry, packaging | — |

Aturan dependensi:
- `sx-core` paling bawah dan tidak mengimpor `windows`.
- Crate non-platform memakai `#![forbid(unsafe_code)]`.
- Crate platform (`sx-win`, `sx-shell`, `sx-integration`, `sx-ipc`, `sx-picker-shim`) boleh `unsafe`
  dengan aturan di `CODING-STANDARDS.md`.
- `sx-ui` tidak memanggil Win32 langsung; selalu lewat service dari `sx-shell`/`sx-integration`.
- `sx-picker-shim` hanya bergantung pada `sx-ipc`, `windows`, dan `serde` — ia dimuat di proses aplikasi
  lain, jadi harus kecil dan tanpa runtime berat.

## 4. Model proses

| Proses | Kapan | Isi |
|---|---|---|
| `switch-explorer.exe` (utama) | Saat app dipakai; tetap hidup jika resident mode | Semua window, worker, IPC server, HotkeyService |
| `switch-explorer.exe` (penerus) | Dipanggil registry verb/Win+E/CLI saat instance utama hidup | Kirim `OpenPaths` lewat pipe, keluar < 50 ms |
| `sx_picker_shim_*.dll` di proses lain | Hanya jika Picker Takeover aktif | Proxy dialog; meminta UI picker ke proses utama |
| Browser default | Saat connect Google Drive | Login Google, redirect ke `127.0.0.1` |
| `prevhost.exe` (milik Windows) | Preview handler sistem | Merender preview Office/PDF di child HWND kita |

## 5. Model thread

| Thread | Jumlah | COM | Tugas | Aturan |
|---|---|---|---|---|
| UI (main) | 1 | STA | Loop GPUI, input, layout, render; `SHDoDragDrop`/`IDropTarget`; menu native | Tidak ada kerja > 4 ms; COM blocking hanya yang wajib di UI STA (DnD, menu) |
| Background executor (GPUI) | sesuai GPUI | — | Enumerasi, sort, filter, parsing tema, hashing ringan, search | Task bisa dibatalkan; hasil dikirim lewat update entity |
| Worker STA shell | `perf.shell_workers` (2) | STA + message pump | `IShellItemImageFactory`, `SHGetFileInfo`, property store, data context menu | Antrean prioritas (item terlihat dulu), cancel token per permintaan |
| Worker ops | maks `ops.parallel_jobs` (2) | — | `CopyFile2`, `MoveFileExW`, hashing verifikasi | Progress di-throttle 10 Hz |
| STA ops-shell | 1 | STA | `IFileOperation` (Recycle Bin, namespace shell, elevasi UAC) | Terpisah dari worker thumbnail |
| Watcher | 1 | — | `ReadDirectoryChangesW` folder yang terbuka | Debounce 100 ms; buffer overflow → rescan |
| HotkeyService | 1 (message-only window) | — | `RegisterHotKey`, `WM_HOTKEY` | Hanya jika ada binding scope global |
| Tokio (lazy) | 2 | — | HTTP Drive, server loopback OAuth | Dibuat saat cloud pertama dipakai; dimatikan setelah idle |
| Picker Bridge hook | 1 | STA | `SetWinEventHook` + UI Automation | Hanya jika `integration.picker_bridge` aktif |
| IPC server | 1 | — | Accept & baca pipe | Pesan maks 1 MB |

Aturan COM: setiap thread memanggil `CoInitializeEx` sekali lewat guard RAII; objek STA hanya dipakai di
thread pembuatnya; data lintas thread dikirim sebagai data biasa (path, PIDL terserialisasi), bukan
pointer COM.

## 6. State & kepemilikan

```text
App (global)
├─ Global<SettingsStore>   snapshot settings+flag (Arc), keymap, preset, lapisan sesi
├─ Global<ThemeStore>      ResolvedTheme aktif + sumber (mode, aksen, override)
├─ Global<Services>        handle sx-shell, sx-ops, sx-search; sx-cloud & sx-tools lazy
├─ Entity<OpsQueue>        operasi berjalan, progress, journal, undo stack
├─ Entity<DriveState>*     akun, kuota, transfer (*dibuat saat cloud pertama dipakai)
└─ Window (per window)
   └─ Entity<Workspace>    tab, split, sidebar, pane, host OSD & toast
      └─ Entity<TabState>  Location, history, view settings, seleksi, posisi scroll
         └─ Entity<FolderModel>  DirSnapshot, status loading, langganan watcher
```

| Event | Pengirim | Penerima utama |
|---|---|---|
| `SettingsChanged { ids }` | SettingsStore | Semua pemakai flag terkait, persistensi |
| `HotSwitchFired { id, value, source }` | SettingsStore | Host OSD, chip status, badge title bar |
| `ThemeChanged` | ThemeStore | Semua window (refresh), bridge gpui-component, backdrop |
| `SystemAppearanceChanged` | ThemeWatcher (sx-shell) | ThemeStore |
| `FolderChanged { tab, delta }` | FolderModel (watcher) | View daftar |
| `OpProgress` / `OpFinished` / `OpFailed` | OpsQueue | Toast, panel Transfer, taskbar, Aktivitas |
| `DriveEvent` | DriveState | Sidebar, Home, panel Transfer |

## 7. Command registry & keymap

Semua aksi (navigasi, operasi file, toggle switch, buka panel) terdaftar sekali di `CommandRegistry`:
id stabil (`file.copy`, `switch.toggle`), label i18n, ikon, konteks (`Workspace`, `List`, `Input`,
`Dialog`), binding default, dan apakah tampil di palette/context menu/toolbar. Context menu, command
palette, toolbar, keymap editor, dan Hot Switch membaca registry yang sama, sehingga label dan shortcut
selalu konsisten.

Keymap efektif = default bawaan ⊕ `keymap.toml` user. Binding dipasang ke GPUI sebagai `KeyBinding`
(termasuk multi-stroke untuk chord `ctrl-k d`). Binding ber-scope global diteruskan ke `HotkeyService`
(`RegisterHotKey`) dan hanya boleh berupa kombinasi tunggal.

## 8. Alur data utama

### 8.1 Membuka folder
1. Enter/double-click → `Action::Navigate(path)` → `TabState` mendorong history, status `Loading`.
2. Background task: `sx_fs::enumerate(path)` mengalirkan batch 512 entri → `FolderModel.append(batch)`;
   `cx.notify()` di-throttle per frame.
3. Batch pertama langsung dirender (daftar tervirtualisasi) → ikon diminta hanya untuk baris terlihat
   → worker STA → cache ikon → notify.
4. Enumerasi selesai → sort final (bila perlu) → langganan watcher dimulai.

### 8.2 Paste (copy)
1. `Action::Paste` → baca clipboard (`CF_HDROP` + `Preferred DropEffect`) → `OpPlan`.
2. Cek konflik cepat (stat) → bila ada, sheet konflik → keputusan user.
3. `OpsQueue.enqueue` → worker menjalankan `CopyFile2`; progress → toast/panel Transfer + taskbar.
4. Journal ditulis sebelum & sesudah tiap item (write-ahead) → selesai → entri undo terdaftar.

### 8.3 Hot Switch
1. `ctrl-k d` cocok dengan keymap GPUI di konteks `Workspace` → action
   `switch::Toggle { id: "dnd.source.folders" }`. Binding global: `WM_HOTKEY` di `HotkeyService` →
   channel → action yang sama di UI thread.
2. `SettingsStore::toggle(id, Source::HotSwitch)`: validasi (flag `hotswitchable`, dependensi) → snapshot
   baru (Arc) → emit `SettingsChanged` + `HotSwitchFired`.
3. `DragController` memakai snapshot baru untuk drag berikutnya (drag yang sedang berjalan tidak diputus).
   Host OSD menampilkan "Drag folder dinonaktifkan"; chip status dan badge di-render ulang.
4. `hotswitch.persist = true` → tulis `settings.toml` (debounce 500 ms, atomic). Jika false → nilai
   disimpan di lapisan sesi dan hilang saat restart.
5. Budget: tombol ditekan → OSD tampil ≤ 50 ms.

### 8.4 Windows berganti ke dark
1. ThemeWatcher (thread di sx-shell) menunggu `RegNotifyChangeKeyValue` pada
   `HKCU\...\Themes\Personalize` (baca saja) → baca `AppsUseLightTheme` → kirim
   `SystemAppearanceChanged { dark: true }` ke UI thread.
   - `WM_SETTINGCHANGE "ImmersiveColorSet"` hanya cadangan: di Fase 0, broadcast itu terukur tiba
     ~600 ms setelah registry berubah, sedangkan pemicu registry → present 7–17 ms.
2. ThemeStore me-resolve ulang (< 1 ms) → bridge memperbarui tema gpui-component → semua window refresh.
   - `DWMWA_USE_IMMERSIVE_DARK_MODE` diset sendiri di tiap HWND, karena GPUI baru mengubahnya saat
     callback appearance-nya datang.
   - Token latar chrome dibuat transparan lagi agar Mica tetap terlihat.

### 8.5 Upload ke Google Drive
1. Context menu "Upload ke Drive" → bila cloud belum aktif: buat runtime tokio, muat refresh token dari
   Credential Manager.
2. Popup tujuan → daftar folder on-demand → pilih/buat folder → `TransferPlan` → antrean transfer.
3. Per file: sesi resumable → chunk dialirkan dari disk sambil menghitung MD5 → verifikasi → (mode
   pindah) `IFileOperation` recycle → journal.

### 8.6 Permintaan picker (Takeover)
1. Aplikasi host memanggil `Show()` → shim konek pipe → `PickRequest`.
2. Proses utama membuka window mode picker di dekat owner → user memilih → `PickResult`.
3. Shim mengembalikan `IShellItem(s)` → aplikasi host lanjut. Gagal di titik mana pun → dialog native.

## 9. Model direktori

```rust
pub struct EntryRec {      // ~48 byte
    name: NameRef,         // offset u32 + panjang u16 ke arena UTF-8
    ext: ExtId,            // u16 indeks ke tabel ekstensi (lowercase)
    attrs: u32,            // FILE_ATTRIBUTE_*
    size: u64,
    modified: i64,         // FILETIME
    created: i64,
    kind: EntryKind,       // u8: File, Dir, Symlink, Junction, ...
    icon: IconSlot,        // u32: indeks cache atau UNRESOLVED
}

pub struct DirSnapshot {
    entries: Vec<EntryRec>,
    names: String,                 // arena nama
    names_lower: String,           // arena lowercase untuk filter cepat
    order: Vec<u32>,               // indeks hasil sort
    visible: Vec<u32>,             // indeks setelah filter
    sort_keys: Option<Vec<SortKey>>,
}
```

- Estimasi 100k entri: entri ≈ 4.8 MB + nama ≈ 2.5 MB + arena lowercase ≈ 2.5 MB + indeks ≈ 0.8 MB → ~11 MB.
- Enumerasi: `GetFileInformationByHandleEx(FileFullDirectoryInfo)` dengan buffer 64 KB, atau
  `FindFirstFileExW(FindExInfoBasic, FIND_FIRST_EX_LARGE_FETCH)`. Atribut, ukuran, dan waktu didapat
  sekaligus tanpa stat per file.
- Sort: kunci natural dihitung sekali (paritas `StrCmpLogicalW` diuji dengan korpus); folders-first;
  stabil; berjalan di background.
- Filter: substring di arena lowercase (`memchr::memmem`); fuzzy hanya untuk palette (nucleo).
- Watcher: delta (tambah/hapus/rename/ubah) diterapkan inkremental; overflow buffer → rescan penuh.
- String tampilan (tanggal, ukuran) dihitung malas hanya untuk baris terlihat dan di-cache per baris.

## 10. Cache

| Cache | Kunci | Budget default | Eviction | Flag |
|---|---|---|---|---|
| Ikon | indeks system image list; path untuk exe/lnk/ico/url | 2048 entri | LRU | `perf.icon_cache_entries` |
| Thumbnail | path + mtime + bucket ukuran | 64 MB | LRU berbasis byte; dibuang saat minimize > 60 s | `perf.thumbnail_cache_mb` |
| Snapshot folder | path | 8 folder terakhir | LRU; invalid via watcher/mtime | `perf.dir_cache_count` |
| Metadata Drive | folderId | 2000 item | LRU + TTL 5 menit | — |
| Keymap & resolved theme | — | kecil | dihitung ulang saat berubah | — |

## 11. Persistensi

| File | Lokasi | Format | Isi |
|---|---|---|---|
| `settings.toml` | `%APPDATA%\SwitchExplorer\` | TOML | Preferensi & flag (hanya yang beda dari default), Location Rules, preset tersimpan |
| `keymap.toml` | idem | TOML | Binding user (hanya yang beda dari default) |
| `theme-overrides.toml` | idem | TOML | Patch token user |
| `themes\*.toml` | idem `\themes\` | TOML | Tema user/impor |
| `pins.toml` | idem | TOML | Pinned sidebar, tujuan Drive favorit |
| `session.json` | `%LOCALAPPDATA%\SwitchExplorer\` | JSON | Window, tab, split, ukuran pane |
| `views.json` | idem | JSON | Mode tampilan per folder (LRU 2000) |
| `journal\*.jsonl` | idem `\journal\` | JSONL | Operasi file; rotasi 10 MB, simpan 30 hari |
| `integration-backup.json` | idem | JSON | Nilai registry sebelum diubah |
| `logs\*.log` | idem `\logs\` | teks | `tracing`, rotasi harian, simpan 7 hari |
| `cloud-cache\` | idem | file | Unduhan sementara Drive (dibersihkan otomatis) |
| Kredensial | Windows Credential Manager | — | Refresh token Drive (`SwitchExplorer/gdrive/<email>`) |

- Tulis atomic: file sementara di folder yang sama → flush → `ReplaceFileW`. File rusak → pakai `.bak`
  terakhir + toast yang menjelaskan.
- Key yang tidak dikenal dipertahankan saat menulis ulang (forward compatibility); `schema` versi +
  migrasi di `sx-settings`.
- Portable mode: file `portable.flag` di samping exe → semua data di `.\data\`.

## 12. IPC

- Nama pipe: `\\.\pipe\switch-explorer.<SID-user>.v1`. Arbitrase instance tunggal memakai pipe itu
  sendiri: instance pertama membuatnya dengan `FILE_FLAG_FIRST_PIPE_INSTANCE` dan maksimum 1 instance,
  sehingga proses berikutnya gagal membuat pipe (`ACCESS_DENIED`/`PIPE_BUSY`), lalu menjadi klien dan
  meneruskan permintaannya. Tidak perlu mutex terpisah.
- Semua I/O pipe overlapped dengan timeout 2 detik per pesan. Klien yang tersambung lalu diam tidak
  memblokir server. Server menunggu klien menutup koneksi sebelum `DisconnectNamedPipe`, karena
  disconnect membuang data yang belum dibaca.
- Klien memanggil `AllowSetForegroundWindow(pid server)` agar window instance utama boleh maju ke depan.
- Framing: panjang `u32` little-endian + JSON (serde). Maksimum 1 MB per pesan.
- Pesan: `Hello { version, client }`, `OpenPaths { paths, select, new_window }`, `Activate`,
  `UploadToDrive { paths }`, `PickRequest { .. }`, `PickResult { .. }`, `Ping`/`Pong`, `Shutdown`.
- Keamanan: DACL hanya SID user saat ini (+ SYSTEM), `PIPE_REJECT_REMOTE_CLIENTS`. Shim memverifikasi
  server lewat `GetNamedPipeServerProcessId` + path image harus sama dengan lokasi instalasi. Versi tidak
  cocok → klien fallback (shim: dialog native).

## 13. Startup & shutdown

1. `main`: parse argumen → cek single instance (connect pipe ≤ 20 ms) → bila server ada: kirim pesan,
   keluar.
2. Baca `settings.toml`, `keymap.toml`, file tema (sinkron, kecil) → resolve tema (< 1 ms).
3. Buka window dengan backdrop + frame pertama (Home atau folder argumen). Target ≤ 300 ms dari start.
4. Setelah frame pertama: IPC server, watcher tema OS, enumerasi drive, restore session (hanya tab aktif
   yang dienumerasi), HotkeyService (jika ada binding global).
5. Idle: prewarm ikon ekstensi umum.
6. Lazy sesuai kebutuhan: worker thumbnail, cloud, tools, search, Picker Bridge.

Shutdown: simpan session (juga autosave debounced), flush journal, `OleFlushClipboard`, hentikan worker.
Resident mode: tutup window, buang cache besar, tetap jalankan IPC dan HotkeyService.

Crash: panic hook menulis log crash lokal (tanpa token). Saat start berikutnya, journal memeriksa operasi
yang belum selesai → tawarkan Lanjutkan/Batalkan/Lihat.

## 14. Error, log, crash

- Error domain per crate (`thiserror`), diterjemahkan ke pesan UI yang menjelaskan apa yang terjadi dan
  cara memperbaikinya (DESIGN.md §10).
- `tracing` + `tracing-appender` (rolling harian); layer redaksi untuk token dan query string sensitif.
- Level default `info`; `dev.log_level` mengubahnya saat runtime.

## 15. Abstraksi platform & test double

```rust
pub trait ShellServices: Send + Sync {
    fn icon(&self, req: IconRequest) -> IconTicket;          // async lewat worker STA
    fn thumbnail(&self, req: ThumbRequest) -> ThumbTicket;
    fn open(&self, path: &Path, verb: Verb) -> Result<()>;
    fn recycle(&self, paths: &[PathBuf]) -> Result<OpHandle>;
    fn show_native_menu(&self, window: RawWindow, items: &[PathBuf], at: ScreenPoint) -> Result<()>;
    fn properties(&self, window: RawWindow, items: &[PathBuf]) -> Result<()>;
}
```

`sx-shell` menyediakan implementasi Windows; test memakai `FakeShell`. Pola yang sama untuk
`CloudProvider`, `HotkeyBackend`, dan `Clock` (agar cooldown/timer bisa dites deterministik).

## 16. Titik ekstensi

| Mau menambah... | Caranya |
|---|---|
| Flag/switch | `define_flags!` di sx-settings (skill `/add-feature-flag`) |
| Hot Switch | Tandai flag `hotswitchable`; binding default opsional di keymap bawaan |
| Token tema | `define_tokens!` di sx-theme + nilai di `themes/*.toml` (skill `/add-theme-token`) |
| Command/aksi | Daftarkan di `CommandRegistry` (id, label, binding, konteks) |
| Provider cloud | Implement `CloudProvider`, daftarkan di enum `Provider` |
| Tool | Implement trait `Tool` (id, flag, view lazy) di sx-tools |
| Mode tampilan | Implement trait `ItemView` di sx-ui |
