# Mesin Tema & Personalisasi

Dokumen ini menjelaskan bagaimana warna dan tampilan Switch Explorer bisa diatur "sefleksibel distro
Linux", tanpa membuat UI rusak atau tidak terbaca, dan tetap mengikuti light/dark Windows.

## 1. Seberapa bebas?

Tidak dibatasi, tapi **bertingkat** (progressive disclosure):

| Level | Untuk siapa | Yang bisa diatur |
|---|---|---|
| **L1 — Cepat** (Settings → Tampilan) | Semua orang | Mode (ikut sistem/light/dark/jadwal), aksen (tema/Windows/kustom/wallpaper), material, intensitas transparansi, densitas, skala font, gambar latar |
| **L2 — Theme Studio** (tab Token) | Yang ingin detail | **Setiap state setiap komponen** sebagai token, mis. hanya warna hover tombol primer (`button.primary.bg_hover`) |
| **L3 — File tema** | Ricer & pembuat tema | File TOML dengan referensi & fungsi warna, hot reload, bisa dibagikan |

Ganti aksen di L1 → seluruh palet digenerate ulang dan semua token yang merujuk palet ikut berubah
harmonis. Ubah satu token di L2 → hanya token itu yang berubah. Token Inspector (§13) memudahkan
menemukan token dari elemen yang terlihat di layar.

## 2. Tiga lapisan token

```text
Palet (pal.*)        → digenerate dari seed aksen & netral (OKLCH)    contoh: pal.accent.500
   ▼ dirujuk oleh
Semantik             → makna umum                                       contoh: hover.bg, fg.primary
   ▼ dirujuk oleh
Komponen             → state komponen tertentu                          contoh: button.primary.bg_hover
```

Komponen merujuk semantik, semantik merujuk palet. Override bisa dilakukan di lapisan mana pun.

## 3. Daftar token

| Grup | Token | Tipe |
|---|---|---|
| Latar | `bg.window`, `bg.titlebar`, `bg.sidebar`, `bg.content`, `bg.statusbar` | Paint |
| Permukaan | `surface.card`, `surface.card_hover`, `surface.overlay`, `surface.input`, `surface.chip`, `scrim` | Paint |
| Teks | `fg.primary`, `fg.secondary`, `fg.muted`, `fg.disabled`, `fg.on_highlight`, `fg.on_solid`, `fg.link` | Color |
| Garis | `border.subtle`, `border.strong`, `border.input`, `border.input_focus`, `border.input_hover` (kotak teks disorot), `divider` | Color |
| Aksen (bidang) | `accent.highlight`, `accent.highlight_hover`, `accent.highlight_pressed`, `accent.solid`, `accent.solid_hover`, `accent.solid_pressed` | Paint |
| Aksen (garis/teks) | `accent.text`, `accent.glow`, `accent.edge` | Color |
| State | `hover.bg`, `pressed.bg`, `selection.bg`, `selection.border`, `selection.inactive_bg`, `focus.ring`, `drop_target.bg`, `drop_target.border`, `drop_blocked.border` | Color |
| Navigasi | `nav.item.fg`, `nav.item.icon`, `nav.item.hover.bg`*, `nav.item.hover.fg`, `nav.item.pressed.bg`*, `nav.item.active.bg`*, `nav.item.active.fg`, `nav.item.active.edge`, `nav.item.active.glow`, `nav.section.fg` | Color (*Paint) |
| Tab | `tab.fg`, `tab.hover.bg`, `tab.active.bg`*, `tab.active.fg`, `tab.active.indicator`* | Color (*Paint) |
| Breadcrumb | `breadcrumb.fg`, `breadcrumb.current.fg`, `breadcrumb.hover.bg`, `breadcrumb.separator` | Color |
| Daftar & grid | `header.fg`, `header.hover.bg`, `row.hover.bg`, `row.selected.bg`, `row.selected.border`, `row.selected_inactive.bg`, `tile.hover.bg`, `tile.selected.bg`, `match.fg` (bagian nama yang cocok dengan filter/pencarian, tebal) | Color |
| Tombol | `button.primary.{bg,bg_hover,bg_pressed,fg}`, `button.secondary.{bg,bg_hover,bg_pressed,border,fg,fg_hover}`, `button.ghost.{bg_hover,bg_pressed,fg,fg_hover}`, `button.danger.{bg,bg_hover,fg}` | Paint/Color |
| Kartu | `surface.card_hover`* (latar), `card.hover.fg` | Paint/Color |
| Kontrol | `toggle.on.bg`, `toggle.on.thumb`, `toggle.off.border`, `toggle.off.thumb`, `checkbox.checked.bg`, `checkbox.check`, `checkbox.border`, `slider.fill`, `slider.track`, `input.placeholder` | Paint/Color |
| Umpan balik | `success.*`, `warning.*`, `danger.*`, `info.*` (`solid`, `text`), `toast.bg`*, `toast.fg`, `progress.fill`*, `progress.track` | Color (*Paint) |
| Hot Switch | `osd.bg`*, `osd.fg`, `osd.border`, `osd.state_on`*, `osd.state_off`, `osd.icon_on`, `osd.icon_off`, `statuschip.on`, `statuschip.partial`, `statuschip.off` | Color (*Paint) |
| Gauge | `gauge.fill`, `gauge.fill_warning`, `gauge.fill_danger`, `gauge.track` | Paint/Color |
| Scrollbar | `scrollbar.thumb`, `scrollbar.thumb_hover`, `scrollbar.track` | Color |
| Kategori | `category.{documents,audio,pictures,videos,archives,code,apps,other}` | Color |
| Ikon | `icon.folder.back`, `icon.folder.front`, `icon.default` | Color |
| Bayangan | `shadow.card`, `shadow.popover`, `shadow.dialog`, `shadow.glow` | Shadow |
| Angka | `radius.*`, `space.*`, `row.height.*`, `row.cut.opacity`, `slash.angle_deg`, `slash.edge_width`, `motion.*` | Number |
| Tipografi | `font.size.{caption,body,subtitle,title,display}`, `font.line.{…}` (px, DESIGN §4) | Number |
| Chrome & kontrol | `size.{titlebar,commandbar,statusbar,sidebar,tab_max,tab_indicator,control,icon,icon_sm}`, `size.osd_{height,min_width,max_width,offset}` (OSD Hot Switch, DESIGN §9.1), `size.search_width` (kotak cari, DESIGN §8.1), `focus.ring_width`, `opacity.disabled` | Number |
| Font | `font.ui`, `font.display`, `font.mono` (daftar fallback), `font.scale` | Font/Number |

