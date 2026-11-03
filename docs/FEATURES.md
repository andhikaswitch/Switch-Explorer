# Spesifikasi Fitur

Setiap fitur punya ID `F-AREA-nn`, prioritas (P0/P1/P2), dan fase (F0–F8). Flag terkait ditulis sebagai
`flag.id` (detail di `DEVELOPER-MODE.md`). Tampilan komponen ada di `DESIGN.md`.

## F-WIN — Window, tab, split

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-WIN-01 | Window kustom: title bar 40 px berisi tab, tombol min/max/close gaya Windows 11, snap layout saat hover tombol maximize, double-click title bar = maximize, backdrop sesuai `appearance.backdrop` | P0 | F1 |
| F-WIN-02 | Tab: `Ctrl+T`, `Ctrl+W`, `Ctrl+Tab`/`Ctrl+Shift+Tab`, `Ctrl+1..9`; middle-click folder = tab baru (`nav.middle_click_new_tab`); drag tab untuk urut ulang; drag tab keluar = window baru (P2); dedupe (`tabs.dedupe`) | P0 | F1 |
| F-WIN-03 | Multi-window (`Ctrl+N`), semua dalam satu proses | P0 | F1 |
| F-WIN-04 | Split view (`Ctrl+\`): dua panel kiri/kanan atau atas/bawah; `F6` pindah fokus; drag antar panel; aksi "Salin/Pindahkan ke panel lain" di context menu & palette (shortcut default kosong) | P1 | F2 |
| F-WIN-05 | Session restore (`tabs.restore_session`): window, tab, split, ukuran pane; hanya tab aktif yang dienumerasi saat start | P1 | F2 |
| F-WIN-06 | Responsif sesuai DESIGN.md §8.8 | P0 | F1 |

## F-NAV — Navigasi & address bar

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-NAV-01 | Back/forward/up: `Alt+←/→/↑`, tombol mouse 4/5, `Backspace` (`nav.backspace_action`: back/up) | P0 | F1 |
| F-NAV-02 | Breadcrumb: klik segmen = navigasi; chevron ▸ = dropdown subfolder; klik area kosong/`Alt+D`/`Ctrl+L`/`F4` = mode edit dengan path terpilih | P0 | F1 |
| F-NAV-03 | Input path: absolut, relatif, UNC, `\\wsl.localhost\`, variabel lingkungan (`%APPDATA%`), `shell:` (`shell:startup`, `shell:sendto`) via `SHParseDisplayName`, `~` = folder profil; autocomplete (`addressbar.autocomplete`); path ke file → buka folder induk & pilih file | P0 | F1 |
| F-NAV-04 | Membuka item: Enter/double-click (single-click opsional `nav.single_click_open`); folder = navigasi; file = verb default `ShellExecuteEx`; `Ctrl+Enter`/middle-click = tab baru; `Shift+Enter` = window baru | P0 | F1 |
| F-NAV-05 | Type-to-select: mengetik saat fokus di daftar → lompat ke item, reset setelah 1 detik | P0 | F1 |
| F-NAV-06 | Riwayat per tab + lokasi terbaru di dropdown address bar | P1 | F2 |
| F-NAV-07 | Quick open `Ctrl+P`: lompat ke folder dari pinned, tab terbuka, dan lokasi terbaru (fuzzy) | P1 | F2 |

## F-SIDE — Sidebar

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-SIDE-01 | Seksi: Home, Pinned, This PC (drive + gauge), Google Drive (bila terhubung), Network (P2), WSL (bila ada), Recycle Bin, Tools (bisa disembunyikan) | P0 | F1 |
| F-SIDE-02 | Pin/unpin folder (klik kanan → "Pin ke sidebar"); urut ulang via drag (`sidebar.pins_locked`); impor pin dari Quick Access Windows (P2) | P0 | F1 |
| F-SIDE-03 | Mode pohon (`sidebar.tree`): expand folder langsung di sidebar | P1 | F2 |
| F-SIDE-04 | Resize, collapse ke rail 56 px, `Ctrl+B` tampil/sembunyi | P0 | F1 |
| F-SIDE-05 | Footer kustom: ringkasan transfer, ringkasan drive, gambar kustom (ricing), atau kosong | P2 | F3 |
| F-SIDE-06 | Gauge drive: warna peringatan ≥ 85%, bahaya ≥ 95%; tooltip kapasitas; eject drive removable (P2) | P0 | F1 |

## F-VIEW — Tampilan daftar

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-VIEW-01 | Details: kolom Nama, Diubah, Tipe, Ukuran; opsional Dibuat, Diakses, Atribut, Ekstensi, Path (hasil search), Dimensi/Durasi (property store, lazy). Resize & urut ulang kolom; klik header = sort, klik lagi = balik. Ukuran rata kanan, angka tabular; ukuran folder "—" kecuali `view.folder_sizes` | P0 | F1 |
| F-VIEW-02 | List, Tiles, Icons S/M/L/XL, Gallery, Content: `Ctrl+Shift+1..8` (paritas Explorer: 1 XL, 2 L, 3 M, 4 S, 5 list, 6 details, 7 tiles, 8 content); `Ctrl+scroll` ubah ukuran | P1 | F2 |
| F-VIEW-03 | Sort: folders-first (`view.folders_first`), natural sort (`view.natural_sort`), group by tipe/tanggal (P2) | P0 | F1 |
| F-VIEW-04 | File tersembunyi (`view.show_hidden`), file sistem (`view.show_system`), ekstensi (`view.show_extensions`) | P0 | F1 |
| F-VIEW-05 | Mode tampilan diingat per folder (`view.remember_per_folder`, `views.json` LRU 2000) | P1 | F2 |
| F-VIEW-06 | Densitas compact/default/comfortable | P1 | F3 |
| F-VIEW-07 | Checkbox seleksi (`view.checkboxes`) | P1 | F2 |
| F-VIEW-08 | Item hasil cut tampil pudar (`view.cut_items_faded`) | P1 | F1 |
| F-VIEW-09 | Satuan ukuran 1024 dengan label KB/MB/GB seperti Explorer; opsi SI (`view.si_units`) | P2 | F3 |

## F-SEL — Seleksi

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-SEL-01 | Klik, `Ctrl`+klik, `Shift`+klik, `Ctrl+A`, `Ctrl+Space` (toggle item fokus), invert selection (menu), rubber-band dari area kosong dengan auto-scroll di tepi | P0 | F1 |
| F-SEL-02 | Ringkasan seleksi di status bar: jumlah + total ukuran file | P0 | F1 |
| F-SEL-03 | Floating action bar saat ≥ 2 item terpilih: Salin, Pindahkan ke…, Upload ke Drive, Kompres, Hapus | P1 | F2 |

## F-OPS — Operasi file

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-OPS-01 | Copy/cut/paste `Ctrl+C/X/V`, interoperabel dengan Explorer (`CF_HDROP` + `Preferred DropEffect`); paste as shortcut (P2) | P0 | F1 |
| F-OPS-02 | Delete → Recycle Bin (`Del`, `Ctrl+D`; `ops.delete_to_recycle_bin`); `Shift+Del` → permanen dengan konfirmasi (`ops.confirm_permanent_delete`) | P0 | F1 |
| F-OPS-03 | Rename inline `F2`: nama tanpa ekstensi terpilih otomatis, `F2` lagi = pilih semua, `Tab` = item berikutnya; tolak karakter `\ / : * ? " < > \|` dan nama reserved (CON, PRN, AUX, NUL, COM1–9, LPT1–9); peringatan saat mengganti ekstensi | P0 | F1 |
| F-OPS-04 | Folder baru `Ctrl+Shift+N` langsung mode rename; file baru dari template (P2) | P0 | F1 |
| F-OPS-05 | Konflik: sheet perbandingan (thumbnail, ukuran, tanggal) — Ganti, Lewati, Simpan keduanya ("nama (2)"), "Ganti jika lebih baru", "Terapkan ke semua" | P0 | F1 |
| F-OPS-06 | Progress: toast ringkas + panel Transfer (`Ctrl+J`) berisi nama file, n/total, MB/s, sisa waktu, pause/resume/cancel; progress taskbar (`ops.taskbar_progress`); popup mengambang di pojok layar (`ops.progress_popup`) | P0 | F1 |
| F-OPS-07 | Undo/redo `Ctrl+Z`/`Ctrl+Y`: rename, move, copy (salinan → Recycle Bin), delete-ke-Recycle-Bin (restore), folder baru; stack sesi + journal persisten (`ops.undo_depth`) | P0 | F1 |
| F-OPS-08 | Akses ditolak di lokasi terlindungi → "Lanjutkan sebagai admin" lewat `IFileOperation` (UAC) | P1 | F2 |
| F-OPS-09 | Kompres/ekstrak ZIP bawaan; 7z/rar lewat 7-Zip bila terpasang (P2) | P1 | F7 |
| F-OPS-10 | Properties `Alt+Enter` → dialog properties Windows | P0 | F2 |
| F-OPS-11 | Copy path `Ctrl+Shift+C` (dengan tanda kutip, seperti Windows 11); varian di menu: tanpa kutip, path WSL (`/mnt/c/...`), path ter-escape | P0 | F1 |
| F-OPS-12 | Buka terminal di sini (Windows Terminal `wt -d`, PowerShell, atau cmd sesuai setting) dan Buka di VS Code bila terpasang | P1 | F2 |
| F-OPS-13 | Mode baca-saja (`safety.readonly_mode`): semua operasi tulis diblokir dengan pesan jelas | P1 | F2 |

## F-CTX — Context menu

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-CTX-01 | Menu bertema: Buka, Buka di tab baru, Buka dengan ▸, Potong, Salin, Tempel, Ganti nama, Hapus, Salin path, Upload ke Drive ▸ (tujuan favorit + Pilih…), Pindahkan ke Drive…, Kompres, Pin ke sidebar, Properties, **Tampilkan opsi lain** | P0 | F2 |
| F-CTX-02 | "Tampilkan opsi lain" / `Shift+F10` / `Shift`+klik kanan = menu native (`IContextMenu`), termasuk entri pihak ketiga (7-Zip, VS Code) | P0 | F2 |
| F-CTX-03 | Menu area kosong: Tampilan ▸, Urutkan ▸, Baru ▸, Tempel, Buka terminal di sini, Properties | P0 | F2 |
| F-CTX-04 | Menu item sidebar: Buka, Buka di tab baru, Unpin, Naik/Turun, Properties | P0 | F2 |
| F-CTX-05 | Setiap item menu adalah command dari `CommandRegistry` (label & shortcut sama dengan palette) | P0 | F2 |

## F-DND — Drag & drop

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-DND-01 | Drag internal ke folder di daftar, item sidebar, segmen breadcrumb, dan tab (tab berpindah otomatis saat di-hover) | P0 | F2 |
| F-DND-02 | OLE keluar: drag ke Chrome, WhatsApp, Discord, Desktop, Explorer (`dnd.external_out`) | P0 | F2 |
| F-DND-03 | OLE masuk: dari Explorer/aplikasi lain termasuk virtual files (lampiran Outlook, isi ZIP) (`dnd.external_in`) | P0 | F2 |
| F-DND-04 | Efek: `Ctrl` = salin, `Shift` = pindah, `Alt` = shortcut; tanpa modifier → `dnd.default_action` (system: pindah di volume sama, salin beda volume; atau copy/move/ask) | P0 | F2 |
| F-DND-05 | Drag tombol kanan selalu menampilkan menu saat drop (paritas Explorer) | P0 | F2 |
| F-DND-06 | **Drag Guard**: switch per sumber/target, cooldown setelah scroll, ambang jarak, tahan-dulu, modifier wajib, konfirmasi pindah folder (DEVELOPER-MODE.md §5) | P0 | F2 |
| F-DND-07 | Hint saat drag diblokir switch + tombol Aktifkan (`dnd.blocked_hint`, DESIGN.md §9.3) | P0 | F2 |
| F-DND-08 | Spring-loaded folder: hover saat drag membuka folder (`dnd.spring_open`, `dnd.spring_delay_ms`) | P1 | F2 |
| F-DND-09 | Toast "Urungkan" 10 detik setelah drop (`dnd.undo_toast`) | P0 | F2 |

## F-KEY — Hot Switch & keymap

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-KEY-01 | **Hot Switch**: setiap flag bool/enum bertanda `hotswitchable` bisa diberi hotkey. Default: lihat tabel shortcut di bawah dan DEVELOPER-MODE.md §6 | P0 | F2 |
| F-KEY-02 | Leader `Ctrl+K`: setelah ditekan, status bar menampilkan "Ctrl+K…" selama `hotswitch.chord_timeout_ms`; tombol kedua menjalankan; `Esc` batal; tombol tak dikenal → pesan "Tidak ada switch untuk Ctrl+K, Q". Chord tidak aktif saat fokus di input teks | P0 | F2 |
| F-KEY-03 | **OSD status**: muncul ≤ 50 ms setelah tombol, menyebut nama switch, status baru, dampaknya ("Drag file tetap aktif"), dan hotkey untuk membalik (DESIGN.md §9.1) | P0 | F2 |
| F-KEY-04 | **Chip status** "Drag: aktif/sebagian/mati" di status bar; tooltip rinci; klik → panel Hot Switch; badge title bar saat master mati | P0 | F2 |
| F-KEY-05 | Panel Hot Switch (`Ctrl+K, K`): semua switch, status, hotkey; cari; `Space`/`Enter` membalik | P1 | F2 |
| F-KEY-06 | Scope global per binding (opsional): `RegisterHotKey`, hanya kombinasi tunggal; OSD tampil di monitor aktif walau app di background; bekerja tanpa window jika resident mode aktif | P1 | F3 |
| F-KEY-07 | Persisten vs sesi (`hotswitch.persist`) | P0 | F2 |
| F-KEY-08 | Keymap editor (Settings → Keyboard): rekam kombinasi, deteksi konflik (internal, shortcut sistem, paritas Explorer, AltGr), reset per item/semua, impor/ekspor `keymap.toml` | P1 | F3 |
| F-KEY-09 | Semua aksi app (bukan hanya switch) bisa di-rebind | P1 | F3 |

## F-SRCH — Pencarian

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-SRCH-01 | Filter instan di folder aktif (`Ctrl+F` atau mengetik di kotak cari), highlight kecocokan, ≤ 16 ms per ketikan untuk 10k item | P0 | F2 |
| F-SRCH-02 | Pencarian rekursif (Enter di kotak cari, `Ctrl+E`, `F3`): hasil streaming + kolom Path; operator `ext:pdf`, `size:>10mb`, `modified:7d`, `kind:folder`, wildcard `*`/`?`; dibatalkan saat query berubah | P1 | F2 |
| F-SRCH-03 | Integrasi Everything (`search.everything`): bila Everything berjalan, pakai IPC-nya untuk hasil instan seluruh disk | P1 | F2 |
| F-SRCH-04 | Fuzzy (palette & quick open) via nucleo (`search.fuzzy`) | P1 | F2 |
| F-SRCH-05 | Cari isi file teks/kode (`search.content`, beta) | P2 | F7 |

## F-PREV — Preview

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-PREV-01 | Preview pane (`Alt+P`): gambar (WIC), teks/kode, metadata audio/video + thumbnail, PDF/Office lewat preview handler sistem (`module.preview_handlers`) | P1 | F2 |
| F-PREV-02 | Details pane (`Alt+Shift+P`): properti dari property system (ukuran, tanggal, dimensi, durasi, penulis) | P1 | F2 |
| F-PREV-03 | Quick Look (`Space`, `view.quick_look`): overlay besar; panah kiri/kanan = item berikutnya; `Space`/`Esc` tutup; fokus daftar tidak hilang | P1 | F2 |

## F-THUMB — Ikon & thumbnail

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-THUMB-01 | Ikon sistem: per ekstensi (system image list) + per file untuk exe/lnk/ico/url/folder khusus; di-cache | P0 | F1 |
| F-THUMB-02 | Thumbnail (`view.thumbnails`): pass 1 dari cache Windows (instan), pass 2 ekstraksi; hanya item terlihat + prefetch; batal saat scroll menjauh | P1 | F2 |
| F-THUMB-03 | Ikon folder bertema (`view.themed_folder_icons`) dengan glyph folder khusus; ikon sistem tetap tersedia agar overlay status cloud terlihat | P1 | F2 |

## F-LOC — Lokasi khusus

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-LOC-01 | This PC: drive + gauge, folder pengguna (Desktop, Documents, Downloads, Pictures, Music, Videos) via Known Folders | P0 | F1 |
| F-LOC-02 | Recycle Bin: daftar item terhapus, Restore, hapus permanen, kosongkan | P1 | F2 |
| F-LOC-03 | Path UNC; jelajah network (P2) | P1 | F2 |
| F-LOC-04 | WSL: `\\wsl.localhost\<distro>` otomatis bila WSL terpasang | P2 | F2 |
| F-LOC-05 | Perangkat portabel (MTP/ponsel) via shell namespace | P2 | F8 |
| F-LOC-06 | Lokasi shell yang belum didukung dibuka di Explorer dengan pemberitahuan (fallback jujur) | P0 | F1 |

## F-HOME — Home dashboard

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-HOME-01 | Gauge storage: drive lokal (data asli) + akun Google Drive (kuota via API); klik = buka | P1 | F2 |
| F-HOME-02 | Grid pinned | P1 | F2 |
| F-HOME-03 | Kategori pintar (Dokumen, Audio, Gambar, Video, Arsip, Kode) = saved search atas folder sumber (default Desktop, Documents, Downloads); hitungan dihitung lazy di background atau via Everything; "+ Kategori"; taksonomi sama dengan Organizer | P1 | F2 |
| F-HOME-04 | Terbaru: dari Recent Items Windows (`%APPDATA%\Microsoft\Windows\Recent`, resolve `.lnk`), tanpa tracking sendiri | P1 | F2 |
| F-HOME-05 | Aktivitas terakhir: 5 operasi terakhir dengan tombol Urungkan | P1 | F2 |

## F-HIST — Aktivitas

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-HIST-01 | Halaman Aktivitas: journal operasi (waktu, jenis, sumber → tujuan, jumlah, status), filter, cari; Urungkan per entri bila kondisi file masih memungkinkan | P1 | F2 |
| F-HIST-02 | Pemulihan setelah crash: operasi yang belum selesai ditawarkan Lanjutkan/Batalkan | P0 | F1 |

## F-TOOL — Tools

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-TOOL-01 | **Organizer** (`tools.organizer`): pilih folder (tombol, drag, atau klik kanan → "Rapikan dengan Organizer"); bar komposisi tersegmentasi; pilih per kategori, per ekstensi (chip n/total), atau manual; template tujuan `{category}`, `{ext}`, `{year}`, `{month}`, `{created:%Y-%m}`, `{size_bucket}`; **dry-run** pohon sebelum/sesudah + konflik; eksekusi lewat sx-ops (progress, cancel, undo seluruh run); preset; pengecualian (tersembunyi, `.git`, `node_modules`); otomatis untuk Downloads (opsional, default mati); deteksi tipe via magic bytes (opsional) | P1 | F7 |
| F-TOOL-02 | **Duplicate Finder** (`tools.duplicates`): kelompok ukuran → hash parsial (64 KB awal & akhir) → hash penuh BLAKE3; strategi simpan (terbaru/terlama/path terpendek/di folder X); hapus ke Recycle Bin; hardlink (lanjutan) | P2 | F7 |
| F-TOOL-03 | **Bulk Rename** (`tools.bulk_rename`): token `{name}`, `{n:03}`, `{date}`, `{ext}`, regex dengan grup, ubah huruf, tambah/hapus teks, tanggal EXIF (opsional); tabel pratinjau lama → baru; deteksi konflik & karakter ilegal; undo | P2 | F7 |
| F-TOOL-04 | **Space Analyzer** (`tools.space_analyzer`): scan paralel, treemap squarified + daftar terbesar, drill-down, aksi hapus/pindah, cache hasil | P2 | F7 |
| F-TOOL-05 | **Sync** (`tools.sync`, eksperimental): mirror satu arah folder → folder/Drive dengan pratinjau | P2 | setelah v1 |

## F-CMD, F-SET, F-NOTIF, F-I18N

| ID | Fitur | P | Fase |
|---|---|---|---|
| F-CMD-01 | Command palette (`Ctrl+Shift+P`, `F1`): semua command, switch, dan navigasi; fuzzy; menampilkan shortcut | P1 | F2 |
| F-SET-01 | Settings: Umum, Tampilan, Keyboard, Drag & drop, File & operasi, Integrasi (onboarding + status + perbaiki), Akun, Tools, Performa, Developer (saat dev mode), Tentang | P0 | F1–F3 |
| F-NOTIF-01 | Toast in-app (kanan bawah, maks 3, hilang 4 s kecuali error), OSD Hot Switch (tengah bawah), panel Transfer, notifikasi Windows opsional saat operasi panjang selesai di background | P0 | F1–F2 |
| F-I18N-01 | Bahasa mengikuti bahasa tampilan Windows (id → Indonesia, lainnya → Inggris), bisa diganti; format tanggal/angka mengikuti locale Windows | P0 | F1 |

## Shortcut default

Semua shortcut bisa diatur ulang di Settings → Keyboard.

| Aksi | Shortcut |
|---|---|
| Window baru | `Ctrl+N` |
| Tab baru / tutup / berikutnya / sebelumnya | `Ctrl+T` / `Ctrl+W` / `Ctrl+Tab` / `Ctrl+Shift+Tab` |
| Lompat ke tab 1–9 | `Ctrl+1..9` |
| Back / forward / up | `Alt+←` / `Alt+→` / `Alt+↑` (Backspace = back) |
| Fokus address bar | `Alt+D`, `Ctrl+L`, `F4` |
| Filter / cari | `Ctrl+F` / `Ctrl+E`, `F3` |
| Refresh | `F5` |
| Rename | `F2` |
| Properties | `Alt+Enter` |
| Folder baru | `Ctrl+Shift+N` |
| Hapus ke Recycle Bin / permanen | `Del`, `Ctrl+D` / `Shift+Del` |
| Salin / potong / tempel | `Ctrl+C` / `Ctrl+X` / `Ctrl+V` |
| Undo / redo | `Ctrl+Z` / `Ctrl+Y` |
| Pilih semua / toggle item fokus | `Ctrl+A` / `Ctrl+Space` |
| Copy path | `Ctrl+Shift+C` |
| Mode tampilan | `Ctrl+Shift+1..8` |
| Ukuran ikon | `Ctrl+scroll` |
| Preview pane / details pane | `Alt+P` / `Alt+Shift+P` |
| Quick Look | `Space` |
| Split view | `Ctrl+\` |
| Pindah fokus area (command bar → sidebar → daftar) | `F6` / `Shift+F6` |
| Pindah antar kontrol (tab, tombol, breadcrumb, item sidebar) | `Tab` / `Shift+Tab`; `Enter`/`Space` mengaktifkan |
| Sidebar tampil/sembunyi | `Ctrl+B` |
| Command palette | `Ctrl+Shift+P`, `F1` |
| Quick open folder | `Ctrl+P` |
| Panel Transfer | `Ctrl+J` |
| Settings | `Ctrl+,` |
| Fullscreen | `F11` |
| **Hot Switch: semua drag & drop** | `Ctrl+Shift+D` |
| **Hot Switch: drag file** | `Ctrl+K`, `F` |
| **Hot Switch: drag folder** | `Ctrl+K`, `D` |
| Hot Switch: drag dari sidebar & pinned | `Ctrl+K`, `S` |
| Hot Switch: drag ke/dari aplikasi lain | `Ctrl+K`, `E` |
| Hot Switch: preset anti salah drag | `Ctrl+K`, `G` |
| Hot Switch: kunci pinned | `Ctrl+K`, `L` |
| Hot Switch: file tersembunyi | `Ctrl+K`, `H` |
| Hot Switch: ekstensi file | `Ctrl+K`, `X` |
| Hot Switch: thumbnail | `Ctrl+K`, `T` |
| Hot Switch: mode baca-saja | `Ctrl+K`, `R` |
| Panel Hot Switch | `Ctrl+K`, `K` |
| Lompat folder di dialog Open/Save (Picker Bridge) | `Ctrl+G` |
| Token Inspector (dev mode) | `Ctrl+Shift+I` |
| Perf HUD (dev mode) | `Ctrl+Shift+F12` |
| Reset tema darurat | `Ctrl+Alt+Shift+T` |
| Kill switch Picker Takeover (global) | `Ctrl+Alt+Shift+F12` |

Catatan: `Space` di Explorer memilih item fokus; di Switch Explorer `Space` membuka Quick Look, dan
fungsi pilih dipindah ke `Ctrl+Space` (bisa dikembalikan lewat keymap).
