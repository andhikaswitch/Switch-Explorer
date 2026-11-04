# Integrasi Sistem Windows

Semua yang menyentuh Windows di luar window kita: tema OS, backdrop, registry, shell, OLE, clipboard,
hotkey global, dan dialog Open/Save aplikasi lain.

## 1. Prinsip

1. **HKCU saja, tanpa admin.** Tidak ada tulis ke HKLM.
2. **Opt-in.** Integrasi yang mengubah perilaku sistem default mati dan diaktifkan lewat onboarding/Settings
   dengan penjelasan apa yang berubah.
3. **Reversible.** Nilai lama dibackup ke `integration-backup.json` sebelum ditimpa; setiap integrasi punya
   unregister; semua tercakup `cargo xtask cleanup-registry` dan `switch-explorer.exe --unregister all`.
4. **Bisa diperbaiki.** Settings → Integrasi menampilkan status tiap integrasi (aktif, rusak, path exe
   berubah) dengan tombol Perbaiki.
5. **Gagal dengan aman.** Jika integrasi gagal, Windows tetap memakai perilaku bawaannya.

## 2. Mengikuti tema & preferensi Windows

| Preferensi | Sumber | Event perubahan | Efek |
|---|---|---|---|
| Light/dark | `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme` | `WM_SETTINGCHANGE` dengan `"ImmersiveColorSet"` | Ganti varian tema (mode `system`) |
| Transparansi | `…\Themes\Personalize\EnableTransparency` | sama | 0 → material solid |
| Warna aksen | `UISettings.GetColorValue(UIColorType::Accent)` | `UISettings.ColorValuesChanged` | Seed bila `accent_source = windows` |
| High contrast | `SystemParametersInfoW(SPI_GETHIGHCONTRAST)` | `WM_SETTINGCHANGE` | Tema `high-contrast`, material solid |
| Animasi | `SPI_GETCLIENTAREAANIMATION` / `UISettings.AnimationsEnabled` | `WM_SETTINGCHANGE` | Reduce motion |
| Skala teks | `UISettings.TextScaleFactor` | `TextScaleFactorChanged` | `font.scale` |
| Wallpaper | `SPI_GETDESKWALLPAPER` / `IDesktopWallpaper` | `WM_SETTINGCHANGE` (`SPI_SETDESKWALLPAPER`) | Seed bila `accent_source = wallpaper` |
| Hemat baterai | `GetSystemPowerStatus().SystemStatusFlag` | notifikasi power setting | Material solid (`perf.solid_on_battery_saver`) |
| Bahasa & format | Bahasa tampilan & locale pengguna | `WM_SETTINGCHANGE` | Bahasa UI awal, format tanggal/angka |

Pemantauan dilakukan `ThemeWatcher` (window tersembunyi di `sx-shell`) yang mengirim event ke UI thread.

## 3. Window chrome & backdrop

- Backdrop lewat GPUI: `WindowBackgroundAppearance::MicaAltBackdrop` (default), `MicaBackdrop`, `Blurred`
  (acrylic), `Opaque`. GPUI memanggil `DWMWA_SYSTEMBACKDROP_TYPE` hanya di build ≥ 22621; di bawah itu
  permintaan Mica **diam-diam diabaikan** (tanpa fallback). Karena itu aplikasi sendiri yang memilih
  `Opaque` di Windows 10 dan Windows 11 21H2 (terverifikasi di spike Fase 0).
- Root gpui-component melukis `theme.background` opak di seluruh window sehingga Mica tertutup. Token
  latar chrome (background, title bar) harus transparan saat backdrop aktif, dan dipasang ulang setelah
  setiap pergantian light/dark karena `Theme::change` memuat ulang tema.
- Title bar kustom dengan tab; area kontrol window ditandai untuk hit-test sehingga **snap layout** muncul
  saat hover tombol maximize (verifikasi di Fase 0).
