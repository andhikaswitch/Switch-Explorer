# Developer Mode, Feature Flag, Drag Guard & Hot Switch

## 1. Filosofi: every feature is a switch

Setiap perilaku yang bisa mengganggu atau mengubah kebiasaan pengguna adalah **switch**. Default-nya
meniru File Explorer (semua fitur aktif) supaya terasa familiar; pengguna lalu mematikan, mengunci, atau
memberi hotkey sesuai kebiasaannya. Fitur yang mengubah sistem (Win+E, dialog Open/Save) adalah
pengecualian: default mati dan harus diaktifkan secara sadar, demi keamanan.

## 2. Membuka Developer Mode

- Settings → Developer → "Aktifkan Developer Mode", atau klik nomor versi di halaman Tentang 7 kali,
  atau jalankan dengan `--dev`.
- Developer Mode hanya **menampilkan** switch tingkat lanjut dan alat developer. Ia tidak mengubah
  perilaku apa pun dengan sendirinya.

## 3. Settings = switch

Semua preferensi adalah entri registry dengan metadata:

| Metadata | Arti |
|---|---|
| `id` | Nama stabil bertitik, mis. `dnd.source.folders` (jangan diganti setelah rilis) |
| `type` | `bool`, `enum`, `u8/u16/u32`, `string`, `keys`, `list<path>` |
| `default` | Nilai bawaan |
| `visibility` | `basic` (Settings biasa), `advanced`, `developer` (hanya di Developer Mode) |
| `stability` | `stable`, `beta`, `experimental` (experimental butuh Developer Mode) |
| `scope` | `global` atau `location` (boleh dipakai di Location Rules) |
| `hotswitchable` | Bisa diberi hotkey Hot Switch (hanya `bool`/`enum`) |
| `restart` | Perlu restart agar berlaku |
| `depends_on` | Switch lain yang harus aktif, mis. semua `dnd.*` bergantung pada `dnd.enabled` |

Halaman Settings, halaman flag, panel Hot Switch, dan keymap editor digenerate dari metadata ini — menambah
switch baru tidak perlu membuat UI baru.

## 4. Registry flag (implementasi)

```rust
// sx-settings/src/flags.rs
define_flags! {
    /// Master switch semua drag & drop.
    dnd_enabled: bool = true,
        id = "dnd.enabled", group = Dnd, visibility = Basic,
        stability = Stable, scope = Location, hotswitchable;

    /// Drag folder dari mana pun (konten, pohon sidebar, pinned).
    dnd_source_folders: bool = true,
        id = "dnd.source.folders", group = Dnd, visibility = Basic,
        stability = Stable, scope = Location, hotswitchable, depends_on = ["dnd.enabled"];

    /// Abaikan drag selama sekian ms setelah scroll terakhir.
    dnd_scroll_cooldown_ms: u16 = 0,
        id = "dnd.scroll_cooldown_ms", group = Dnd, visibility = Advanced,
        stability = Stable, scope = Location, range = 0..=2000;

    integration_picker_takeover: bool = false,
        id = "integration.picker_takeover", group = Integration,
        visibility = Developer, stability = Experimental;
}
```

- Macro menghasilkan `struct Features` bertipe kuat, tabel `FLAG_META`, parse/serialize TOML, dan
  `Features::effective_for(&Path)` (menerapkan Location Rules).
- Runtime: `SettingsStore` menyimpan snapshot `Arc<Features>`; perubahan membuat snapshot baru (murah),
  lalu emit `SettingsChanged { ids }` (dan `HotSwitchFired` bila sumbernya hotkey).
- Lapisan nilai: default → `settings.toml` → profil/preset aktif → Location Rules → lapisan sesi
  (`--set`, Hot Switch non-persisten).
- Persistensi: hanya nilai yang berbeda dari default yang ditulis; key tak dikenal dipertahankan; ada
  `schema` versi + migrasi.

## 5. Drag Guard

