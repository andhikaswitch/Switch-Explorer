# Keputusan Teknis (ADR)

Format: Konteks → Keputusan → Konsekuensi. Status: Diterima, Diusulkan, atau Diganti (sebutkan penggantinya).
Mengubah keputusan = tambah ADR baru yang mengganti, jangan menghapus yang lama.

## ADR-001 — Framework UI: GPUI Kit
**Status:** Diterima, dengan gerbang go/no-go di Fase 0.

**Konteks.** Targetnya "lebih ringan dari Explorer", tampilan translucent modern, kustomisasi berbasis
token, widget khas explorer (tabel virtual ratusan ribu baris, tree, dock), dan bisa dikerjakan bersama AI.

| Kriteria | GPUI Kit | Slint | Tauri 2 | egui | Iced |
|---|---|---|---|---|---|
| RAM idle (kualitatif) | rendah | sangat rendah | sedang–tinggi (proses WebView2) | rendah | rendah |
| Backdrop Mica | bawaan (`MicaBackdrop`, `MicaAltBackdrop`) | manual via HWND | bawaan (window effects) | manual | manual |
| Widget explorer | DataTable & VirtualList virtual, Dock, 75+ komponen | ListView, TableView dasar | apa saja (web) | dasar | dasar |
| Tema berbasis token | ya (ThemeColor, puluhan tema bawaan) | globals, bangun sendiri | CSS variables | struct Visuals | Theme/Palette |
| Teks di Windows | DirectWrite | renderer sendiri | Chromium | renderer sendiri | cosmic-text |
| Aksesibilitas | AccessKit | AccessKit | baik (Chromium) | AccessKit | terbatas |
| Stabilitas API | pra-1.0, versi dipin | 1.x stabil | 2.x stabil | stabil, immediate mode | sering breaking |
| Dukungan AI coding | llms-full.txt + skill resmi | baik | sangat baik | baik | sedang |
| Bahasa | 100% Rust | Rust + DSL `.slint` | Rust + JS/TS | Rust | Rust |

**Keputusan.** GPUI Kit (`gpui-kit`): satu dependency berisi GPUI (Zed), gpui-base (primitif tanpa gaya:
fokus, seleksi, overlay, virtualisasi, aksesibilitas), dan gpui-component (komponen bergaya). Komponen
signature dibangun di gpui-base (visual 100% milik kita); komponen kompleks memakai gpui-component.

**Alasan utama.** UI native Rust (tanpa WebView) untuk RAM/startup; Mica bawaan lewat API DWM resmi;
tabel virtual skala besar; sistem tema semantik dengan banyak tema bawaan (cocok untuk "ricing");
AccessKit; UI testing (`#[gpui_kit::test]`); dokumentasi untuk AI (llms-full.txt, skill resmi).

**Konsekuensi.** API pra-1.0 → pin versi exact, upgrade di PR khusus. Integrasi shell (OLE DnD, context
menu native) kita bangun sendiri via HWND. Fitur `gpui-fast` dievaluasi di Fase 0.

**Gerbang Fase 0 (wajib lolos, jika ≥ 2 gagal tanpa jalan keluar wajar → evaluasi Slint):**
1. Mica Alt + title bar kustom + snap layout saat hover maximize.
2. Tabel 100k baris scroll ≥ 60 fps (release).
3. Idle 1 window ≤ 90 MB private working set.
4. OLE drag-out ke Chrome & Explorer dari HWND GPUI.
5. Menu native `IContextMenu` tampil & bekerja (dengan forwarding pesan).
6. Ikut light/dark Windows ≤ 300 ms.
7. Input teks + IME bekerja.

**Kenapa bukan Tauri walau FariSYNC memakainya.** Tauri menghemat ukuran installer dan backend-nya Rust,
tapi UI-nya tetap WebView2 (berbasis Chromium, beberapa proses). Untuk target "lebih ringan dari Explorer"
kita butuh UI yang juga native.

## ADR-002 — Windows-first
**Status:** Diterima. Fitur inti bergantung pada shell Windows (ikon, thumbnail, context menu, OLE,
Recycle Bin, dialog). v1 hanya Windows. GPUI Kit mendukung macOS/Linux, dan semua panggilan platform ada di
balik trait (`ShellServices`, `HotkeyBackend`), jadi port tetap terbuka.