- Dark title bar/border mengikuti mode; sudut membulat mengikuti sistem.
- Mica hanya tampil saat window aktif; window tidak aktif solid (perilaku Windows).
- Bila properti di atas tidak tersedia lewat GPUI, gunakan `DwmSetWindowAttribute` pada HWND
  (`DWMWA_USE_IMMERSIVE_DARK_MODE`, `DWMWA_WINDOW_CORNER_PREFERENCE`, `DWMWA_SYSTEMBACKDROP_TYPE`).

## 4. Single instance, IPC, resident mode

- Proses pertama membuat pipe `\\.\pipe\switch-explorer.<SID>.<session>[.admin].v1` (detail di
  ARCHITECTURE.md §12).
- Proses berikutnya (dari registry verb, Win+E, CLI) mengirim `OpenPaths`/`Activate` lalu keluar < 50 ms.
- Resident mode (`integration.resident`): saat window terakhir ditutup, proses tetap hidup dengan cache besar
  dibuang (≤ 25 MB), ikon di system tray dengan menu (Buka, Hot Switch, Settings, Keluar). Win+E/buka folder
  menjadi instan. Ikut startup Windows lewat `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (opsional).

## 5. Menjadi explorer utama

### 5.1 Membuka folder & drive (`integration.default_folder_handler`)

```text
HKCU\Software\Classes\Directory\shell
    (Default) = "switchexplorer"
HKCU\Software\Classes\Directory\shell\switchexplorer
    (Default) = "Buka di Switch Explorer"
    Icon      = "C:\…\switch-explorer.exe,0"
HKCU\Software\Classes\Directory\shell\switchexplorer\command
    (Default) = "\"C:\…\switch-explorer.exe\" \"%1\""
HKCU\Software\Classes\Drive\shell\…   (struktur sama untuk root drive)
```

- Mengubah verb default `Directory` juga membuat double-click folder di Explorer/Desktop membuka Switch
  Explorer — memang itu tujuannya saat dijadikan explorer utama.
- Kelas `Folder` **tidak** diubah agar folder virtual (Control Panel, This PC, Recycle Bin) tetap ditangani
  Explorer.

### 5.2 Win+E (`integration.win_e`)

```text
HKCU\Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}\shell\opennewwindow\command
    (Default)       = "\"C:\…\switch-explorer.exe\" --home"
    DelegateExecute = ""          (string kosong, wajib ada)