### 5.1 Masalah
Di Explorer, menggeser kursor sedikit saja (ambang drag sistem kecil) sambil menekan item — misalnya saat
scroll dengan touchpad — sudah memulai drag. Melepasnya di atas folder lain di volume yang sama berarti
**memindahkan** folder tersebut. Item pinned di navigation pane paling sering jadi korban.

### 5.2 Switch drag & drop

| Flag | Tipe | Default | Hot Switch | Keterangan |
|---|---|---|---|---|
| `dnd.enabled` | bool | true | `Ctrl+Shift+D` | Master semua drag & drop, termasuk menyusun ulang pinned |
| `dnd.source.files` | bool | true | `Ctrl+K, F` | File boleh di-drag, dari mana pun |
| `dnd.source.folders` | bool | true | `Ctrl+K, D` | Folder boleh di-drag (konten, pohon sidebar, pinned) |
| `dnd.source.sidebar` | bool | true | `Ctrl+K, S` | Item sidebar (pinned & pohon) boleh di-drag keluar sidebar |
| `dnd.external_out` | bool | true | `Ctrl+K, E` (grup) | Drag ke aplikasi lain |
| `dnd.external_in` | bool | true | `Ctrl+K, E` (grup) | Terima drop dari aplikasi lain |
| `dnd.target.sidebar` | bool | true | — | Boleh drop ke item sidebar |
| `dnd.target.folders` | bool | true | — | Boleh drop ke folder di daftar |
| `dnd.threshold_px` | u16 | 0 (ikut sistem) | — | Jarak minimal (px pada skala 100%) sebelum drag dimulai |
| `dnd.hold_delay_ms` | u16 | 0 | — | Tombol harus ditahan selama ini sebelum drag bisa dimulai |
| `dnd.scroll_cooldown_ms` | u16 | 0 | — | Drag diabaikan selama ini setelah scroll terakhir |
| `dnd.require_modifier` | enum `none`/`alt`/`ctrl`/`shift` | none | — | Drag hanya jika modifier ditahan saat mulai |
| `dnd.right_drag_bypass` | bool | false | — | Drag tombol kanan tetap boleh walau sumbernya dimatikan (selalu memunculkan menu konfirmasi) |
| `dnd.default_action` | enum `system`/`copy`/`move`/`ask` | system | "Tanya saat drop" (pilihan `ask` ⇄ nilai sebelumnya, tanpa hotkey default) | Aksi drop tanpa modifier |
| `dnd.confirm_folder_move` | bool | false | — | Konfirmasi saat memindahkan folder lewat drag |
| `dnd.undo_toast` | bool | true | — | Toast "Urungkan" 10 detik setelah drop |
| `dnd.blocked_hint` | bool | true | — | Beri tahu saat drag diblokir switch |
| `dnd.spring_open` | bool | true | — | Hover di atas folder saat drag membuka folder itu |
| `dnd.spring_delay_ms` | u16 | 800 | — | Jeda spring-open |
| `sidebar.pins_locked` | bool | false | `Ctrl+K, L` | Urutan & isi pinned dikunci |

### 5.3 Aturan efektif

```text
boleh_mulai_drag(item, asal, tombol) =
     dnd.enabled
  && (item.folder ? dnd.source.folders : dnd.source.files)
  && (asal == Sidebar ? dnd.source.sidebar : true)
  || (tombol == Kanan && dnd.right_drag_bypass)          // bypass tetap memunculkan menu saat drop

seleksi campuran (file + folder): semua item harus lolos; jika satu gagal → seluruh drag ditolak + hint.
boleh_drop(target)  = (target.sidebar ? dnd.target.sidebar : dnd.target.folders) && !safety.readonly_mode
boleh_keluar        = dnd.external_out      boleh_masuk = dnd.external_in
menyusun ulang pinned = dnd.enabled && !sidebar.pins_locked   (tidak memindahkan file)
pindah item sidebar = hanya lewat "Pindahkan ke sini" di menu drop, tidak pernah lewat drag biasa, dan tidak
                      saat sidebar.pins_locked; ke aplikasi lain item sidebar hanya bisa disalin/shortcut
pindah known folder (Desktop, Documents, Downloads, …) = tidak pernah (Windows tetap menunjuk tempat lamanya)
pin mengikuti foldernya saat folder itu dipindah atau di-rename (juga lewat undo)
```