## ADR-003 — Engine operasi file hibrida
**Status:** Diterima. Copy/move memakai engine sendiri: `CopyFile2` (progress per byte, cancel, pause via
stop + restartable) dan `MoveFileExW` untuk volume yang sama. `IFileOperation` dipakai untuk Recycle Bin,
namespace shell (MTP, ZIP), dan elevasi UAC. Alternatif ditolak: `IFileOperation` untuk semuanya (kontrol
UI/journal kurang), `std::fs` murni (tanpa progress/pause).

## ADR-004 — Format penyimpanan
**Status:** Diterima. TOML untuk yang diedit manusia (settings, keymap, tema, override), JSON untuk state
(session, views), JSONL append-only untuk journal. Tanpa SQLite di inti (ukuran & kesederhanaan); boleh
untuk modul lazy (tags/indeks) nanti.

## ADR-005 — Warna OKLCH & token berlapis
**Status:** Diterima. Palet digenerate dari satu seed di OKLCH (seragam secara persepsi) sehingga aksen
kustom tetap harmonis. Tiga lapis: palet → semantik → komponen. Kontras divalidasi otomatis (THEMING.md).

## ADR-006 — File picker: Bridge dulu, Takeover eksperimental
**Status:** Diterima. Mengganti dialog Open/Save aplikasi lain tidak didukung resmi oleh Windows, dan
pengembang file manager besar menyatakan pendekatan pengganti rawan merusak aplikasi yang memodifikasi
dialog. Files (files-community) punya proyek eksperimental yang meng-override `FileOpenDialog`. Keputusan:
- Level 1 **Picker Bridge** (default saat integrasi diaktifkan): panel pendamping + lompat folder, tanpa injeksi.
- Level 2 **Picker Takeover** (eksperimental, default mati, butuh Developer Mode): COM proxy yang
  membungkus dialog asli, fallback otomatis ke dialog native, kill switch.

## ADR-007 — Google Drive via REST + OAuth PKCE loopback
**Status:** Diterima. `reqwest` + rustls, runtime tokio lazy yang dimatikan saat idle. Dua mode scope:
penuh (`drive`, untuk build pribadi dengan OAuth client sendiri) dan terbatas (`drive.file`). Google
Picker API ditolak karena berbasis web (butuh WebView).

## ADR-008 — Tanpa telemetri
**Status:** Diterima. Tidak ada data yang dikirim keluar. Log & crash report lokal; user yang memutuskan
membagikannya.

## ADR-009 — Single instance + resident mode opsional
**Status:** Diterima. Satu proses melayani semua window; proses kedua meneruskan argumen lewat pipe.
Resident mode (default mati) membuat Win+E/buka folder terasa instan dengan biaya RAM kecil.

## ADR-010 — OLE drag & drop sendiri + Drag Guard
**Status:** Diterima. Dukungan drop bawaan framework umumnya hanya menangani path. Kita butuh efek
(copy/move/link), virtual files, drag image, drag-out ke aplikasi lain, dan aturan Drag Guard, jadi OLE
DnD diimplementasikan di `sx-shell` dengan `IDropSource`/`IDropTarget`/`IDataObject` milik shell.

## ADR-011 — Memakai ulang layanan shell Windows
**Status:** Diterima. Thumbnail lewat `IShellItemImageFactory` (memakai thumbcache yang sama dengan
Explorer), ikon lewat system image list, preview Office/PDF lewat preview handler sistem, properti lewat
property system. Tidak membundel renderer → binary kecil dan RAM rendah.

## ADR-012 — Ikon
**Status:** Diterima. Chrome UI memakai Fluent UI System Icons (MIT) agar terasa Windows 11, menimpa aset
ikon default GPUI Kit lewat mekanisme asset. Ikon file berasal dari sistem.

## ADR-013 — i18n
**Status:** Diterima. Ikuti mekanisme I18N GPUI Kit; `en` dan `id` sejak hari pertama; bahasa awal
mengikuti bahasa tampilan Windows, bisa diganti di Settings.