Setiap token dideklarasikan sekali di `sx-theme` lewat `define_tokens!` (id, tipe, grup, deskripsi i18n,
pasangan kontras). File tema hanya memberi nilai.

## 4. Sintaks nilai

```text
nilai    := warna | ref | fungsi | gradien | bayangan | angka | transparent
warna    := #RRGGBB | #RRGGBBAA | rgb(r, g, b) | rgba(r, g, b, a) | oklch(L C h [/ a])
ref      := {token.id} | {pal.accent.500} | {pal.neutral.900} | {pal.lagoon}
fungsi   := alpha(nilai, 0..1)                   di luar rentang → peringatan
          | mix(nilai, nilai, 0..100%)        campuran di ruang OKLab; tanda % wajib
          | lighten(nilai, n) | darken(nilai, n)       ubah L OKLCH sebesar n
          | saturate(nilai, n) | desaturate(nilai, n)  ubah chroma
          | shift_hue(nilai, derajat)
          | on(nilai)                          pilih Ink atau putih dengan kontras terbaik
gradien  := gradient(<sudut>deg, nilai, nilai[, nilai…])   hanya untuk token Paint
bayangan := shadow(x, y, blur, spread, nilai)              hanya untuk token Shadow
```

- Referensi di-resolve topologis; siklus → error yang menyebut rantai siklusnya; kedalaman maks 16.
- Gradien pada token bertipe Color → error. Token tak dikenal → error + saran nama terdekat.
- File tema maks 1 MB; tidak ada eksekusi kode, tidak ada pengambilan dari jaringan.

## 5. File tema — Cerulean (tema bawaan)

Isi ini identik dengan `themes/cerulean.toml` (file itu sumber kebenarannya; perbarui keduanya
bersamaan). Kunci bertitik **wajib** diberi tanda kutip agar TOML tidak membuatnya menjadi tabel bersarang.

Di light, indikator status (`toggle.on.bg`, `slider.fill`, `progress.fill`, `tab.active.indicator`)
memakai `{accent.solid}` karena gradien lagoon hanya ~1.5:1 di atas latar hampir putih; di dark gradien
highlight tetap dipakai. Chip status light memakai `pal.accent.800` dan amber `#92400E` karena
`surface.chip` menggelapkan latar status bar. `match.fg` light juga `pal.accent.800`: `accent.text`
(`pal.accent.700`) hanya ~4.3:1 di atas baris terpilih.