```

- Efek samping: menjalankan `explorer.exe` tanpa argumen (Win+R) dan ikon File Explorer di taskbar juga
  membuka Switch Explorer.
- Restore: hapus key `{52205fd8-…}` di HKCU **hanya jika kita yang membuatnya**; jika sebelumnya sudah ada
  (dari aplikasi lain), kembalikan dari backup.
- Key ini dikenal sebagai titik persistensi yang juga dipakai malware, sehingga antivirus bisa curiga.
  Mitigasi: opt-in eksplisit, binary ditandatangani, penjelasan di UI (SECURITY.md).

## 6. Yang tidak tercakup & Explorer redirect

- "Show in folder" dari browser memakai `SHOpenFolderAndSelectItems` dan bisa tetap membuka Explorer.
- Folder virtual, Library, dan beberapa entri shell tetap di Explorer (fallback jujur, F-LOC-06).
- **Explorer redirect** (`integration.explorer_redirect`, eksperimental):
  1. `CoCreateInstance(CLSID_ShellWindows)` + connection point `DShellWindowsEvents` → `WindowRegistered`.
  2. Untuk window Explorer baru: `IWebBrowser2` → `IServiceProvider` → `IShellBrowser` →
     `QueryActiveShellView` → `IFolderView` → folder (PIDL → path) + seleksi.
  3. Path file system dan tidak dikecualikan → buka di Switch Explorer (dengan seleksi) → `IWebBrowser2::Quit()`.
  4. Pengecualian: folder virtual; jeda 3 detik setelah pengguna sengaja memilih "Buka di Explorer" dari app kita.
  Kedipan singkat window Explorer adalah konsekuensi yang diterima.

## 7. Entri menu Explorer & Send To

- `integration.explorer_menu_entry`: verb non-default "Buka di Switch Explorer" di
  `HKCU\Software\Classes\Directory\shell\switchexplorer.open` dan `…\Directory\Background\shell\…` (pakai
  `%V` untuk background). Di Windows 11 tampil di "Show more options"; masuk ke menu atas butuh
  `IExplorerCommand` + package identity (P2).
- `integration.sendto_drive`: shortcut di `shell:sendto` → `switch-explorer.exe --upload-to-drive`.

## 8. Layanan shell

| Fitur | API | Catatan |
|---|---|---|
| Ikon per ekstensi | `SHGetFileInfoW(SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES)` + `SHGetImageList` | Cache per indeks |
| Ikon per file | `IShellItemImageFactory::GetImage(SIIGBF_ICONONLY)` | exe, lnk, ico, url, folder khusus |
| Thumbnail | `IShellItemImageFactory::GetImage` pass 1 `SIIGBF_INCACHEONLY`, pass 2 `SIIGBF_THUMBNAILONLY` | Worker STA, cancelable; memakai thumbcache Windows |
| Menu native | `IShellFolder::GetUIObjectOf(IID_IContextMenu)` → `QueryContextMenu` (`CMF_NORMAL | CMF_EXPLORE`, + `CMF_EXTENDEDVERBS` saat Shift) → `TrackPopupMenuEx` → `InvokeCommand` (`CMINVOKECOMMANDINFOEX`) | Selama menu terbuka, teruskan `WM_INITMENUPOPUP`, `WM_DRAWITEM`, `WM_MEASUREITEM`, `WM_MENUCHAR` ke `IContextMenu2/3` lewat `SetWindowSubclass` agar submenu "Send to"/"Open with" berikon |
| Menu background | `SHBindToObject` folder → `IShellFolder::CreateViewObject(IID_IContextMenu)` | Termasuk submenu Baru ▸ |
| Menu native: verb milik sendiri | `GetCommandString(GCS_VERBW)` atas command yang dipilih | `rename`, `delete` (item), dan `paste` (latar folder, atau satu folder item) tidak dijalankan shell. Ketiganya kembali ke Switch Explorer (`sx_shell::menu::OwnVerb`) supaya tercatat di journal, bisa di-undo, dan rename-nya inline. Verb lain dijalankan shell (`InvokeCommand` memakai offset command). |
| Menu native: dua langkah | `sx_shell::menu::prepare` (thread latar: `SHParseDisplayName`, cek induk sama lewat `ILIsParent`) → `show` di modal host | Share jaringan yang offline tidak membekukan window. Path verbatim (`\\?\`, `\\?\UNC\`) dibuang awalannya. Path ≥ 260 karakter ditolak dengan pesan. Hanya menu terbaru yang dibuka (tiket): klik kanan yang lebih tua dibatalkan. |
| Menu native: invoke | `CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE | CMIC_MASK_ASYNCOK`, `CMF_CANRENAME | CMF_ITEMMENU` untuk item | Verb shell boleh berjalan async, jadi verb yang lama tidak menahan window. `ERROR_CANCELLED` (dialog verb dibatalkan) = ditutup. Mode tema menu dikembalikan ke default setelah menu tertutup, karena berlaku untuk seluruh proses. Belum: `SHSetInstanceExplorer` agar verb async menahan proses tetap hidup saat app ditutup. |
| Menu native: posisi & loop | `sx_shell::modal` (ADR-015) → `TrackPopupMenuEx(TPM_RETURNCMD)` | Dari mouse, menu muncul di titik lepas tombol kanan. Dari keyboard (`Shift+F10`), di bawah baris yang fokus, atau di pointer/tengah window bila baris itu tidak terlihat. Multi-pilih memakai item yang satu folder dengan item fokus, karena hasil pencarian bisa lintas folder. |
| Menu native dark | `SetPreferredAppMode` di uxtheme (ordinal 135, tidak terdokumentasi) | `integration.native_menu_dark`; gagal → abaikan |
| Open with | `SHAssocEnumHandlers` → `IAssocHandler`; "Pilih aplikasi lain" → `SHOpenWithDialog` | Tampil di menu bertema kita |
| Properties | `SHObjectProperties` (1 item), `SHMultiFileProperties` (banyak) | Dialog asli Windows |
| Recycle Bin | `IFileOperation` + `FOFX_RECYCLEONDELETE | FOF_ALLOWUNDO` | Di STA ops-shell |
| Preview | `AssocQueryStringW(ASSOCSTR_SHELLEXTENSION, ext, "{8895b1c6-b41f-4c1c-a562-0d564250836f}")` → `IPreviewHandler` di child HWND | Spike di Fase 2 (z-order & backdrop) |
| Properti file | `IPropertyStore` (`SHGetPropertyStoreFromParsingName`) | Dimensi, durasi, penulis |
| Known folders | `SHGetKnownFolderPath` | Desktop, Documents, Downloads, dst. |
| Recent | `%APPDATA%\Microsoft\Windows\Recent\*.lnk` + `IShellLinkW` | Untuk Home |

## 9. OLE drag & drop

**Sebagai sumber (drag keluar):**
- `IDataObject` dari shell untuk item terpilih (`SHCreateDataObject` atau
  `IShellFolder::GetUIObjectOf(IID_IDataObject)`) sehingga aplikasi lain menerima format lengkap.
- Drag image dari bitmap kita (ikon + badge jumlah) lewat `IDragSourceHelper`.
- `SHDoDragDrop` di UI thread (STA), dijalankan lewat modal host (ADR-015), **bukan** langsung di handler
  atau task GPUI. Setelah selesai, kirim `WM_LBUTTONUP` sintetis. Efek yang diizinkan dihitung Drag Guard
  (DEVELOPER-MODE.md §5).

**Sebagai target (drop masuk):**
- `IDropTarget` milik kita di HWND window (ADR-016): `RevokeDragDrop` terhadap target bawaan GPUI (yang
  selalu menjawab COPY dan hanya memberi path), lalu `RegisterDragDrop` milik kita. Event diteruskan ke UI
  lewat channel.
- `DragEnter`/`DragOver`: titik layar → koordinat client → registry zona drop (bounds folder/sidebar/tab yang
  dicatat per frame) → target → efek dari modifier (`MK_CONTROL`/`MK_SHIFT`/`MK_ALT`), Drag Guard, dan
  volume sama/beda → `*pdwEffect`; `IDropTargetHelper` untuk drag image.
- `Drop`: `CF_HDROP` atau "Shell IDList Array"; virtual files lewat `CFSTR_FILEDESCRIPTORW` +
  `CFSTR_FILECONTENTS` (stream). Operasi dijalankan `sx-ops` (journal + undo).
- Skenario pindah: ikuti dokumentasi Microsoft "Handling Shell Data Transfer Scenarios" (*optimized move*,
  *delete-on-paste*, `CFSTR_PERFORMEDDROPEFFECT`, `CFSTR_LOGICALPERFORMEDDROPEFFECT`, `CFSTR_PASTESUCCEEDED`).

## 10. Clipboard

Fase 1 (`sx_shell::clipboard`):
- Copy/cut: clipboard Win32 dengan `CF_HDROP` + `Preferred DropEffect` (`DROPEFFECT_COPY`, atau
  `DROPEFFECT_MOVE` untuk cut), sehingga paste di Explorer bekerja dan sebaliknya. Data langsung diisi (tanpa
  delayed rendering), jadi tidak perlu `OleFlushClipboard` saat keluar.
- Paste: `CF_HDROP` + `Preferred DropEffect` (ukuran blok dicek dengan `GlobalSize`) → rencana operasi.
- Semua akses clipboard berjalan di satu thread clipboard (`StaWorker`, berurutan: paste tepat setelah copy
  melihat copy itu), bukan UI thread. App lain bisa memegang clipboard (dicoba ulang), dan data yang
  di-render app lain saat diminta menunggu app itu.
- Nomor urut clipboard dibaca saat clipboard masih terbuka, setelah datanya (render data on demand bisa
  mengubahnya).
- Setelah cut-paste selesai **tanpa error dan tanpa batal**, clipboard dikosongkan, tetapi hanya bila
  `GetClipboardSequenceNumber` masih sama dengan saat dibaca (dicek saat clipboard terbuka). Salinan baru
  user di app mana pun tidak pernah terhapus.

Fase 2: `IDataObject` shell lewat `OleSetClipboard` (untuk "Shell IDList Array", item virtual) dan "Paste
Succeeded" sesuai skenario delete-on-paste.

## 11. Operasi file & elevasi

- `CopyFile2` dengan `COPYFILE2_EXTENDED_PARAMETERS` + callback progress (continue/cancel/stop);
  `COPY_FILE_RESTARTABLE` untuk pause/resume file besar.
- `MoveFileExW` untuk volume yang sama (rename instan).
- `IFileOperation` (STA ops-shell) untuk Recycle Bin, namespace shell (ponsel/MTP, ZIP), dan lokasi yang
  butuh admin: Windows menampilkan dialog UAC dan menjalankan operasi dengan hak tinggi lewat COM elevation.
  Progress diambil lewat `IFileOperationProgressSink` dengan UI bawaan disembunyikan.
- Recycle & restore (`sx_shell::recycle`): satu sink per item. `PostDeleteItem` memberi slot `$R…` item di
  Recycle Bin (atau tanpa hasil = dihapus permanen setelah peringatan shell). Restore = `MoveItem` dari slot
  itu ke folder asal (`FOF_RENAMEONCOLLISION`), `PostMoveItem` memberi path akhir, lalu file `$I…`
  pasangannya dihapus. Hanya `$I` di samping slot `$R` di dalam folder `$Recycle.Bin` yang pernah dihapus.
  Sink memanggil `Compare` (`SICHINT_CANONICAL`) agar laporan untuk isi folder tidak tercampur dengan item
  itu sendiri. Sink tidak agile (`Agile = false`): COM selalu memanggilnya di STA pembuatnya.
- Item dihitung "dihapus permanen" hanya bila hasilnya `S_OK`, tanpa item hasil, dan item aslinya memang
  sudah tidak ada. Kode sukses lain (`COPYENGINE_S_USER_IGNORED` saat user menjawab No di peringatan) =
  item tetap di tempat, dihitung batal.
- Sebelum restore: file `$I` slot harus mencatat path asli yang sama (slot `$R` bisa dipakai ulang setelah
  Recycle Bin dikosongkan), dan nama tujuan dipilih dulu ("nama (2)") bila nama asli sudah dipakai, supaya
  shell tidak pernah bertemu tabrakan (ia bisa menggabungkan folder). Callback yang berisi logika dibungkus `catch_unwind`; sisanya hanya mengembalikan `S_OK`.

## 12. Taskbar, notifikasi, hotkey global

- `ITaskbarList3::SetProgressValue/SetProgressState` untuk progress operasi (`sx_shell::taskbar`,
  `ops.taskbar_progress`): item selesai dari total item semua job yang berjalan; tak tentu bila total belum
  diketahui; hilang saat tidak ada job. Panggilannya menunggu proses Explorer, jadi berjalan di `StaWorker`
  sendiri, dan pembaruan digabung (hanya state terbaru yang dikirim). Flag mati → objeknya tidak dibuat.
  Flash taskbar saat operasi panjang selesai di background menyusul.
- Notifikasi Windows (opsional) untuk operasi panjang yang selesai saat app tidak fokus; butuh AppUserModelID
  `SwitchExplorer.App`.
- **Hotkey global (Hot Switch & buka window):** `HotkeyService` di `sx-integration` membuat message-only
  window, mendaftarkan tiap binding scope global dengan `RegisterHotKey` (+ `MOD_NOREPEAT`), meneruskan
  `WM_HOTKEY` ke UI thread lewat channel, dan `UnregisterHotKey` saat binding dihapus/app keluar.
  Hanya kombinasi tunggal. Kegagalan pendaftaran (sudah dipakai aplikasi lain) dilaporkan ke keymap editor.
  OSD untuk hotkey global tampil di monitor tempat window foreground berada, sebagai window topmost yang tidak
  mengambil fokus.

## 13. File picker — Level 1: Picker Bridge (`integration.picker_bridge`)

Panel pendamping yang menempel di dialog Open/Save **asli**. Tanpa injeksi DLL, risiko rendah.

1. **Deteksi:** `SetWinEventHook(EVENT_OBJECT_SHOW, …, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS)`;
   saring window kelas `#32770` yang berisi `DirectUIHWND` dan kotak nama file (ComboBoxEx dengan ID kontrol
   1148 — verifikasi dengan Accessibility Insights/Inspect). Tentukan Open/Save dari tombol utama (AutomationId
   "1") dan keberadaan "Save as type". Ambil nama proses untuk daftar pengecualian. Proses target elevated
   sementara kita tidak → lewati (UIPI).