## ADR-014 — Hot Switch: chord `Ctrl+K` + OSD milik sendiri
**Status:** Diterima.
- Master drag & drop memakai kombinasi tunggal `Ctrl+Shift+D` (paling sering dipakai, bisa global).
- Switch lain memakai chord mnemonik `Ctrl+K` lalu huruf (F file, D folder/directory, S sidebar, E
  eksternal, G guard, L lock, H hidden, X extension, T thumbnail, R read-only, K panel).
- Chord hanya aktif di luar input teks agar tidak "memakan" karakter; scope global hanya untuk
  kombinasi tunggal (`RegisterHotKey`).
- OSD memakai window ringan milik sendiri, bukan notifikasi Windows: lebih cepat, tidak menumpuk di
  Notification Center, tidak butuh registrasi AUMID.
- Perubahan persisten secara default (`hotswitch.persist`).

Alternatif ditolak: default `Ctrl+Alt+huruf` (bentrok dengan AltGr di banyak layout keyboard), tombol F
(tidak mnemonik), toast Windows (lambat, menumpuk).

## ADR-015 — Loop modal OS dijalankan dari window host milik sendiri
**Status:** Diterima (dibuktikan di spike Fase 0, `docs/benchmarks/phase0.md`).

**Konteks.** `DoDragDrop`, `TrackPopupMenuEx`, dan dialog shell menjalankan loop pesan bersarang. Pola
gpui-kit `NativeMenu` menjalankan loop itu di dalam task foreground. Dengan fitur `profiler` GPUI aktif
(dibutuhkan Perf HUD), cara itu **panic**: loop bersarang mengeksekusi task lain di dalam task pembuka,
padahal `ThreadTimings::save_task_timing` hanya punya satu slot "task berjalan" per thread
(`gpui-pre 0.3.10`, `profiler.rs:602`). Di spike, UI langsung beku setelah menu ditutup.

**Keputusan.** Semua loop modal OS dijalankan dari window message-only milik `sx-shell` ("modal host"):
1. Handler GPUI hanya mem-post job ke window itu.
2. Wndproc window itu menjalankan job dari loop pesan biasa, di luar borrow GPUI maupun executor.
3. Hasilnya dikirim balik lewat channel `oneshot` yang ditunggu task foreground.

**Konsekuensi.**
- Tidak ada panic, dan task GPUI tetap berjalan selama loop modal: tick UI tiap ~108 ms terukur saat menu
  dan drag terbuka.
- Job disimpan di antrean milik thread, bukan sebagai pointer di pesan. Hanya satu job berjalan
  sekaligus: wake-up dari loop bersarang diabaikan, lalu dikirim ulang setelah job selesai. Window
  dibuat di UI thread saat startup.
- Setelah `DoDragDrop`, kirim `WM_LBUTTONUP` sintetis karena loop modal menelannya.
- Bug profiler dilaporkan ke upstream; jangan memakai `NativeMenu` gpui-kit bersama fitur `profiler`.

## ADR-016 — Drop target OLE milik sendiri
**Status:** Diterima (keputusan Fase 0).

**Konteks.** GPUI mendaftarkan `IDropTarget` sendiri. Target itu hanya pernah menjawab
`DROPEFFECT_COPY` dan hanya meneruskan path, tanpa modifier, tanpa zona, dan tanpa format lain.
Drag Guard, move/link, dan virtual files tidak bisa dibangun di atasnya.

**Keputusan.** `sx-shell` memanggil `RevokeDragDrop` lalu `RegisterDragDrop` dengan target sendiri:
efek dari modifier dan zona drop (bounds dicatat per frame lewat `canvas`), plus `IDropTargetHelper`
untuk drag image. Spike membuktikan ini aman dan menerima drop dari Explorer maupun item shell.

**Konsekuensi.**
- API `on_drop::<ExternalPaths>` GPUI tidak dipakai di area file.
- Efek selalu diiriskan dengan efek yang diizinkan sumber (`*pdweffect` masuk). Modifier yang tidak
  diizinkan menghasilkan NONE.
- `Drop` membalas efek yang **benar-benar** dijalankan `sx-ops`. Kalau MOVE dibalas tanpa dikerjakan,
  sumber non-optimized akan menghapus file aslinya. Ikuti aturan optimized move dan
  `CFSTR_PERFORMEDDROPEFFECT`.