```toml
[meta]
id = "cerulean"
name = "Cerulean"
author = "Switch Explorer"
version = "1.0.0"
schema = 1
variants = ["light", "dark"]
description = "Biru muda modern, terinspirasi highlight menu Persona 3 Reload."

[palette]
accent = "#2FA8FF"     # seed → pal.accent.50 … pal.accent.950
neutral = "#5B6B80"    # seed → pal.neutral.50 … pal.neutral.950

[palette.pin]          # nilai tetap untuk seed bawaan; diabaikan jika user mengganti aksen
"accent.300" = "#8FD0FF"
"accent.400" = "#5CBBFF"
"accent.500" = "#2FA8FF"
"accent.700" = "#1474D0"
"accent.800" = "#0B55A8"
"lagoon" = "#5CE1FF"

[material]
backdrop = "mica_alt"  # mica | mica_alt | acrylic | solid
tint_strength = 1.0
noise = 0.02

[fonts]
ui = ["Segoe UI Variable Text", "Segoe UI"]
display = ["Saira Semi Condensed", "Segoe UI Variable Display", "Segoe UI"]
mono = ["Cascadia Mono", "Consolas"]

[numbers]
"radius.xs" = 4
"radius.sm" = 6
"radius.md" = 8
"radius.lg" = 12
"radius.xl" = 16
"space.xxs" = 2
"space.xs" = 4
"space.sm" = 6
"space.md" = 8
"space.lg" = 12
"space.xl" = 16
"space.2xl" = 24
"space.3xl" = 32
"row.height.compact" = 28
"row.height.default" = 32
"row.height.comfortable" = 36
"row.cut.opacity" = 0.5
"slash.angle_deg" = 12
"slash.edge_width" = 3
"motion.fast" = 100
"motion.base" = 160
"motion.slow" = 240
"motion.slide" = 280
"font.size.caption" = 12
"font.size.body" = 14
"font.size.subtitle" = 16
"font.size.title" = 20
"font.size.display" = 28
"font.line.caption" = 16
"font.line.body" = 20
"font.line.subtitle" = 22
"font.line.title" = 26
"font.line.display" = 34
"size.titlebar" = 40
"size.commandbar" = 44
"size.statusbar" = 28
"size.sidebar" = 248
"size.tab_max" = 220
"size.tab_indicator" = 3
"size.control" = 32
"size.icon" = 20
"size.icon_sm" = 16
"size.osd_height" = 56
"size.osd_min_width" = 280
"size.osd_max_width" = 420
"size.osd_offset" = 72
"size.search_width" = 240
"focus.ring_width" = 2
"opacity.disabled" = 0.4

[common]               # berlaku di kedua mode
"fg.on_highlight" = "#0A1A2F"
"fg.on_solid" = "#FFFFFF"
"accent.highlight" = "gradient(120deg, {pal.lagoon}, {pal.accent.500})"
"accent.highlight_hover" = "gradient(120deg, lighten({pal.lagoon}, 0.04), lighten({pal.accent.500}, 0.04))"
"accent.highlight_pressed" = "gradient(120deg, darken({pal.lagoon}, 0.04), darken({pal.accent.500}, 0.05))"
"accent.solid" = "gradient(135deg, {pal.accent.700}, {pal.accent.800})"
"accent.solid_hover" = "gradient(135deg, {pal.accent.700}, lighten({pal.accent.800}, 0.04))"
"accent.solid_pressed" = "gradient(135deg, darken({pal.accent.700}, 0.05), darken({pal.accent.800}, 0.05))"
"nav.item.fg" = "{fg.secondary}"
"nav.item.icon" = "{fg.secondary}"
"nav.item.hover.bg" = "{accent.highlight_hover}"
"nav.item.hover.fg" = "{fg.on_highlight}"
"nav.item.pressed.bg" = "{accent.highlight_pressed}"
"nav.item.active.bg" = "{accent.highlight}"
"nav.item.active.fg" = "{fg.on_highlight}"
"nav.item.active.edge" = "{accent.edge}"
"nav.item.active.glow" = "{accent.glow}"
"nav.section.fg" = "{fg.muted}"
"tab.fg" = "{fg.secondary}"
"tab.hover.bg" = "{hover.bg}"
"tab.active.bg" = "{bg.content}"
"tab.active.fg" = "{fg.primary}"
"tab.active.indicator" = "{accent.highlight}"
"breadcrumb.fg" = "{fg.secondary}"
"breadcrumb.current.fg" = "{fg.primary}"
"breadcrumb.hover.bg" = "{hover.bg}"
"breadcrumb.separator" = "{fg.muted}"
"header.fg" = "{fg.muted}"
"header.hover.bg" = "{hover.bg}"
"row.hover.bg" = "{hover.bg}"
"row.selected.bg" = "{selection.bg}"
"row.selected.border" = "{selection.border}"
"row.selected_inactive.bg" = "{selection.inactive_bg}"
"match.fg" = "{accent.text}"
"tile.hover.bg" = "{hover.bg}"
"tile.selected.bg" = "{selection.bg}"
"button.primary.bg" = "{accent.highlight}"
"button.primary.bg_hover" = "{accent.highlight_hover}"
"button.primary.bg_pressed" = "{accent.highlight_pressed}"
"button.primary.fg" = "{fg.on_highlight}"
"button.secondary.bg" = "{surface.input}"
"button.secondary.border" = "{border.strong}"
"button.secondary.fg" = "{fg.primary}"
"button.secondary.bg_hover" = "{accent.highlight_hover}"
"button.secondary.bg_pressed" = "{accent.highlight_pressed}"
"button.secondary.fg_hover" = "{fg.on_highlight}"
"button.ghost.bg_hover" = "{accent.highlight_hover}"
"button.ghost.bg_pressed" = "{accent.highlight_pressed}"
"button.ghost.fg" = "{fg.primary}"
"button.ghost.fg_hover" = "{fg.on_highlight}"
"card.hover.fg" = "{fg.on_highlight}"
"button.danger.bg" = "#C42B1C"
"button.danger.bg_hover" = "#D13A2B"
"button.danger.fg" = "#FFFFFF"
"toggle.on.bg" = "{accent.highlight}"
"toggle.on.thumb" = "{fg.on_highlight}"
"toggle.off.border" = "{border.input}"
"toggle.off.thumb" = "{fg.secondary}"
"checkbox.checked.bg" = "{accent.solid}"
"checkbox.check" = "{fg.on_solid}"
"checkbox.border" = "{border.input}"
"slider.fill" = "{accent.highlight}"
"input.placeholder" = "{fg.muted}"
"progress.fill" = "{accent.highlight}"
"gauge.fill" = "{accent.highlight}"
"gauge.fill_warning" = "gradient(120deg, #FCD34D, #F59E0B)"
"gauge.fill_danger" = "gradient(120deg, #FF8A80, #E5484D)"
"toast.bg" = "{surface.overlay}"
"toast.fg" = "{fg.primary}"
"osd.bg" = "{surface.overlay}"
"osd.fg" = "{fg.primary}"
"osd.border" = "{border.subtle}"
"osd.state_on" = "{accent.highlight}"
"osd.icon_on" = "{fg.on_highlight}"
"osd.icon_off" = "{fg.secondary}"
"statuschip.on" = "{accent.text}"
"statuschip.partial" = "{warning.text}"
"statuschip.off" = "{fg.muted}"
"scrollbar.thumb_hover" = "{pal.accent.500}"
"scrollbar.track" = "transparent"
"info.solid" = "{pal.accent.500}"
"category.documents" = "#3B82F6"
"category.audio" = "#F43F5E"
"category.pictures" = "#F59E0B"
"category.videos" = "#8B5CF6"
"category.archives" = "#10B981"
"category.code" = "#06B6D4"
"category.apps" = "#64748B"
"category.other" = "#94A3B8"
"shadow.glow" = "shadow(0, 6, 18, 0, {accent.glow})"

[light]
"bg.window" = "alpha(#EAF5FF, 0.55)"
"bg.titlebar" = "alpha(#EAF5FF, 0.20)"
"bg.sidebar" = "alpha(#FFFFFF, 0.30)"
"bg.content" = "alpha(#FFFFFF, 0.84)"
"bg.statusbar" = "alpha(#FFFFFF, 0.55)"
"surface.card" = "alpha(#FFFFFF, 0.78)"
"surface.card_hover" = "{accent.highlight_hover}"
"surface.overlay" = "alpha(#FFFFFF, 0.96)"
"surface.input" = "alpha(#FFFFFF, 0.90)"
"surface.chip" = "alpha({pal.accent.500}, 0.10)"
"scrim" = "alpha(#0A1A2F, 0.32)"
"fg.primary" = "#0A1A2F"
"fg.secondary" = "#3E5068"
"fg.muted" = "#55687E"
"fg.disabled" = "#A3B1C2"
"fg.link" = "{pal.accent.700}"
"border.subtle" = "alpha({pal.accent.700}, 0.10)"
"border.strong" = "alpha({pal.accent.700}, 0.22)"
"border.input" = "#72869E"
"border.input_focus" = "{pal.accent.700}"
"border.input_hover" = "darken({border.input}, 0.14)"
"divider" = "alpha(#0A1A2F, 0.08)"
"accent.text" = "{pal.accent.700}"
"accent.glow" = "alpha({pal.accent.500}, 0.45)"
"accent.edge" = "{pal.accent.800}"
"hover.bg" = "alpha({pal.accent.700}, 0.06)"
"pressed.bg" = "alpha({pal.accent.700}, 0.10)"
"selection.bg" = "alpha({pal.accent.500}, 0.16)"
"selection.border" = "alpha({pal.accent.700}, 0.80)"
"selection.inactive_bg" = "alpha(#607389, 0.12)"
"focus.ring" = "{pal.accent.700}"
"drop_target.bg" = "alpha({pal.lagoon}, 0.18)"
"drop_target.border" = "{pal.accent.700}"
"drop_blocked.border" = "#B45309"
"slider.track" = "alpha(#607389, 0.30)"
"progress.track" = "alpha(#607389, 0.20)"
"gauge.track" = "alpha(#607389, 0.18)"
"osd.state_off" = "alpha(#607389, 0.35)"
"scrollbar.thumb" = "alpha(#607389, 0.45)"
"success.solid" = "#16A34A"
"success.text" = "#15803D"
"warning.solid" = "#D97706"
"warning.text" = "#B45309"
"danger.solid" = "#C42B1C"
"danger.text" = "#B42318"
"info.text" = "{pal.accent.700}"
"statuschip.on" = "{pal.accent.800}"
"match.fg" = "{pal.accent.800}"
"statuschip.partial" = "#92400E"
"tab.active.indicator" = "{accent.solid}"
"toggle.on.bg" = "{accent.solid}"
"toggle.on.thumb" = "{fg.on_solid}"
"slider.fill" = "{accent.solid}"
"progress.fill" = "{accent.solid}"
"icon.folder.back" = "#7CC8FF"
"icon.folder.front" = "{pal.accent.500}"
"icon.default" = "{fg.secondary}"
"shadow.card" = "shadow(0, 1, 2, 0, alpha(#0A1A2F, 0.06))"
"shadow.popover" = "shadow(0, 8, 24, 0, alpha(#0A1A2F, 0.16))"
"shadow.dialog" = "shadow(0, 24, 64, 0, alpha(#0A1A2F, 0.28))"

[dark]
"bg.window" = "alpha(#06101F, 0.60)"
"bg.titlebar" = "alpha(#06101F, 0.25)"
"bg.sidebar" = "alpha(#06101F, 0.35)"
"bg.content" = "alpha(#0A1628, 0.86)"
"bg.statusbar" = "alpha(#081322, 0.70)"
"surface.card" = "alpha(#0F1E35, 0.80)"
"surface.card_hover" = "{accent.highlight_hover}"
"surface.overlay" = "alpha(#0C1A2E, 0.96)"
"surface.input" = "alpha(#0A1628, 0.90)"
"surface.chip" = "alpha({pal.accent.400}, 0.14)"
"scrim" = "alpha(#000000, 0.50)"
"fg.primary" = "#EAF4FF"
"fg.secondary" = "#A9BCD3"
"fg.muted" = "#94A8C0"
"fg.disabled" = "#4A5B70"
"fg.link" = "{pal.accent.400}"
"border.subtle" = "alpha({pal.accent.400}, 0.12)"
"border.strong" = "alpha({pal.accent.400}, 0.24)"
"border.input" = "#5A7394"
"border.input_focus" = "{pal.lagoon}"
"border.input_hover" = "lighten({border.input}, 0.14)"
"divider" = "alpha(#EAF4FF, 0.08)"
"accent.text" = "{pal.accent.400}"
"accent.glow" = "alpha({pal.accent.500}, 0.55)"
"accent.edge" = "{pal.lagoon}"
"hover.bg" = "alpha({pal.accent.300}, 0.07)"
"pressed.bg" = "alpha({pal.accent.300}, 0.11)"
"selection.bg" = "alpha({pal.accent.500}, 0.22)"
"selection.border" = "alpha({pal.lagoon}, 0.80)"
"selection.inactive_bg" = "alpha(#A9BCD3, 0.12)"
"focus.ring" = "{pal.lagoon}"
"drop_target.bg" = "alpha({pal.lagoon}, 0.16)"
"drop_target.border" = "{pal.lagoon}"
"drop_blocked.border" = "#F59E0B"
"slider.track" = "alpha(#A9BCD3, 0.24)"
"progress.track" = "alpha(#A9BCD3, 0.16)"
"gauge.track" = "alpha(#A9BCD3, 0.14)"
"osd.state_off" = "alpha(#A9BCD3, 0.25)"
"scrollbar.thumb" = "alpha(#A9BCD3, 0.35)"
"success.solid" = "#22C55E"
"success.text" = "#4ADE80"
"warning.solid" = "#F59E0B"
"warning.text" = "#FBBF24"
"danger.solid" = "#E5484D"
"danger.text" = "#F97066"
"info.text" = "{pal.accent.400}"
"icon.folder.back" = "#3A9BEA"
"icon.folder.front" = "{pal.accent.400}"
"icon.default" = "{fg.secondary}"
"shadow.card" = "shadow(0, 1, 2, 0, alpha(#000000, 0.35))"
"shadow.popover" = "shadow(0, 8, 24, 0, alpha(#000000, 0.45))"
"shadow.dialog" = "shadow(0, 24, 64, 0, alpha(#000000, 0.55))"
```