### 5.4 State machine inisiasi drag

```text
Idle ──tekan tombol di item──▶ Pressed { t0, p0, item, tertahan = false }
Pressed:
  • !boleh_mulai_drag            → hanya klik/seleksi; drag tidak akan dimulai (hint bila mencoba)
  • now − scroll_terakhir < dnd.scroll_cooldown_ms → tertahan = true (press ini tidak bisa jadi drag)
  • dnd.require_modifier != none && modifier tidak ditahan → tertahan = true
  • gerak: jarak ≥ ambang && now − t0 ≥ dnd.hold_delay_ms && !tertahan → Dragging
  • gerak kecil sebelum hold delay selesai → belum apa-apa; gerak pertama setelahnya (masih lewat ambang)
    memulai drag. Lepas sebelum delay selesai tetap klik.
  • gerak ≥ 3× ambang sebelum hold delay selesai (flick, mis. geser touchpad sambil menekan) → tertahan
    = true
  • lepas tombol → Click
Dragging (internal atau OLE):
  • efek = modifier (Ctrl salin, Shift pindah, Alt shortcut) atau dnd.default_action
  • target tidak diizinkan → efek NONE + border drop_blocked
  • drop: ask → menu (Salin di sini / Pindahkan ke sini / Buat shortcut / Batal)
  • pindah folder && dnd.confirm_folder_move → sheet konfirmasi
  • selesai → toast Urungkan (dnd.undo_toast)
```

`scroll_terakhir` dicatat dari semua event scroll (wheel, touchpad pan, scrollbar) di window mana pun.

Implementasi: `sx_core::drag` (logika murni, waktu dan posisi diberikan pemanggil). `DragPolicy` diisi dari
snapshot switch; `DragPolicy::may_start`/`may_drop`/`effect` = aturan §5.3; `DragGuard` = state machine di
atas. Policy saat tombol ditekan berlaku sampai drag selesai, jadi Hot Switch tidak memutus drag yang
sedang berjalan.

Sambungan (`sx_ui::dnd`):
- Tekan tombol di baris daftar atau item sidebar (pin & drive) memanggil `DragGuard::press`. Gerak dan lepas
  dibaca listener tingkat window (capture), jadi tetap sampai walau kursor sudah keluar dari item. Scroll di
  mana pun mencatat `scroll_terakhir`.
- `StartDrag` menjalankan OLE drag lewat modal host (ADR-015). Drop ke window ini sendiri juga lewat drop
  target OLE milik kita (ADR-016), jadi aturan drop sama untuk drag internal maupun dari aplikasi lain.
- Drop target menjawab seketika dari zona frame terakhir (baris folder, area folder terbuka, item sidebar)
  dan `DragPolicy`. Pindah ke folder asal tidak melakukan apa pun. Masuk ke dirinya sendiri atau subfolder-nya
  ditolak. Sumber yang tidak mengizinkan pindah mendapat salin (kecuali modifier meminta pindah).
- `Ask` membuka menu bertema (Salin di sini / Pindahkan ke sini / Batal), hanya dengan efek yang diizinkan
  sumber; memilih di menu sudah menjadi konfirmasi. `ConfirmMove` membuka dialog bila ada folder di
  antara item.
- Drag milik kita memakai switch saat tombol ditekan sampai selesai. Pin yang di-drag ke item sidebar
  ditolak (memindahkan folder pinned tidak boleh terjadi; menyusun ulang pinned lewat drag menyusul).
  Drive atau share utuh tidak pernah disalin/dipindah. Selama dialog, palette, menu, atau rename terbuka,
  drop ditolak.