- Zona drop harus memakai bounds yang benar: elemen `absolute` wajib diberi `inset_0`, kalau tidak ia
  jatuh ke posisi statisnya.

## ADR-017 — Tetap memakai snapshot GPUI upstream, bukan `gpui-fast`
**Status:** Diterima (keputusan Fase 0).

**Konteks.** gpui-kit menawarkan fitur `gpui-fast` (engine retained). Aturan dari rencana Fase 0:
`gpui-fast` dipilih hanya jika p99 frame ≥ 15% lebih baik atau RAM ≥ 10 MB lebih kecil, tanpa regresi
fungsional.

**Hasil (iGPU, 165 Hz, `docs/benchmarks/phase0.md`).** Dibanding upstream, `gpui-fast`:
- startup median 238.6 ms vs 194.9 ms;
- idle 40–42 MB vs 36.8 MB;
- RAM saat scroll 100k naik terus hingga 137–179 MB vs 57–61 MB;
- p99 frame setara atau sedikit lebih buruk, dengan satu frame 30 ms;
- exe 24.6 MB vs 21.8 MB.

Satu-satunya keunggulannya CPU idle (16–47 ms vs ~250 ms per 10 s).

**Keputusan.** Upstream (default). `gpui-fast` dievaluasi ulang bila masalah RAM-nya selesai, atau bila
vsync upstream tetap tidak bisa idle.

## ADR-018 — OSD: overlay in-app, window OSD dipakai ulang (amandemen ADR-014)
**Status:** Diterima (2 November 2026, didelegasikan pemilik proyek).

**Konteks.** ADR-014 memakai window ringan untuk OSD. Di spike, setiap window GPUI tambahan menambah
**+13.5 MB** private working set (swap chain + sumber daya), walau frame pertamanya cepat (12.8 ms).

**Usulan.**
- Perubahan dari dalam app → OSD digambar sebagai overlay di workspace (tanpa window baru).
- Hotkey global saat app di background → satu window OSD yang dibuat saat pertama dipakai, lalu
  disembunyikan (`SW_HIDE`) dan dipakai ulang. Window itu dibuang saat minimize/idle panjang.
- Resident mode tanpa window → window OSD dibuat on-demand dan dibuang setelah tampil.

## ADR-019 — Lisensi `MIT OR Apache-2.0`
**Status:** Diterima (menutup pertanyaan terbuka PRD §10).

Lisensi ganda konvensi ekosistem Rust: kompatibel dengan GPUI Kit (Apache-2.0) dan dependency umum,
serta memberi pengguna pilihan. File `LICENSE-MIT` dan `LICENSE-APACHE` ada di root. Setiap crate
menulis `license.workspace = true`.

## ADR-020 — Fase 1 dimulai dengan verifikasi Fase 0 yang masih terbuka
**Status:** Diterima.

Dua item Fase 0 belum bisa diverifikasi otomatis dan menunggu tangan manusia:
- IME Jepang, karena IME Microsoft menolak input sintetis;
- fallback Windows 10, karena butuh VM.

Keduanya tidak memblokir Fase 1. Jalur kode IME sama dengan yang dipakai Zed (IMM32 GPUI), dan fallback
Win10 sudah punya cabang `Opaque`.

Risiko diterima. Keduanya tetap tercatat di ROADMAP dan wajib lulus sebelum rilis (Fase 8). Target
`i686` ditunda sampai `sx-picker-shim` benar-benar dibangun (Fase 6).

---

## ADR-021 — Engine operasi file Fase 1: `std::fs` + shell, tanpa `IFileOperation` untuk copy/move

**Status:** Diterima (Fase 1). Ditinjau ulang di Fase 2.

`sx-ops` menjalankan rename, folder baru, copy, dan move lewat `std::fs`:
- Copy memakai `std::fs::copy` (`CopyFileExW`).
- Move dalam volume yang sama memakai `rename`. Move antar volume memakai copy → cek ukuran → hapus sumber.

Recycle Bin dan restore lewat shell (`IFileOperation` dengan progress sink per item).

Alasan:
- Kebijakan konflik, journal, undo, dan cancel harus milik kita (`.claude/rules/file-ops-safety.md`).
  `IFileOperation` membawa dialog konflik dan progress-nya sendiri.
- Engine bisa dites tanpa COM, cukup dengan `Trash` palsu di folder sementara.