## 6. File override user

Perubahan dari Theme Studio disimpan terpisah dari file tema, sehingga update tema tidak menghapus
kustomisasi (mirip `workbench.colorCustomizations` di VS Code).

```toml
# %APPDATA%\SwitchExplorer\theme-overrides.toml
schema = 1

[global.all]                 # semua tema, kedua mode
"focus.ring" = "#FFB020"

[cerulean.all]               # tema cerulean, kedua mode
"button.primary.bg_hover" = "#E5484D"

[cerulean.dark]              # tema cerulean, hanya dark
"bg.window" = "alpha(#020814, 0.72)"
```

## 7. Pengaturan tampilan (`settings.toml`)

```toml
[appearance]
theme = "cerulean"
mode = "system"              # system | light | dark | schedule
accent_source = "theme"      # theme | windows | custom | wallpaper
accent_custom = "#2FA8FF"
backdrop = "theme"           # theme | mica | mica_alt | acrylic | solid
tint_strength = 1.0          # 0.0 – 1.5, mengalikan alfa bg.window & bg.sidebar
density = "default"          # compact | default | comfortable
font_scale = "system"        # system | 0.9 | 1.0 | 1.1 | 1.25
display_font = true          # false → judul memakai font UI
radius_scale = 1.0           # 0.5 – 1.5, untuk ricing
background_image = ""        # path opsional
background_blur = 24
background_dim = 0.6

[appearance.schedule]
light_at = "06:00"
dark_at = "18:00"
```