- Drag kiri yang dilepas di folder asalnya tidak melakukan apa pun, juga dengan `ask` (seperti Explorer);
  drag kanan di sana tetap memunculkan menu dengan "Salin di sini". Menu hanya berisi pilihan yang bisa
  dijalankan.
- Belum: shortcut (`Alt`, `Ctrl+Shift`) ditolak dulu, spring-open, menyusun ulang pinned lewat drag, drop
  ke breadcrumb/tab, dan virtual files.

### 5.5 Preset "Anti salah drag"

| Flag | Nilai preset |
|---|---|
| `dnd.source.sidebar` | false |
| `dnd.threshold_px` | 14 |
| `dnd.hold_delay_ms` | 150 |
| `dnd.scroll_cooldown_ms` | 400 |
| `dnd.default_action` | ask |
| `dnd.confirm_folder_move` | true |
| `dnd.spring_open` | false |
| `dnd.right_drag_bypass` | true |
| `sidebar.pins_locked` | true |

Preset ini juga bisa di-toggle lewat Hot Switch `Ctrl+K, G`: menyalakan menyimpan nilai sebelumnya di
`settings.toml` (`[presets.saved_before.anti_drag]`), mematikan mengembalikannya persis.

## 6. Hot Switch

### 6.1 Konsep
Hot Switch = hotkey yang membalik switch secara instan, **selalu** disertai umpan balik: OSD (pop-up
kecil yang tidak mengambil fokus), chip status di status bar, dan badge title bar saat drag mati total.
Desain visual di `DESIGN.md` §9.

### 6.2 Binding default

| Hotkey | Switch | Jenis |
|---|---|---|
| `Ctrl+Shift+D` | `dnd.enabled` (semua drag & drop) | toggle, boleh global |
| `Ctrl+K`, `F` | `dnd.source.files` (drag file) | toggle |
| `Ctrl+K`, `D` | `dnd.source.folders` (drag folder) | toggle |
| `Ctrl+K`, `S` | `dnd.source.sidebar` | toggle |
| `Ctrl+K`, `E` | `dnd.external_out` + `dnd.external_in` | grup |
| `Ctrl+K`, `G` | preset "Anti salah drag" | preset |
| (tanpa hotkey) | "Tanya saat drop": `dnd.default_action` = `ask`, atau kembali ke nilai sebelumnya | pilihan |
| `Ctrl+K`, `L` | `sidebar.pins_locked` | toggle |
| `Ctrl+K`, `H` | `view.show_hidden` | toggle |
| `Ctrl+K`, `X` | `view.show_extensions` | toggle |
| `Ctrl+K`, `T` | `view.thumbnails` | toggle |
| `Ctrl+K`, `R` | `safety.readonly_mode` | toggle |
| `Ctrl+K`, `K` | buka panel Hot Switch | aksi |

Jenis aksi:
- **toggle**: bool dibalik.
- **cycle**: enum maju ke nilai berikutnya (mis. `hotswitch.osd`: in_app → system → off).
- **pilihan**: satu nilai enum ditampilkan sebagai on/off. Menyalakan menyetel nilai itu; mematikan
  mengembalikan nilai sebelumnya (sesi ini), atau default bila tidak diketahui. Contoh: "Tanya saat drop".
- **grup**: beberapa switch disetel ke nilai yang sama — jika ada yang aktif, semua dimatikan; jika semua
  mati, semua dinyalakan.
- **preset**: menyalakan/mematikan preset dengan menyimpan & mengembalikan nilai sebelumnya.

### 6.3 Switch Hot Switch

| Flag | Tipe | Default | Keterangan |
|---|---|---|---|
| `hotswitch.enabled` | bool | true | Semua hotkey Hot Switch aktif |
| `hotswitch.persist` | bool | true | Perubahan via hotkey disimpan; false = hanya sesi ini |
| `hotswitch.osd` | enum `in_app`/`system`/`off` | in_app | Cara menampilkan status (chip status tetap diperbarui walau `off`) |
| `hotswitch.osd_ms` | u16 | 1800 | Lama OSD tampil |
| `hotswitch.osd_position` | enum `bottom_center`/`top_center`/`near_cursor` | bottom_center | Posisi OSD |
| `hotswitch.status_chip` | bool | true | Chip "Drag: …" di status bar |
| `hotswitch.titlebar_badge` | bool | true | Badge "Drag mati" di title bar saat master mati |
| `hotswitch.chord_timeout_ms` | u16 | 1500 | Batas waktu tombol kedua setelah `Ctrl+K` |

