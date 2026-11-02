---
name: perf-audit
description: Audit performa Switch Explorer terhadap budget di docs/PERFORMANCE.md dan tulis laporan di docs/benchmarks.
disable-model-invocation: true
---
# Audit performa

1. Baca `docs/PERFORMANCE.md` (budget & metode). Semua pengukuran di build `--release`.
2. Siapkan fixture: `cargo xtask fixtures --count 1000`, `10000`, `100000`.
3. Jalankan `switch-explorer.exe --bench-open <fixture>` 5× per fixture; ambil median start → frame pertama →
   baris pertama → listing lengkap.
4. Catat RAM (private working set) di skenario: idle Home, folder 100k, setelah scroll penuh dengan thumbnail,
   setelah minimize 60 s, setelah cloud idle. Pakai memory meter untuk rincian per modul.
5. Jalankan benchmark `criterion` (sort, filter, parser tema, palet).
6. Tulis `docs/benchmarks/<tanggal>.md`: tabel hasil vs budget (lulus/gagal), tren dibanding laporan sebelumnya,
   tiga penyebab utama bila ada yang gagal, dan usulan perbaikan berprioritas. Jangan mengubah kode tanpa
   persetujuan.
