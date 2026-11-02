---
name: add-theme-token
description: Menambah token tema baru (warna, paint, shadow, angka) ke sx-theme dan semua tema bawaan, lengkap dengan pasangan kontras dan dokumentasi.
---
# Tambah token tema

1. Pastikan token benar-benar dibutuhkan (cek apakah token semantik yang ada sudah cukup).
2. Pilih id sesuai pola `grup.nama` atau `komponen.state` (mis. `osd.state_on`) dan tipe
   (Color/Paint/Shadow/Number/Font).
3. Deklarasikan di `define_tokens!` (`crates/sx-theme/src/schema.rs`): tipe, grup, deskripsi i18n, pasangan
   kontras bila token berisi teks/ikon/garis fokus.
4. Beri nilai di semua tema bawaan (`themes/*.toml`): `[common]` bila sama di kedua mode, atau `[light]` dan
   `[dark]`. Utamakan referensi (`{pal.accent.500}`, `{hover.bg}`) daripada hex lepas.
5. Jalankan validasi kontras; CI harus tetap lulus untuk semua tema bawaan.
6. Tambahkan ke bridge gpui-component bila relevan, dan ke spesimen pratinjau Theme Studio.
7. Perbarui tabel token di `docs/THEMING.md` §3 (dan contoh Cerulean §5 bila tema bawaan berubah).