Implementasi: `sx_ui::hotswitch`. Binding dibaca dari `sx_settings::DEFAULT_BINDINGS` (keymap.toml menyusul);
setiap perubahan lewat `SettingsStore::run_hotswitch` menghasilkan `Change` yang memperbarui snapshot, chip,
badge, dan OSD; `settings.toml` ditulis di thread `sx-settings`, berurutan.

### 6.4 Aturan perilaku
- Chord `Ctrl+K` hanya aktif saat fokus **tidak** di input teks (kotak cari, rename, address bar), agar
  tidak memakan karakter. Kombinasi tunggal seperti `Ctrl+Shift+D` tetap aktif di mana pun.
  Pengecualian: di kotak cari command palette, `Ctrl+K`, `K` membuka panel Hot Switch (pindah mode,
  DESIGN §8.9); chord lain tidak cocok di sana, sehingga ketikan diteruskan ke kotak cari.
- Drag yang sedang berlangsung tidak diputus; switch baru berlaku untuk drag berikutnya.
- Switch yang `depends_on` master: bila master mati, menyalakan sub-switch memunculkan OSD dengan catatan
  Tertahan karena "Semua drag" mati. (nama induk diambil dari `depends_on`). Mematikannya tidak memberi
  catatan. Panel Hot Switch menandai sub-switch yang menyala tapi tertahan dengan "Tertahan".
- Hotkey yang mengubah switch eksperimental ditolak bila Developer Mode mati (OSD menjelaskan).

### 6.5 Scope global
- Binding boleh diberi `scope = "global"`: didaftarkan dengan `RegisterHotKey` sehingga bekerja walau app
  tidak fokus (atau tanpa window saat resident mode).
- Hanya kombinasi tunggal (bukan chord). Kombinasi dengan tombol Windows tidak disarankan (banyak dicadangkan
  shell). Pendaftaran gagal (dipakai aplikasi lain) → ditampilkan di keymap editor.
- OSD untuk hotkey global tampil di monitor aktif sebagai window kecil topmost yang tidak mengambil fokus.

### 6.6 Format `keymap.toml`

```toml
# %APPDATA%\SwitchExplorer\keymap.toml — hanya berisi yang berbeda dari default
[[binding]]
keys = "ctrl-shift-d"
action = "switch.toggle"
args = { id = "dnd.enabled" }
scope = "global"             # app (default) | global

[[binding]]
keys = "ctrl-k o"            # ganti drag folder dari Ctrl+K, D ke Ctrl+K, O
action = "switch.toggle"
args = { id = "dnd.source.folders" }

[[binding]]
keys = "ctrl-alt-f9"
action = "switch.preset"
args = { id = "anti_drag" }
scope = "global"

[[binding]]
keys = "ctrl-k q"            # pilihan: "Tanya saat drop" dapat hotkey
action = "switch.choice"
args = { id = "dnd.default_action", value = "ask" }

[[unbind]]
keys = "ctrl-k h"
```

### 6.7 Konflik
Keymap editor memperingatkan bila: (1) bentrok dengan binding lain di konteks yang sama; (2) termasuk
shortcut sistem yang dicadangkan (`Ctrl+Alt+Del`, `Alt+Tab`, `Alt+F4`, `Ctrl+Shift+Esc`, `Win+…`);
(3) menimpa shortcut paritas Explorer (`Ctrl+C`, `F2`, dll.); (4) memakai `Ctrl+Alt+huruf` — kombinasi ini
sama dengan AltGr di banyak layout keyboard; binding seperti itu tidak dijalankan saat fokus di input teks
dan tombol menghasilkan karakter.