## 8. Pipeline resolusi

1. Muat tema dasar (bawaan atau file user) → validasi schema.
2. Tentukan varian: `mode = system` → registry `AppsUseLightTheme`; high contrast aktif → tema
   `high-contrast` (mengabaikan tema user).
3. Tentukan seed aksen dari `accent_source` (§9).
4. Generate palet `pal.*`; `[palette.pin]` hanya dipakai bila sumber aksen = tema.
5. Susun lapisan (yang belakang menimpa yang depan): default schema → `[common]` tema → `[light]`/`[dark]`
   tema → `global.all` → `global.<mode>` → `<tema>.all` → `<tema>.<mode>` → edit sesi Theme Studio.
6. Evaluasi ekspresi (topologis, cek siklus).
7. Penyesuaian lingkungan:
   - Material solid (Windows 10, "Transparency effects" Windows mati, `backdrop = solid`, hemat baterai
     dengan `perf.solid_on_battery_saver`) → token latar beralfa dikomposit menjadi opak di atas warna
     dasar varian.
   - `tint_strength`, `font_scale`/text scale Windows, `radius_scale`, reduce motion (durasi → 0).
8. Validasi kontras (§11) → peringatan + perbaikan otomatis opsional.
9. Hasil: `ResolvedTheme` (array datar terindeks `Token`) → dipublikasikan ke ThemeStore → bridge
   gpui-component → semua window refresh.

