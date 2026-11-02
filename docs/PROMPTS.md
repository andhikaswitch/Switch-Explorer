# Prompt untuk Claude Code

## 0. Cara pakai

- **Satu sesi = satu fase atau satu fitur.** Jalankan `/clear` saat berganti tugas besar agar konteks bersih.
- **Rencanakan dulu.** Untuk tugas besar aktifkan Plan Mode (`Shift+Tab` sampai mode plan) supaya Claude
  menyusun rencana tanpa mengubah file, lalu setujui atau koreksi rencananya.
- **Sebut ID.** Contoh: "kerjakan F-DND-06 dan F-KEY-01..05", "ikuti ADR-014". Claude akan membuka bagian
  dokumen yang tepat.
- **Bahasa.** Prompt boleh berbahasa Indonesia. Kode, komentar, dan commit tetap berbahasa Inggris (sudah
  diatur di CLAUDE.md).
- **Review.** Setelah fitur selesai: "Pakai agent windows-integration-reviewer untuk mereview perubahan ini"
  atau "Pakai agent ux-theme-reviewer".
- **Skill proyek:** `/start-phase <n>`, `/add-feature-flag`, `/add-theme-token`, `/perf-audit`.

## 1. Setup sekali di awal

1. `rustup update stable` dan `rustup target add i686-pc-windows-msvc`; pasang Visual Studio Build Tools.
2. Pasang skill resmi GPUI Kit ke `.claude/skills/` (lihat https://gpui-kit.com/skills).
3. Taruh semua file dokumentasi ini di root repo, `git init`, commit pertama.
4. Jalankan `claude` di root repo, lalu pakai prompt kickoff.

## 2. Prompt kickoff

```text
Kamu lead engineer proyek Switch Explorer — file explorer Windows 100% Rust (GPUI Kit), ringan,
translucent, dan setiap fiturnya adalah switch (termasuk Hot Switch via hotkey dengan OSD).

Baca: docs/PRD.md, docs/DECISIONS.md (ADR-001 dan ADR-014), docs/ARCHITECTURE.md, dan
docs/ROADMAP.md bagian Fase 0. CLAUDE.md sudah termuat.

Jangan menulis kode dulu. Berikan:
1. Ringkasan pemahamanmu tentang produk dan batasannya (maks 10 poin).
2. Rencana Fase 0 langkah demi langkah, termasuk cara mengukur setiap kriteria go/no-go.
3. Tiga risiko teknis terbesar menurutmu dan cara memitigasinya.
4. Pertanyaan yang perlu saya jawab sebelum mulai.
```

## 3. Prompt per fase

**Fase 0 — Spike**
```text
/start-phase 0
Tambahan: buat spike di spikes/phase0 sebagai crate terpisah (boleh dibuang). Urutan: window Mica Alt +
title bar kustom → tabel virtual 100k baris → ikut light/dark sistem → OLE drag-out ke Chrome via HWND →
menu native IContextMenu → chord Ctrl+K + RegisterHotKey → IME. Setelah tiap langkah, catat hasil ukur
(release) di docs/benchmarks/phase0.md. Akhiri dengan rekomendasi go/no-go yang jujur.
```

**Fase 1 — Fondasi & MVP**
```text
/start-phase 1
Mulai dari scaffolding workspace sesuai docs/ARCHITECTURE.md §3 dan docs/CODING-STANDARDS.md §1–2,
lalu sx-settings (define_flags!) dan sx-theme (themes/cerulean.toml dari docs/THEMING.md §5) sebelum UI.
Setiap crate non-platform wajib #![forbid(unsafe_code)].
```

**Fase 2 — Paritas + Drag Guard + Hot Switch**
```text
/start-phase 2
Prioritas: CommandRegistry → context menu (tema + native) → OLE DnD dua arah → Drag Guard → Hot Switch
→ thumbnail & preview → search → Home & Aktivitas. Ikuti docs/DEVELOPER-MODE.md §5–§6 dan
docs/DESIGN.md §9 untuk OSD dan chip status.
```

**Fase 3 — Personalisasi & Developer Mode**
```text
/start-phase 3
Settings & halaman switch harus digenerate dari metadata registry flag, bukan dibuat manual per flag.
Theme Studio mengikuti docs/THEMING.md §10–§13 (termasuk adaptasi otomatis dan panel kontras).
```

**Fase 4 — Google Drive**
```text
/start-phase 4
Baca docs/CLOUD-GOOGLE-DRIVE.md lengkap. Mulai dari auth.rs + test wiremock, lalu popup tujuan,
lalu upload resumable dengan verifikasi MD5. Runtime tokio harus lazy dan mati saat idle.
Client ID dibaca dari cloud.local.toml atau env SX_GDRIVE_CLIENT_ID; jangan pernah di-commit.
```

**Fase 5 — Integrasi sistem**
```text
/start-phase 5
Semua tulis registry lewat sx_integration::registry dengan backup + unregister + cleanup. Setelah selesai,
pakai agent windows-integration-reviewer, lalu isi matriks Bridge di docs/TESTING.md §5.
```

**Fase 6 — Picker Takeover**
```text
/start-phase 6
Ikuti docs/SYSTEM-INTEGRATION.md §14 secara ketat: proxy yang membungkus dialog asli dari comdlg32
(DllGetClassObject, bukan CoCreateInstance), fallback di setiap langkah, kill switch 4 jalur, penghitung
kegagalan. Fitur tetap default mati dan hanya muncul di Developer Mode.
```

**Fase 7 — Tools** / **Fase 8 — Rilis**
```text
/start-phase 7
Organizer dulu (paritas FariSYNC + template + dry-run + undo), lalu Duplicate Finder, Bulk Rename,
Space Analyzer. Semua operasi lewat sx-ops agar tercatat di journal.
```
```text
/start-phase 8
Jalankan /perf-audit, audit aksesibilitas (DESIGN.md §14), audit keamanan (SECURITY.md), lalu packaging.
Uninstaller wajib menjalankan cleanup registry sebelum menghapus file.
```

## 4. Prompt fitur spesifik

**Drag Guard + Hot Switch (inti)**
```text
Implementasikan F-DND-06, F-DND-07, dan F-KEY-01 sampai F-KEY-05.
- Switch: tabel docs/DEVELOPER-MODE.md §5.2 dan §6.3, lewat define_flags!.
- DragController: state machine §5.4 dengan Clock yang bisa dipalsukan; catat waktu scroll terakhir dari
  semua sumber scroll. Seleksi campuran ditolak seluruhnya bila salah satu jenis nonaktif.
- Hot Switch: action switch::Toggle/Cycle/Group/Preset; binding default §6.2; chord Ctrl+K tidak aktif saat
  fokus di input teks; preset menyimpan & mengembalikan nilai sebelumnya.
- Test unit untuk setiap baris tabel kasus + UI test: tekan ctrl-k d → flag berubah → OSD muncul.
Rencanakan dulu, tunggu persetujuan saya.
```

**OSD & chip status**
```text
Buat komponen OSD dan chip status sesuai docs/DESIGN.md §9 di atas gpui-base. OSD tidak mengambil fokus,
memperbarui instance yang sama saat toggle beruntun, diumumkan sebagai live region, dan memakai token
osd.* serta statuschip.*. Untuk hotkey global, OSD tampil sebagai window kecil topmost tanpa aktivasi di
monitor aktif. Tambahkan spesimen OSD di pratinjau Theme Studio.
```

**Theme Studio**
```text
Implementasikan Theme Studio (docs/THEMING.md §13) dengan tiga tab. Override disimpan ke
theme-overrides.toml (§6), pilihan "Mode ini / Keduanya / Adaptasi otomatis" (§10), panel kontras dengan
tombol perbaikan otomatis (§11). Perubahan tampil live < 50 ms.
```

**Token Inspector**
```text
Implementasikan Token Inspector (Ctrl+Shift+I) sesuai docs/THEMING.md §13. Registrasi (bounds, tokens)
hanya terjadi saat inspector aktif; ukur bahwa biaya saat nonaktif nol. Klik membuka tab Token di token itu.
```

**Popup tujuan Google Drive**
```text
Implementasikan popup tujuan Drive (docs/DESIGN.md §8.3, docs/CLOUD-GOOGLE-DRIVE.md §7.2): pohon folder
lazy dengan paging, + Folder baru (langsung dibuat di Drive lalu terpilih), Salin/Pindahkan, kebijakan
konflik, tujuan favorit di pins.toml. Mode Pindahkan hanya membuang file lokal setelah verifikasi checksum.
```

**Picker Bridge**
```text
Implementasikan Picker Bridge (docs/SYSTEM-INTEGRATION.md §13). Deteksi dialog dengan SetWinEventHook
out-of-context + UI Automation, bukan posisi piksel. Lewati dialog milik proses elevated. Tambahkan
diagnostik di halaman Developer. Buat daftar uji manual untuk Chrome, Firefox, WhatsApp, VS Code, Office.
```

**Organizer**
```text
Implementasikan Organizer (F-TOOL-01). Wajib dry-run (pohon sebelum/sesudah + konflik) sebelum eksekusi,
template tujuan tervalidasi (tidak boleh keluar folder tujuan), dan undo seluruh run dari journal.
UI mengambil inspirasi bar komposisi & chip ekstensi FariSYNC (docs/REFERENCE-ANALYSIS.md §3).
```

## 5. Prompt pemeliharaan

**Laporan bug**
```text
Bug: <apa yang terjadi>
Langkah: 1) … 2) … 3) …
Harapan: <apa yang seharusnya>
Lingkungan: Windows <versi>, build <dev/release>, flag yang diubah: <…>
Log: <potongan log level debug, tanpa data pribadi>
Cari akar masalahnya dulu, jelaskan, lalu usulkan perbaikan + test regresi sebelum mengubah kode.
```

**Audit performa**
```text
/perf-audit
```

**Review sebelum merge**
```text
Review semua perubahan di branch ini terhadap checklist PR di docs/CODING-STANDARDS.md §12.
Pakai agent yang relevan (windows-integration-reviewer / ux-theme-reviewer). Laporkan temuan berdasarkan
tingkat keparahan; jangan mengubah file.
```

**Sinkron dokumen**
```text
Bandingkan perilaku kode saat ini dengan docs/FEATURES.md dan docs/DEVELOPER-MODE.md. Daftar semua
perbedaan; untuk masing-masing usulkan apakah kode atau dokumen yang perlu diubah. Tunggu keputusan saya.
```

**Upgrade GPUI Kit**
```text
Upgrade gpui-kit dari <versi lama> ke <versi baru> di PR terpisah. Baca catatan rilis dan llms-full.txt
versi baru, daftar breaking changes yang mengenai kode kita, perbaiki, jalankan semua test & UI test,
lalu bandingkan fps/RAM dengan laporan benchmark terakhir.
```

**Refactor aman**
```text
Refactor <modul> tanpa mengubah perilaku. Pastikan test yang ada mencakup perilaku sekarang (tambah bila
kurang) sebelum mengubah apa pun. Langkah kecil, test hijau di setiap langkah.
```

## 6. Template kosong

```text
Tugas: <ID fitur / deskripsi>
Konteks: <dokumen & bagian yang relevan>
Batasan: <performa, keamanan, flag yang terlibat>
Selesai jika: <acceptance criteria yang bisa dicek>
Rencanakan dulu dan tunggu persetujuan saya sebelum menulis kode.
```