## 7. Tabel switch lengkap

Switch drag & drop dan Hot Switch ada di §5.2 dan §6.3.

**Navigasi & tab**

| Flag | Tipe | Default | Keterangan |
|---|---|---|---|
| `nav.single_click_open` | bool | false | Buka item dengan sekali klik |
| `nav.backspace_action` | enum back/up | back | Fungsi Backspace |
| `nav.middle_click_new_tab` | bool | true | Middle-click folder = tab baru |
| `tabs.enabled` | bool | true | Tab |
| `tabs.restore_session` | bool | true | Pulihkan tab saat start |
| `tabs.dedupe` | bool | false | Folder yang sudah terbuka → pindah ke tab itu |
| `split.enabled` | bool | true | Split view |
| `addressbar.shell_paths` | bool | true | Dukungan `shell:` & variabel lingkungan |
| `addressbar.autocomplete` | bool | true | Autocomplete path |
| `sidebar.tree` | bool | false | Mode pohon di sidebar |

**Tampilan**

| Flag | Tipe | Default | Hot Switch | Keterangan |
|---|---|---|---|---|
| `view.show_hidden` | bool | false | `Ctrl+K, H` | File tersembunyi |
| `view.show_system` | bool | false | — | File sistem terlindungi |
| `view.show_extensions` | bool | true | `Ctrl+K, X` | Ekstensi file |
| `view.folders_first` | bool | true | — | Folder di atas |
| `view.natural_sort` | bool | true | — | Urutan alami (file2 sebelum file10) |
| `view.checkboxes` | bool | false | — | Checkbox seleksi |
| `view.thumbnails` | bool | true | `Ctrl+K, T` | Thumbnail |
| `view.themed_folder_icons` | bool | true | — | Ikon folder bertema |
| `view.quick_look` | bool | true | — | Quick Look dengan Space |
| `view.folder_sizes` | bool | false | — | Hitung ukuran folder (beta, berat) |
| `view.relative_dates` | bool | false | — | "2 jam lalu" |
| `view.remember_per_folder` | bool | true | — | Ingat mode tampilan per folder |
| `view.cut_items_faded` | bool | true | — | Item cut pudar |
| `view.si_units` | bool | false | — | KB = 1000 |

**File & keselamatan**

| Flag | Tipe | Default | Hot Switch | Keterangan |
|---|---|---|---|---|
| `ops.delete_to_recycle_bin` | bool | true | — | Del ke Recycle Bin |
| `ops.confirm_delete` | bool | true | — | Konfirmasi hapus |
| `ops.confirm_permanent_delete` | bool | true | — | Hanya bisa dimatikan di Developer Mode |
| `ops.undo` | bool | true | — | Undo/redo |
| `ops.undo_depth` | u16 | 100 | — | Kedalaman undo |
| `ops.parallel_jobs` | u8 | 2 | — | Operasi paralel |
| `ops.verify_copy` | bool | false | — | Verifikasi hash setelah copy |
| `ops.taskbar_progress` | bool | true | — | Progress di taskbar |
| `ops.progress_popup` | enum inline/floating/both | inline | — | Tampilan progress |
| `safety.readonly_mode` | bool | false | `Ctrl+K, R` | Blokir semua operasi tulis |
| `safety.protected_paths` | list<path> | [] | — | Operasi tulis di path ini butuh konfirmasi |
| `safety.confirm_system_paths` | bool | true | — | Konfirmasi ekstra di `C:\Windows`, `Program Files` |

**Pencarian**

| Flag | Tipe | Default | Keterangan |
|---|---|---|---|
| `search.everything` | enum auto/off | auto | Pakai Everything bila berjalan |
| `search.recursive_default` | bool | false | Kotak cari langsung rekursif |
| `search.fuzzy` | bool | true | Fuzzy di palette & quick open |
| `search.content` | bool | false | Cari isi file (beta) |