## 9. Sumber aksen & generator palet

| Sumber | Asal warna | Diperbarui saat |
|---|---|---|
| `theme` | `[palette].accent` + `[palette.pin]` | Tema diganti |
| `windows` | Aksen Windows (`UISettings.GetColorValue(Accent)`) | Event `UISettings.ColorValuesChanged` |
| `custom` | Color picker (hex/OKLCH) | User mengubah |
| `wallpaper` | Warna dominan wallpaper: decode WIC 64×64 → k-means (k=5) di OKLab → klaster paling berwarna dengan populasi cukup | `WM_SETTINGCHANGE` `SPI_SETDESKWALLPAPER` |

Generator (OKLCH, gamut-mapped ke sRGB dengan menurunkan chroma):

| Step | L target | Faktor chroma |
|---|---|---|
| 50 / 100 / 200 | 0.975 / 0.95 / 0.90 | 0.15 / 0.25 / 0.45 |
| 300 / 400 | 0.84 / 0.77 | 0.70 / 0.90 |
| 500 | L seed, dijepit 0.68–0.74 | 1.00 |
| 600 / 700 / 800 | 0.62 / 0.55 / 0.47 | 0.95 / 0.85 / 0.75 |
| 900 / 950 | 0.38 / 0.28 | 0.60 / 0.45 |

Batasan otomatis:
- `700` harus ≥ 4.5:1 dengan latar terang tergelap `#F3F3F3` (base solid / fallback Mica); L diturunkan
  sampai lolos.
- `500` dan `lagoon` harus ≥ 5.5:1 dengan Ink; L dinaikkan sampai lolos. Angka 5.5:1 memberi ruang agar
  state pressed `darken(…, 0.05)` tetap ≥ 4.5:1.
- `lagoon` = seed dengan L +0.08, hue −12° (ke arah cyan), chroma ×0.9.

Palet netral digenerate dari seed netral dengan chroma rendah (0.02–0.03). Seed abu-abu (chroma < 0.01)
tetap abu-abu.

CI menyapu 36 aksen (12 hue × 3 kombinasi L/chroma) untuk setiap tema bawaan, di kedua mode, dan semuanya
harus lolos §11 (`crates/sx-theme/tests/builtin_themes.rs`).

## 10. Light/dark & adaptasi otomatis

- Setiap tema punya varian `light` dan `dark` (atau menyatakan hanya satu varian di `meta.variants`).
- Saat mengubah token, Theme Studio bertanya: **Mode ini saja** (hanya varian aktif), **Keduanya** (nilai
  sama di kedua varian), atau **Adaptasi otomatis**.
- Adaptasi otomatis: konversi ke OKLCH, `L' = clamp(1.04 − L, 0.18, 0.96)`, hue tetap, chroma ×0.9 bila
  `L' > 0.85` (menghindari neon di light), alfa tetap. Hasilnya disimpan sebagai nilai eksplisit di
  varian lain (tetap bisa diedit), lalu diperiksa kontrasnya.
- Mode `system` mengikuti Windows secara live; `schedule` berganti pada jam yang ditentukan.

## 11. Validasi & keselamatan

