# Spike Fase 0 — Switch Explorer

Aplikasi minimal untuk menguji gerbang go/no-go ADR-001 (lihat `docs/ROADMAP.md` Fase 0). Kode ini
**boleh dibuang**, tapi dipertahankan sebagai harness regresi setiap kali GPUI Kit di-upgrade. Laporan
hasil: [`docs/benchmarks/phase0.md`](../../docs/benchmarks/phase0.md).

Aturan produksi (i18n, token tema, `forbid(unsafe_code)` per crate) sengaja dilonggarkan di sini.
Spike tidak pernah menulis registry dan tidak menjalankan operasi file. Drop hanya dilaporkan, dan
drag-out dibatasi ke copy/link.

## Build & jalan

```powershell
cd spikes\phase0
cargo run                                   # interaktif (debug)
cargo build --release                       # angka resmi selalu dari release
cargo build --release --features gpui-fast --target-dir target-fast
```

| Argumen | Arti |
|---|---|
| `--trace` | aktifkan profiler GPUI; latensi input→present ditulis ke log |
| `--own-drop` | ganti drop target bawaan GPUI dengan milik spike (RevokeDragDrop + RegisterDragDrop) |
| `--opaque` | paksa backdrop solid (meniru fallback Windows 10) |
| `--table datatable\|uniform` | varian tabel |
| `--data fixture\|synthetic` | folder fixture nyata (`%TEMP%\sx-fixtures\phase0`) atau 100k baris sintetis |
| `--bench idle\|startup\|scroll-slow\|scroll-fast\|scroll-jump\|scroll-pixel` | satu pengukuran lalu keluar |
| `--seconds N`, `--rows N`, `--tag nama`, `--out dir` | parameter benchmark |

Log per proses: `out\spike-<pid>.log`. Hasil benchmark: `out\summary.tsv`, `out\startup.tsv`,
`out\idle-external.tsv`. Matriks lengkap: `.\scripts\measure.ps1 -Exe <exe> -Tag <label>`.

## Checklist manual (butuh tangan & mata)

Jalankan `cargo run -- --trace`, lalu:

1. **IME Jepang (gerbang 7).** Klik kotak input, tekan `Win+Space` untuk pilih Microsoft IME Jepang.
   Tekan `Alt+`` agar mode あ aktif, lalu ketik `nihongo`. Yang diharapkan:
   - teks komposisi bergaris bawah;
   - `Space` membuka kandidat **di dekat caret**;
   - `Enter` meng-commit 日本語;
   - Backspace di tengah komposisi bekerja.
   Coba juga `Win+.` (emoji) dan `Win+V`.
2. **Ikut light/dark (gerbang 6).** Settings → Personalization → Colors → ganti mode app. Log harus
   memuat `latency theme: ... registry→present X ms` (≤ 300 ms). Ulangi beberapa kali.
3. **Drag dari Explorer ke spike.** Jalankan dua kali: tanpa dan dengan `--own-drop`. Dengan target
   sendiri, tahan Ctrl/Shift/Alt dan lihat efek (copy/move/link) di log.
4. **Hotkey global di background.** Pindah fokus ke aplikasi lain, lalu tekan `Ctrl+Shift+D`. OSD
   kecil harus muncul di bawah layar tanpa merebut fokus.
5. **Windows 10 (VM).** `cargo run` di Windows 10 22H2: window solid, tidak hitam/tembus, tidak crash.
