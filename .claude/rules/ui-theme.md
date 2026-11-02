---
paths:
  - "crates/sx-ui/**"
  - "crates/sx-theme/**"
  - "themes/**"
---
# Aturan UI & tema

- Tidak ada warna, radius, spacing, durasi, atau shadow hardcode. Semua dari token (`docs/THEMING.md`).
  Token baru → skill `/add-theme-token`.
- Fungsi `render` murni dan murah: tanpa I/O, tanpa sort/filter besar, tanpa format string mahal per baris.
- Daftar & grid wajib tervirtualisasi.
- Komponen signature (nav sidebar/Tide Slash, tab, gauge, breadcrumb, OSD) di atas gpui-base; komponen
  kompleks dari gpui-component dengan warna lewat bridge tema.
- Setiap komponen interaktif punya state hover, pressed/aktif, fokus terlihat, dan disabled (DESIGN.md §11).
- Gerak memakai token `motion.*` dan menghormati reduce motion.
- Semua string lewat i18n (`en` + `id`); copy mengikuti DESIGN.md §10 (sentence case, kata kerja aktif,
  error menyebut penyebab + solusi, tanpa label ALL CAPS).
- Perubahan switch dari UI/hotkey harus terlihat: OSD (sesuai `hotswitch.osd`) dan chip status.
- Setelah selesai, minta review agent `ux-theme-reviewer`.