**Integrasi sistem** (semua default mati; diaktifkan lewat onboarding/Settings)

| Flag | Tipe | Default | Stabilitas | Keterangan |
|---|---|---|---|---|
| `integration.default_folder_handler` | bool | false | stable | Buka folder dari aplikasi lain di Switch Explorer |
| `integration.win_e` | bool | false | stable | Win+E membuka Switch Explorer |
| `integration.explorer_redirect` | bool | false | experimental | Alihkan window Explorer yang tetap terbuka |
| `integration.explorer_menu_entry` | bool | false | stable | "Buka di Switch Explorer" di menu klik kanan Explorer |
| `integration.sendto_drive` | bool | false | stable | Entri Send To → Google Drive |
| `integration.resident` | bool | false | stable | Tetap berjalan di background |
| `integration.global_open_hotkey` | keys | "" | stable | Hotkey global membuka window |
| `integration.picker_bridge` | bool | false | beta | Panel di samping dialog Open/Save |
| `integration.picker_bridge_hotkey` | keys | "ctrl-g" | beta | Lompat folder di dialog |
| `integration.picker_takeover` | bool | false | experimental | Ganti dialog Open/Save (Developer Mode) |
| `integration.native_menu_dark` | bool | true | stable | Menu native ikut dark (API tak terdokumentasi) |

**Cloud**

| Flag | Tipe | Default | Keterangan |
|---|---|---|---|
| `cloud.enabled` | bool | true | Modul tersedia (tetap lazy) |
| `cloud.gdrive.context_menu` | bool | true | Menu Upload/Pindahkan ke Drive |
| `cloud.gdrive.browse` | bool | true | Drive di sidebar setelah terhubung |
| `cloud.gdrive.verify_before_delete` | bool | true | Hanya bisa dimatikan di Developer Mode |
| `cloud.transfer.concurrency` | u8 | 3 | Transfer paralel |
| `cloud.transfer.chunk_mib` | u16 | 8 | Ukuran chunk (kelipatan 256 KiB) |
| `cloud.transfer.bandwidth_kbps` | u32 | 0 | Batas bandwidth (0 = tanpa batas) |
| `cloud.runtime_idle_shutdown_s` | u32 | 300 | Matikan runtime jaringan setelah idle |

**Modul, performa, developer**

| Flag | Tipe | Default | Keterangan |
|---|---|---|---|
| `home.enabled` | bool | true | Halaman Home |
| `tools.organizer` / `tools.duplicates` / `tools.bulk_rename` / `tools.space_analyzer` | bool | true | Tools (lazy) |
| `tools.sync` | bool | false | Sync (experimental) |
| `tags.enabled` | bool | false | Tag (beta) |
| `module.preview_handlers` | bool | true | Preview handler sistem (Office/PDF) |
| `perf.thumbnail_cache_mb` | u16 | 64 | Budget cache thumbnail |
| `perf.icon_cache_entries` | u16 | 2048 | Budget cache ikon |
| `perf.dir_cache_count` | u8 | 8 | Snapshot folder yang disimpan |
| `perf.shell_workers` | u8 | 2 | Worker STA (restart) |
| `perf.watcher` | bool | true | Pantau perubahan folder |
| `perf.trim_on_minimize` | bool | true | Buang cache saat minimize lama |
| `perf.solid_on_battery_saver` | bool | true | Material solid saat hemat baterai |
| `perf.animations` | enum system/on/off | system | Animasi |
| `dev.mode` | bool | false | Developer Mode |
| `dev.perf_hud` | bool | false | Perf HUD |
| `dev.memory_meter` | bool | false | RAM per modul |
| `dev.token_inspector` | bool | false | Token Inspector |
| `dev.theme_hot_reload` | bool | false | Hot reload tema (otomatis aktif di Developer Mode) |
| `dev.log_level` | enum | info | error/warn/info/debug/trace |
| `dev.slow_frame_warn_ms` | u16 | 24 | Peringatan frame lambat |
| `dev.show_layout_bounds` | bool | false | Garis batas layout |