| Elemen depan | Latar (di atas lapisan) | Minimum |
|---|---|---|
| `fg.primary`, `fg.secondary`, `fg.muted` | `bg.content`, `surface.card`, `surface.overlay` | 4.5:1 |
| `fg.primary`, `fg.secondary`, `fg.muted` | `row.selected.bg`, `row.selected_inactive.bg` (di atas `bg.content`) | 4.5:1 |
| `header.fg` | `bg.content` | 4.5:1 |
| `match.fg` | `bg.content`; `row.hover.bg`, `row.selected.bg`, `row.selected_inactive.bg` (di atas `bg.content`) | 4.5:1 |
| `fg.primary`, `fg.secondary` | `bg.window` (command bar), `bg.statusbar` | 4.5:1 |
| `fg.primary`, `fg.secondary`, `fg.muted` | `bg.sidebar` | 4.5:1 |
| `nav.item.fg`, `nav.section.fg` | `bg.sidebar` | 4.5:1 |
| `nav.item.active.fg` | `nav.item.active.bg` (semua stop gradien, di atas `bg.sidebar`) | 4.5:1 |
| `tab.fg` | `bg.titlebar` | 4.5:1 |
| `tab.active.fg` | `tab.active.bg` | 4.5:1 |
| `breadcrumb.fg`, `breadcrumb.current.fg` | `bg.window` | 4.5:1 |
| `statuschip.on`, `…partial`, `…off` | `bg.statusbar`; `surface.chip` (di atas `bg.statusbar`) | 4.5:1 |
| `osd.fg` | `osd.bg` | 4.5:1 |
| `toast.fg` | `toast.bg` | 4.5:1 |
| `input.placeholder` | `surface.input` | 4.5:1 |
| `accent.text`, `fg.link`, `success.text`, `warning.text`, `danger.text`, `info.text` | `bg.content` | 4.5:1 |
| `button.primary.fg` | `button.primary.bg`, `…bg_hover`, `…bg_pressed` | 4.5:1 |
| `button.secondary.fg` | `button.secondary.bg` (di atas `bg.content`) | 4.5:1 |
| `button.secondary.fg_hover` | `button.secondary.bg_hover`, `…bg_pressed` (di atas `bg.content`) | 4.5:1 |
| `button.ghost.fg_hover` | `button.ghost.bg_hover`, `…bg_pressed` (di atas `bg.window`) | 4.5:1 |
| `nav.item.hover.fg` | `nav.item.hover.bg`, `nav.item.pressed.bg` (di atas `bg.sidebar`) | 4.5:1 |
| `card.hover.fg` | `surface.card_hover` (di atas `bg.content`) | 4.5:1 |
| `fg.on_solid` | `accent.solid` & variannya | 4.5:1 |
| `button.danger.fg` | `button.danger.bg`, `…bg_hover` | 4.5:1 |
| `focus.ring`, `border.input`, `border.input_focus`, `border.input_hover` | `bg.content`, `surface.input` | 3:1 |
| `border.input`, `border.input_focus`, `border.input_hover` | `bg.window` (address field & kotak cari di command bar) | 3:1 |
| `focus.ring` | `bg.sidebar`, `bg.titlebar`, `bg.window` | 3:1 |
| `focus.ring` | `surface.overlay`, `toast.bg`, `surface.card` (di atas `bg.content`) | 3:1 |
| `fg.secondary` | `osd.bg` (di atas `bg.content`) | 4.5:1 |
| `statuschip.off` | `bg.titlebar` (badge "Drag mati") | 4.5:1 |
| `checkbox.border`, `toggle.off.border`, `selection.border` | `bg.content` | 3:1 |
| `drop_target.border`, `drop_blocked.border` | `bg.content` | 3:1 |
| `slider.fill`, `progress.fill`, `toggle.on.bg` | `bg.content` | 3:1 |
| `tab.active.indicator` | `bg.titlebar` | 3:1 |
| `checkbox.check` | `checkbox.checked.bg` (di atas `bg.content`) | 3:1 |
| `toggle.on.thumb` | `toggle.on.bg` (di atas `bg.content`) | 3:1 |
| `osd.icon_on` / `osd.icon_off` | `osd.state_on` / `osd.state_off` (di atas `osd.bg`) | 3:1 |

- Latar transparan dikomposit di atas backdrop terburuk (light: `#FFFFFF` dan `#E9E9E9`; dark: `#000000`
  dan `#2B2B2B`), lalu lapisan di bawahnya (rata-rata OKLab bila gradien), lalu setiap stop latar dan
  setiap stop warna depan; nilai terendah yang dipakai. Teks 4.5:1 (WCAG 1.4.3), batas kontrol dan
  indikator status 3:1 (WCAG 1.4.11). `gauge.fill` tidak diuji karena kapasitas selalu juga tertulis
  sebagai teks.
- CI juga menjalankan tabel ini untuk sapuan 36 aksen di §9.
- Hasil: lulus atau peringatan (tidak memblokir). "Perbaiki otomatis" menggeser L warna depan seminimal
  mungkin sampai lulus.
- CI gagal jika ada tema **bawaan** yang tidak lulus.
- Nilai tidak valid → lapisan di bawahnya dicoba (override → varian tema → `[common]`) sampai nilai tema
  dasar, dengan satu peringatan per kegagalan. Token tidak pernah menjadi transparan karena satu nilai
  rusak. File gagal parse → tema terakhir yang valid tetap dipakai + toast berisi baris/kolom error.
- Material solid mengkomposit token `bg.*`/`surface.*` (kecuali `scrim`) di titik definisinya, sehingga
  token turunan seperti `osd.bg` ikut opak. Warna dasarnya `[material].solid_light`/`solid_dark`
  (default `#F3F3F3`/`#202020`).
- Reset darurat: `Ctrl+Alt+Shift+T` atau `--reset-theme`; `--safe-mode` memakai Cerulean tanpa override.

## 12. Hot reload