Konsekuensi:
- Progress dilaporkan per item, belum per byte.
- Pause dan "Lanjutkan sebagai admin" (F-OPS-08) belum ada.
- `CopyFile2` dengan callback progress dan cancel, serta `IFileOperation` khusus untuk elevasi, masuk Fase 2.

Replace tidak pernah menimpa. Item lama dipindah ke Recycle Bin dulu, sehingga undo bisa memulihkannya.

Undo memulihkan item persis yang di-recycle, bukan item lain dari path yang sama:
- Progress sink (`IFileOperationProgressSink::PostDeleteItem`) mencatat slot tiap item di Recycle Bin
  (`$R…`). Slot itu disimpan di operasi (`Op::Recycle.binned`, `Op::Copy.replaced_slots`) dan di journal.
- Restore memindahkan item dari slotnya ke folder asal dengan `FOF_RENAMEONCOLLISION`, lalu menghapus file
  metadata `$I…` miliknya seperti Restore di Explorer. `PostMoveItem` melaporkan path akhir ("nama (2)"
  bila nama aslinya sudah dipakai lagi).
- Redo dibangun dari kebalikan hasil undo, sehingga redo me-recycle "nama (2)", bukan file baru milik user.
- Versi awal Fase 1 mencocokkan item lewat cap waktu dalam detik. Itu bisa memilih versi yang salah bila
  path yang sama di-recycle dua kali dalam detik yang sama, jadi diganti.

Operasi yang berhenti di tengah (cancel, error, item yang ditolak Recycle Bin) tetap melaporkan bagian yang
sudah jalan (`Outcome.done`). Journal dan undo mencakup persis bagian itu. Item yang terlalu besar untuk
Recycle Bin hanya dihapus permanen setelah user setuju di peringatan shell (`FOF_WANTNUKEWARNING`), lalu
disebut di toast.

## ADR-022 — Watcher: muat ulang debounce, bukan diff inkremental

**Status:** Diterima (Fase 1).

`FindFirstChangeNotificationW` per tab hanya memberi tahu "ada perubahan". Folder dimuat ulang
setelah 250 ms tenang. Daftar lama tetap tampil sampai yang baru siap, dan seleksi serta fokus
dipertahankan berdasarkan nama.

Alasannya, pendekatan ini sederhana, benar untuk semua jenis perubahan, dan murah (10k entri ≈ 20 ms
di release).

Diff inkremental lewat `ReadDirectoryChangesW` baru dikerjakan kalau pengukuran pada folder besar yang
sering berubah menunjukkan perlu.

## ADR-023 — Fuzzy matcher sendiri, bukan nucleo

**Status:** Diterima (Fase 2).

**Konteks.** Command palette (F-CMD-01) dan quick open (F-SRCH-04) membutuhkan pencocokan fuzzy.
Rencana awal memakai nucleo. Kandidatnya pendek dan sedikit: label command, switch, dan tempat, jumlahnya
ratusan.

**Opsi.**
- `nucleo-matcher`. Matcher yang sudah teruji, tetapi berlisensi MPL-2.0. Lisensi ini membawa kewajiban
  sumber per file yang berbeda dari lisensi proyek (ADR-019), dan berarti satu dependency lagi.
- Matcher sendiri di `sx-search::fuzzy`, bergaya fzf v1: maju, mundur ke jendela terpendek, lalu skor
  dengan bonus awal kata, run, dan prefix.

**Keputusan.** Matcher sendiri. Kodenya sekitar 150 baris dengan test peringkat, tanpa dependency, dan
biayanya mikrodetik per ketikan untuk ratusan kandidat.

**Konsekuensi.** Bila quick open kelak mencocokkan ribuan path dan hasilnya kurang baik atau terlalu
lambat, `nucleo-matcher` dievaluasi ulang. API `fuzzy_match(query, candidate)` bisa dipertahankan.

---

## Template ADR

```markdown
## ADR-0NN — Judul singkat
**Status:** Diusulkan
**Konteks.** Masalah dan batasan.
**Opsi.** A, B, C dengan kelebihan/kekurangan.
**Keputusan.** Yang dipilih dan alasannya.
**Konsekuensi.** Dampak, risiko, pekerjaan lanjutan.
```
