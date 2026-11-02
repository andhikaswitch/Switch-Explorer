---
name: add-feature-flag
description: Menambah switch/feature flag baru ke registry sx-settings dengan metadata, i18n, dokumentasi, dan test. Dipakai setiap kali perilaku baru perlu bisa dinyalakan/dimatikan.
---
# Tambah switch baru

1. Tentukan: `id` bertitik (mis. `view.show_hidden`), tipe, default (meniru perilaku Explorer bila relevan),
   visibility, stability, scope (`global`/`location`), `hotswitchable` (hanya bool/enum), `restart`,
   `depends_on`. Integrasi yang mengubah sistem → default `false`.
2. Tambahkan entri di `define_flags!` (`crates/sx-settings/src/flags.rs`) dengan doc comment.
3. Tambahkan label & deskripsi di `locales/en.yml` dan `locales/id.yml`.
4. Pakai flag lewat snapshot (`cx.features()`); reaksi terhadap perubahan lewat `SettingsChanged`.
5. Jika `hotswitchable`: pastikan teks OSD jelas (nama switch + dampak). Tambahkan binding default di keymap
   bawaan **hanya** jika sering dipakai, dan perbarui tabel shortcut di `docs/FEATURES.md`.
6. Perbarui tabel di `docs/DEVELOPER-MODE.md` (§5.2, §6.3, atau §7).
7. Test: default, parse/serialize, efek perilaku saat on/off, Location Rules bila `scope = location`.