Watcher pada `themes\` dan `theme-overrides.toml` hanya aktif bila `dev.theme_hot_reload` (otomatis menyala
di Developer Mode). Debounce 150 ms; penerapan < 16 ms; error tidak pernah merusak UI.

## 13. Theme Studio & Token Inspector

**Theme Studio** (Settings → Tampilan → "Buka Theme Studio"):
- Tab **Cepat** (L1): mode, aksen + color picker (hex & slider OKLCH), material & intensitas, densitas,
  skala font, font display, radius scale, gambar latar (blur/dim).
- Tab **Token** (L2): pohon token per grup + pencarian; tiap token menampilkan swatch, ekspresi, status
  (bawaan/override), pilihan mode, panel kontras, reset per token/grup. Panel pratinjau berisi "spesimen"
  mini explorer dengan semua state (hover, pressed, selected, focus, disabled, drop target, OSD).
- Tab **File** (L3): daftar tema, duplikat sebagai tema baru, buka di editor teks, impor/ekspor
  `.sxtheme`, status hot reload & error, impor tema bawaan GPUI Kit (§14).

**Token Inspector** (`Ctrl+Shift+I`, butuh `dev.token_inspector` atau Developer Mode):
- Hover elemen → outline + label: nama komponen, token yang dipakai (bg/fg/border), nilai hasil.
- `Alt`+klik → berpindah antar token elemen itu; klik → membuka tab Token tepat di token tersebut; `Esc`
  keluar.
- Implementasi: saat inspector aktif, komponen mendaftarkan `(bounds, &[Token])` ke registry per frame
  lewat wrapper debug `.tokens(&[..])`. Saat tidak aktif, tidak ada pendaftaran (nol biaya).

## 14. Bridge ke gpui-component

Komponen gpui-component membaca `ThemeColor` mereka. Bridge di `sx-ui` mengisinya dari token kita setiap
tema berubah. Nama field di bawah adalah contoh — sesuaikan dengan `ThemeColor` versi terpin.

| `ThemeColor` | Token Switch Explorer |
|---|---|
| `background` / `foreground` | `bg.content` (komposit) / `fg.primary` |
| `border` | `border.subtle` |
| `primary`, `primary_hover`, `primary_active` | `button.primary.bg*` (gradien → warna rata-rata) |
| `primary_foreground` | `button.primary.fg` |
| `secondary`, `secondary_hover` | `button.secondary.bg`, `…bg_hover` |
| `accent` / `accent_foreground` | `hover.bg` / `fg.primary` |
| `muted` / `muted_foreground` | `surface.chip` / `fg.muted` |
| seleksi list/table | `row.selected.bg` |
| hover list/table | `row.hover.bg` |
| `sidebar` / `sidebar_foreground` | `bg.sidebar` / `nav.item.fg` |
| `title_bar` | `bg.titlebar` |
| `tab_active` | `tab.active.bg` |
| `ring` | `focus.ring` |
| `popover` / `popover_foreground` | `surface.overlay` / `fg.primary` |
| `scrollbar_thumb` | `scrollbar.thumb` |
| `danger`, `success`, `warning`, `info` | `*.solid` |

**Impor tema GPUI Kit:** GPUI Kit membawa puluhan tema (Catppuccin, Tokyo Night, Gruvbox, Solarized,
Everforest, Ayu, Flexoki, dll.). Pemetaan terbalik menghasilkan file `.toml` kita dari `ThemeColor` mereka,
sehingga pengguna langsung punya banyak pilihan untuk ricing. Tide Slash tetap memakai aksen dari
`primary` tema tersebut.

## 15. Performa

- `ResolvedTheme` berisi array datar terindeks `Token` (u16). `Paint` = `Solid(Rgba)` atau
  `Linear { angle, stops }`, dikonversi sekali ke tipe background GPUI.
- Resolusi ±150 token < 1 ms; ganti tema = satu kali refresh semua window; tidak ada parsing per frame.

## 16. Tema bawaan

| id | Varian | Karakter |
|---|---|---|
| `cerulean` | light + dark | Default. Biru muda, Tide Slash lagoon dengan teks navy |
| `abyss` | dark | Navy pekat + cyan neon, kontras tinggi, glow lebih kuat |
| `glacier` | light | Putih beku minimal, aksen tipis, nyaman untuk kerja lama |
| `high-contrast` | light + dark | Otomatis saat high contrast Windows aktif; warna sistem, material solid |
| Port palet populer | sesuai sumber | Catppuccin (Latte/Mocha), Nord, Tokyo Night, Gruvbox, Dracula — palet publik berlisensi MIT, dipetakan ke token kita |

## 17. Berbagi tema

- Format `.sxtheme` = file TOML tema (opsional: arsip zip berisi gambar latar jika user memilih
  menyertakannya).
- Impor memvalidasi schema, ukuran, dan kontras; tidak ada script.
- Metadata `author`, `version`, `description` ditampilkan di Theme Studio.

## 18. Rencana implementasi (`sx-theme`)

| Modul | Isi |
|---|---|
| `schema.rs` | `define_tokens!` → enum `Token`, metadata (id, tipe, grup, deskripsi, pasangan kontras) |
| `value.rs` | Parser & AST ekspresi nilai |
| `color.rs` | sRGB ↔ OKLab/OKLCH, gamut mapping, kontras WCAG |
| `palette.rs` | Generator palet dari seed + batasan kontras |
| `resolve.rs` | Penyusunan lapisan, evaluasi topologis, penyesuaian lingkungan |
| `adapt.rs` | Adaptasi otomatis light ↔ dark |
| `validate.rs` | Pasangan kontras, peringatan, perbaikan otomatis |
| `file.rs` | Baca/tulis TOML tema & override (atomic) |
| `watch.rs` | Hot reload (lazy) |
| `sx-ui/theme_bridge.rs` | Pengisian `ThemeColor` gpui-component |

Test: property test parser (proptest), snapshot `ResolvedTheme` Cerulean (insta), test kontras semua tema
bawaan, test adaptasi otomatis, test komposit material solid.