## 8. Preset/profil

| Preset | Isi |
|---|---|
| Seperti Explorer (default) | Semua nilai default |
| Anti salah drag | §5.5 |
| Hemat RAM | `view.thumbnails=false`, `perf.thumbnail_cache_mb=16`, `perf.icon_cache_entries=1024`, `perf.dir_cache_count=2`, `perf.shell_workers=1`, `module.preview_handlers=false`, `cloud.gdrive.browse=false` |
| Power user | `view.show_hidden=true`, `tabs.dedupe=true`, `view.relative_dates=true`, `sidebar.tree=true` |
| Baca saja | `safety.readonly_mode=true`, `dnd.enabled=false` |

Preset bisa diterapkan dari Settings, palette, `--profile`, atau Hot Switch (jenis preset). Profil buatan
user bisa disimpan, diekspor, dan diimpor.

## 9. Location Rules

```toml
# settings.toml — aturan per lokasi; yang paling spesifik menang
[[rules]]
path = 'D:\Kuliah\**'
"dnd.source.folders" = false
"dnd.confirm_folder_move" = true

[[rules]]
path = 'C:\Projects\**'
"safety.readonly_mode" = true
```

Hanya flag ber-`scope = location` yang boleh dipakai. Status bar menampilkan ikon kecil saat aturan lokasi
aktif, dengan tooltip aturannya.

## 10. Alat developer

| Alat | Isi |
|---|---|
| Perf HUD (`Ctrl+Shift+F12`) | FPS, frame time p50/p99, private bytes, working set, handle, thread, antrean task |
| Memory meter | Perkiraan RAM per modul: cache ikon, cache thumbnail, snapshot folder, cloud, tools, font/atlas |
| Token Inspector (`Ctrl+Shift+I`) | Lihat & edit token elemen di bawah kursor (THEMING.md §13) |
| Log viewer | Tail log dengan filter level; tombol buka folder log |
| Config | Buka `settings.toml`/`keymap.toml` di editor, validasi, muat ulang |
| Integrasi | Status tiap integrasi registry + tombol Perbaiki/Batalkan registrasi |
| Diagnostik picker | 20 permintaan picker terakhir + alasan fallback |

## 11. Argumen CLI

```text
switch-explorer.exe [PATH ...]
  --new-window             buka di window baru (default: tab baru di window aktif)
  --select <PATH>          buka folder induk dan pilih item
  --home                   buka Home (dipakai Win+E)
  --safe-mode              tema default, integrasi & fitur eksperimental mati, log verbose
  --reset-theme            kembalikan tema ke Cerulean dan hapus override sesi
  --profile <NAMA>         pakai preset/profil untuk sesi ini
  --set <id>=<nilai>       override switch untuk sesi ini (boleh berulang)
  --dev                    Developer Mode untuk sesi ini
  --resident               jalan di background tanpa window
  --picker-host            mode host picker (dipanggil shim)
  --upload-to-drive <PATH...>   buka popup tujuan Drive (dipakai Send To)
  --register <fitur>       default-folder | win-e | explorer-menu | sendto-drive | picker-takeover
  --unregister <fitur|all>
  --picker-takeover off    matikan takeover seketika (kill switch)
  --bench-open <PATH>      cetak timestamp startup/listing lalu keluar
```

Menahan `Shift` saat aplikasi dibuka menawarkan safe mode.

## 12. Menambah switch baru

1. Tambahkan entri di `define_flags!` (id, tipe, default, visibility, stability, scope, hotswitchable).
2. Tambahkan string label & deskripsi di `locales/en.yml` dan `locales/id.yml`.
3. Baca flag lewat snapshot (`cx.features()`), bukan dari file.
4. Bila `hotswitchable`: pastikan teks OSD jelas (nama switch + dampak); tambah binding default hanya jika
   benar-benar sering dipakai.
5. Perbarui tabel di dokumen ini + test (default, parse, efek perilaku).
Skill `/add-feature-flag` menjalankan checklist ini.