2. **UI:** window tool owned (`WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`) menempel di sisi kanan dialog (atau bawah
   bila ruang sempit), mengikuti lewat `EVENT_OBJECT_LOCATIONCHANGE`, hilang saat dialog ditutup. Isi: folder tab
   aktif Switch Explorer, Pinned, folder terbaru, Google Drive (unduh lalu pilih), tombol "Telusuri di Switch
   Explorer…" (window mode picker, DESIGN.md §8.7).
3. **Aksi:** navigasi = isi kotak nama file dengan path folder via UI Automation `ValuePattern` (fallback
   `WM_SETTEXT`) lalu invoke tombol utama; dialog berpindah folder; teks nama file semula dikembalikan (Save
   dialog). Memilih file = navigasi ke folder lalu isi `"a.jpg" "b.pdf"` dan invoke. File Drive diunduh dulu ke
   `cloud-cache\`.
4. **Hotkey:** `integration.picker_bridge_hotkey` (`Ctrl+G`) saat dialog fokus → lompat ke folder tab aktif.
5. **Batasan:** dialog milik proses elevated, picker UWP/WinUI (`FileOpenPicker`, lewat broker), dialog buatan
   sendiri (Java Swing, sebagian game).

## 14. File picker — Level 2: Picker Takeover (`integration.picker_takeover`, eksperimental)

Mengganti dialog Open/Save dengan UI Switch Explorer. Preseden: Files (files-community) punya proyek
eksperimental untuk meng-override `FileOpenDialog`/`FileSaveDialog`. Pengembang file manager lain menyebut
pendekatan pengganti rawan merusak aplikasi yang memodifikasi dialog — karena itu desain kita adalah
**proxy yang membungkus dialog asli** dengan fallback di setiap langkah. Default mati, butuh Developer Mode.

**Registrasi per-user:**

```text
HKCU\Software\Classes\CLSID\{DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7}\InprocServer32   (FileOpenDialog)
    (Default)      = "C:\…\sx_picker_shim_x64.dll"
    ThreadingModel = "Apartment"
