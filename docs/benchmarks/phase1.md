# Benchmark Fase 1 — explorer MVP

Diukur 2026-11-03 di laptop pengembang: Windows 11 build 26200, iGPU, layar 165 Hz, skala 125%, build
release (`lto = "fat"`). Folder uji dibuat dengan `cargo xtask fixtures --count 10000` (9000 file + 1000
folder) di `%TEMP%\sx-fixtures\flat-10000`.

## Cara mengukur

```powershell
cargo build -p switch-explorer --release
# 5× buka folder 10k, cetak waktu listing dan frame pertama berisi baris, lalu keluar:
target\release\switch-explorer.exe --bench-open "$env:TEMP\sx-fixtures\flat-10000"
```

Arti angka `--bench-open`:
- `listing_ms`: enumerasi + sort di background.
- `first_rows_ms`: listing + frame pertama yang menampilkan baris.
- `process_ms`: dari `main` sampai frame itu.

Budget cold start (≤ 300 ms) diukur dari `main` sampai frame berisi baris. Angka ini lebih ketat daripada
"frame pertama" di Fase 0, karena sudah termasuk listing folder.

RAM idle = *private working set* (sama dengan kolom Memory di Task Manager), 8 detik setelah window
terbuka. CPU idle = waktu CPU proses selama 10 detik tanpa input.

## Hasil

| Metrik | Budget | Terukur (5 run) | Status |
|---|---|---|---|
| Folder 10k: baris pertama tampil | ≤ 50 ms | 24,6 / 27,8 / 28,5 / 29,8 / 32,8 ms | lulus |
| Folder 10k: enumerasi + sort | — | 18,3–26,8 ms | — |
| `main` → frame berisi baris (cold start) | ≤ 300 ms | 207 / 240 / 260 / 264 / 304 ms (median 260) | lulus di median; 1 run lewat 3,5 ms |
| RAM idle, Home | catat | 39,2 MB | — |
| RAM idle, folder 10k | catat | 42,0 MB | — |
| RAM idle, System32 dengan ikon sistem | catat | 41,5 MB | — |
| CPU idle 10 s | 0% (dikecualikan, lihat catatan) | 125–219 ms CPU (1,3–2,2%) | dikecualikan |
| Ukuran exe | ≤ 30 MB | 23,26 MB | lulus |

Regresi yang ditemukan dan diperbaiki saat pengukuran: RAM idle sempat naik ke 66 MB karena Tide Slash
dilukis sebagai path GPUI. Path pertama membuat tekstur MSAA 4× seukuran window terpakai (~29 MB di iGPU).
Tide Slash kini dirasterisasi di CPU dan dilukis sebagai gambar, sehingga RAM kembali ke 39 MB.

## Catatan

- Sebagian besar cold start (~140–170 ms) habis di inisialisasi platform GPUI (DirectX, DirectWrite)
  sebelum kode aplikasi berjalan. Pemilihan font (0,2 ms), pemuatan pin (1 ms), dan resolve tema (< 1 ms)
  tidak berarti.
- CPU idle tidak 0% karena thread vsync GPUI menggambar ulang tiap refresh (temuan Fase 0,
  PERFORMANCE §1). Dengan folder berisi baris, biaya render baris ikut terhitung di setiap refresh.
  Perbaikan upstream atau render yang di-cache tetap menjadi tindak lanjut.
- Hot path yang dioptimalkan di Fase 1 (release, 100k entri):
  - sort key 65 → 29 ms;
  - sort natural 70 → 40 ms;
  - 100k ekstensi unik 4,38 s → 24,5 ms.