HKCU\Software\Classes\CLSID\{C0B4E2F3-BA21-4773-8DBA-335EC946EB8B}\InprocServer32   (FileSaveDialog)
    … sama
Aplikasi 32-bit: tampilan registry WOW64 (HKCU\Software\Classes\WOW6432Node\CLSID\{…}) → sx_picker_shim_x86.dll
```
Perilaku redirection 32-bit WAJIB diverifikasi di mesin uji.

**Dialog asli di dalam proxy:** muat `comdlg32.dll` dari System32 (SysWOW64 untuk x86) dengan
`LoadLibraryExW(LOAD_LIBRARY_SEARCH_SYSTEM32)`, ambil `DllGetClassObject`, buat instance CLSID yang sama.
**Jangan** memakai `CoCreateInstance` (akan memanggil shim kita sendiri secara rekursif).

**Interface proxy:** `IModalWindow`, `IFileDialog`, `IFileOpenDialog`/`IFileSaveDialog`, `IFileDialog2`,
`IFileDialogCustomize` (diteruskan; bila aplikasi menambah kontrol kustom → kebijakan fallback ke native),
`IOleWindow`. IID lain → `E_NOINTERFACE` (risiko: aplikasi yang meminta interface privat → masuk daftar
pengecualian). Semua setter diteruskan ke dialog asli **dan** dicatat (judul, label tombol, file types, index,
default extension, folder, nama file, options, sink `Advise`).

**Alur `Show(hwndOwner)`:**
1. Bypass ke `inner.Show` bila: `Shift` ditahan, proses dikecualikan, ada kontrol kustom, options belum
   didukung, takeover dimatikan, atau penghitung kegagalan ≥ 3.
2. Konek pipe (300 ms); bila server belum ada → jalankan `switch-explorer.exe --picker-host`, coba lagi
   hingga 1500 ms; gagal → `inner.Show`.
3. Kirim `PickRequest`; `EnableWindow(owner, FALSE)`; pompa pesan (`MsgWaitForMultipleObjectsEx`) agar aplikasi
   host tetap menggambar; tunggu `PickResult` atau owner tertutup.
4. Berhasil → buat `IShellItem` (`SHCreateItemFromParsingName`) / `IShellItemArray`; panggil
   `IFileDialogEvents::OnFileOk` milik aplikasi — bila `S_FALSE`, picker diberi tahu dan tetap terbuka;
   kembalikan `S_OK`. Batal → `HRESULT_FROM_WIN32(ERROR_CANCELLED)` (`0x800704C7`). Error → `inner.Show`.
5. `EnableWindow(owner, TRUE)` dan kembalikan foreground. `GetResult(s)`/`GetFileTypeIndex` mengembalikan
   nilai dari picker kita.

**Semantik Save di UI picker:** kotak nama file, pilihan tipe (menerapkan default extension), konfirmasi
timpa (`FOS_OVERWRITEPROMPT`), `FOS_PATHMUSTEXIST`, `FOS_NOREADONLYRETURN`, `FOS_CREATEPROMPT`.

**Kill switch:** (a) toggle Settings; (b) hotkey global `Ctrl+Alt+Shift+F12` menghapus key HKCU seketika;
(c) `--picker-takeover off`; (d) `cargo xtask cleanup-registry`; (e) otomatis: shim menulis penanda
"sedang berjalan" per `Show`; jika aplikasi host crash saat dialog terbuka, penanda basi terdeteksi pada
pemanggilan berikutnya dan penghitung kegagalan naik; setelah 3 kali shim selalu bypass dan app memberi tahu.

**Risiko kritis:** bila file DLL terhapus/berpindah saat masih terdaftar, aplikasi **tidak bisa membuka dialog
file sama sekali** (tidak ada fallback otomatis ke registrasi sistem). Mitigasi: DLL dipasang di lokasi stabil
`%LOCALAPPDATA%\SwitchExplorer\shim\`, diverifikasi setiap app start, uninstaller menghapus key lebih dulu,
tersedia `restore-file-dialogs.reg`, dan peringatan jelas di UI.

**Catatan lain:** proses elevated kemungkinan besar mengabaikan registrasi COM per-user (HKCU) sehingga tetap
memakai dialog native — perilaku aman, tetap diverifikasi. Aplikasi Store/UWP dan browser yang menjalankan
dialog di proses utilitas terpisah wajib masuk matriks uji (TESTING.md §5).

## 15. Manifest, DPI, path panjang

- Manifest aplikasi: DPI awareness PerMonitorV2, `longPathAware`, supportedOS Windows 10/11, Common Controls v6.
  **Temuan Fase 0:** `gpui-pre-platform` memaksa fitur `windows-manifest` di Windows, sehingga manifest
  GPUI (resource ID 1: PerMonitorV2, SegmentHeap, ComCtl v6, asInvoker) selalu tersemat di exe.
  Manifest itu tanpa `longPathAware`, dan manifest kedua akan bentrok. Keputusan sementara: path panjang
  ditangani prefix `\\?\` di `sx-win`. Bila `longPathAware` tetap dibutuhkan, manifest diganti
  pasca-link dengan `mt.exe` di `xtask` (bukan fork GPUI).
- Path > 260 karakter: std Rust menangani prefix verbatim secara otomatis. Panggilan Win32 kita sendiri
  **selalu** memakai bentuk verbatim (`\\?\C:\…`, `\\?\UNC\server\share\…`) dari path absolut yang
  dinormalisasi secara leksikal (`/` → `\`, `.`/`..`, pemisah ganda), lewat `sx_win::win32_path`. Alasannya:
  - tidak ada batas `MAX_PATH`;
  - nama yang berakhiran titik atau spasi sampai ke file system persis seperti saat didaftar (Win32 biasa
    memangkasnya dan membuka item **lain**).
- Path relatif dan relatif-drive (`docs`, `C:`, `\docs`) ditolak, karena akan diselesaikan terhadap CWD proses
  yang bisa diubah kode lain. Path dengan NUL di tengah ditolak, bukan dipotong.
- Nama yang bukan UTF-16 valid (surrogate tunggal, mungkin di volume SMB/WSL) ditampilkan secara lossy, tetapi
  path selalu dibangun dari nama aslinya (`DirSnapshot::path_of`). Teks tampilan tidak pernah dipakai untuk
  membangun path atau membandingkan identitas.
- Tampilan memakai path tanpa prefix; `ShellExecuteEx` menerima bentuk biasa.

## 16. Cleanup & uninstall

`cargo xtask cleanup-registry` dan `switch-explorer.exe --unregister all` menghapus/mengembalikan:
- `HKCU\Software\Classes\Directory\shell\switchexplorer*`, `…\Drive\shell\switchexplorer*`,
  `…\Directory\Background\shell\switchexplorer*` (+ mengembalikan nilai default `shell` dari backup)
- `HKCU\Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}` (jika dibuat oleh kita)
- `HKCU\Software\Classes\CLSID\{DC1C5A9C-…}` dan `{C0B4E2F3-…}` (+ tampilan WOW64)
- Entri `Run` (resident startup), shortcut Send To
- Semua hotkey global dilepas otomatis saat proses berakhir

Uninstaller menjalankan langkah yang sama **sebelum** menghapus file.

Aturan keamanan `cleanup-registry` (`xtask/src/registry.rs`):
- Key yang hanya dibuat Switch Explorer (verb `switchexplorer*`, value `Run\SwitchExplorer`) dihapus bila
  ada.
- Key bersama (CLSID Win+E, proxy dialog termasuk tampilan WOW64) dihapus **hanya** bila nilainya masih
  menunjuk `switch-explorer.exe` atau `sx_picker_shim`. Registrasi aplikasi lain tidak pernah disentuh.
- `--dry-run` hanya melaporkan.
- Pemulihan nilai dari `integration-backup.json` ditambahkan bersama `sx_integration::registry`
  (Fase 5).
