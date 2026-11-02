# Integrasi Google Drive

## 1. Jawaban singkat

- **Apakah dibuka di browser?** Hanya **sekali** saat menghubungkan akun (login & persetujuan Google).
  Setelah itu semuanya terjadi di dalam Switch Explorer; browser tidak dibuka lagi kecuali akun perlu
  dihubungkan ulang.
- **Bisa dilihat di explorer sendiri?** Bisa. Drive muncul di sidebar dan bisa dijelajahi. Yang dimuat
  hanya metadata (nama, ukuran, tanggal) saat folder dibuka, jadi tambahan RAM-nya kecil; saat tidak
  dipakai, modul jaringan dimatikan dan memorinya kembali.
- **Alur utama:** klik kanan file/folder → **Upload ke Drive** atau **Pindahkan ke Drive** → popup berisi
  pohon folder Drive → pilih folder atau buat folder baru → upload. "Pindahkan" membuang file lokal ke
  Recycle Bin hanya setelah checksum di Drive cocok.
- **Sudah pakai Google Drive for desktop?** Drive virtual (mis. `G:\`) otomatis muncul seperti drive biasa
  tanpa koneksi API.

## 2. Cara kerja koneksi (OAuth 2.0 PKCE + loopback)

```text
Switch Explorer                    Browser default                 Google
 │ buat code_verifier + state         │                              │
 │ buka server 127.0.0.1:<port acak>  │                              │
 │──── buka URL login ───────────────▶│──── login & setujui ────────▶│
 │                                    │◀─── redirect 127.0.0.1?code ─│
 │◀─── GET /?code=…&state=… ──────────│  (halaman "Berhasil, tab ini boleh ditutup")
 │ cek state, tutup server            │                              │
 │──── tukar code + code_verifier ───────────────────────────────────▶│ /token
 │◀─── access token (±1 jam) + refresh token ────────────────────────│
 │ simpan refresh token di Windows Credential Manager                │
```

- Access token hanya di memori; diperbarui otomatis memakai refresh token sebelum kedaluwarsa.
- Server loopback hanya mendengar di `127.0.0.1`, menerima satu permintaan, timeout 5 menit.
- Disconnect = cabut token di Google (`/revoke`) + hapus kredensial lokal.

## 3. Setup Google Cloud (build pribadi)

1. Buka Google Cloud Console → buat project → aktifkan **Google Drive API**.
2. OAuth consent screen → tipe **External** → isi nama aplikasi & email → tambahkan scope (lihat §4).
3. **Publishing status: In production.** Pada status *Testing*, refresh token untuk aplikasi tipe External
   kedaluwarsa dalam 7 hari sehingga harus login ulang tiap minggu.
4. Credentials → **Create OAuth client ID** → tipe **Desktop app**.
5. Simpan client ID/secret di `cloud.local.toml` (masuk `.gitignore`) atau variabel lingkungan saat build
   (`SX_GDRIVE_CLIENT_ID`, `SX_GDRIVE_CLIENT_SECRET`). Untuk aplikasi desktop, client secret tidak benar-benar
   rahasia; keamanan dijaga PKCE. Tetap jangan di-commit.

## 4. Strategi scope

| Mode | Scope | Kemampuan | Konsekuensi |
|---|---|---|---|
| **Penuh** (default build pribadi) | `drive` | Menjelajah seluruh Drive, upload ke folder mana pun, buat folder di mana pun | Scope *restricted*. Untuk pemakaian pribadi dengan client sendiri: muncul layar peringatan "aplikasi belum diverifikasi" yang bisa dilewati. Untuk distribusi publik: wajib verifikasi Google + security assessment tahunan |
| **Terbatas** | `drive.file` | Hanya file/folder yang dibuat aplikasi ini | Scope *non-sensitive*, verifikasi ringan. Popup tujuan hanya menampilkan folder "Switch Explorer" dan subfolder buatan app |

Catatan: scope yang hanya membaca metadata (`drive.metadata.readonly`) juga tergolong *restricted*, jadi
tidak ada jalan tengah yang lebih ringan untuk menjelajah seluruh Drive. Google Picker API (yang bisa
memakai `drive.file`) berbasis web dan butuh WebView, jadi tidak dipakai (ADR-007).

## 5. Arsitektur modul (`sx-cloud`)

```text
sx-cloud/
├─ provider.rs     trait CloudProvider + enum Provider (dispatch statis)
├─ gdrive/
│  ├─ auth.rs      PKCE, server loopback, token refresh, revoke
│  ├─ api.rs       klien REST tipis di atas reqwest (rustls)
│  ├─ upload.rs    upload resumable + MD5 inkremental
│  └─ model.rs     RemoteEntry, AccountInfo, Quota
├─ transfer.rs     antrean transfer (concurrency, bandwidth, pause/resume, retry)
├─ credentials.rs  Windows Credential Manager (crate keyring)
└─ runtime.rs      runtime tokio lazy (2 thread), dimatikan setelah idle
```

```rust
pub trait CloudProvider: Send + Sync {
    async fn account(&self) -> Result<AccountInfo>;   // email, nama, kuota
    async fn list_children(&self, folder: &RemoteId, page: Option<PageToken>, filter: ListFilter)
        -> Result<Page<RemoteEntry>>;
    async fn create_folder(&self, parent: &RemoteId, name: &str) -> Result<RemoteEntry>;
    async fn find_by_name(&self, parent: &RemoteId, name: &str) -> Result<Vec<RemoteEntry>>;
    async fn upload(&self, req: UploadRequest, progress: ProgressSink, cancel: CancelToken)
        -> Result<RemoteEntry>;
    async fn download(&self, id: &RemoteId, dest: &Path, progress: ProgressSink, cancel: CancelToken)
        -> Result<()>;
    async fn trash(&self, id: &RemoteId) -> Result<()>;
}
```

Async fn di trait tidak object-safe; pakai enum `Provider` (dispatch statis) atau crate `async-trait`.
Transfer yang menyentuh file lokal (hapus setelah pindah) lewat `sx-ops` agar tercatat di journal.

## 6. Endpoint yang dipakai

Basis API: `https://www.googleapis.com`.

| Tujuan | Method & path | Catatan |
|---|---|---|
| Login | `GET https://accounts.google.com/o/oauth2/v2/auth` | `response_type=code`, `code_challenge` (S256), `state`, `access_type=offline`, `prompt=consent` saat connect pertama |
| Tukar/refresh token | `POST https://oauth2.googleapis.com/token` | `grant_type=authorization_code` + `code_verifier`; `grant_type=refresh_token` |
| Cabut akses | `POST https://oauth2.googleapis.com/revoke` | Saat Disconnect |
| Akun & kuota | `GET /drive/v3/about?fields=user(displayName,emailAddress),storageQuota(limit,usage,usageInDrive,usageInDriveTrash)` | `limit` kosong = tanpa batas |
| Isi folder | `GET /drive/v3/files?q='<id>' in parents and trashed=false&fields=nextPageToken,files(id,name,mimeType,size,modifiedTime,md5Checksum,shortcutDetails)&pageSize=1000&orderBy=folder,name_natural&supportsAllDrives=true&includeItemsFromAllDrives=true` | Paging via `nextPageToken` |
| Folder saja (popup) | query di atas + `and mimeType='application/vnd.google-apps.folder'` | |
| Cari nama | `q=name='<escaped>' and '<id>' in parents and trashed=false` | Escape `'` dan `\` |
| Buat folder | `POST /drive/v3/files` body `{name, mimeType:"application/vnd.google-apps.folder", parents:[id]}` | |
| Mulai upload | `POST /upload/drive/v3/files?uploadType=resumable&fields=id,name,size,md5Checksum` | Header `X-Upload-Content-Type`, `X-Upload-Content-Length`; `Location` = URI sesi |
| Kirim chunk | `PUT <URI sesi>` dengan `Content-Range: bytes a-b/total` | `308` = lanjut (header `Range`), `200/201` = selesai |
| Cek status sesi | `PUT <URI sesi>` dengan `Content-Range: bytes */total` | Untuk melanjutkan |
| Ganti (revisi baru) | `PATCH /upload/drive/v3/files/<id>?uploadType=resumable` | Opsi konflik "Ganti" |
| Download | `GET /drive/v3/files/<id>?alt=media` (+ `Range`) | |
| Ekspor Google Docs | `GET /drive/v3/files/<id>/export?mimeType=…` | Ada batas ukuran ekspor |
| Trash | `PATCH /drive/v3/files/<id>` body `{trashed:true}` | |

## 7. UX

### 7.1 Menghubungkan akun
Settings → Akun → "Hubungkan Google Drive" → browser terbuka → setelah berhasil, kartu akun menampilkan
email dan kuota; Drive muncul di sidebar dan Home. Bila Drive for desktop terdeteksi, tampilkan info bahwa
drive virtualnya sudah bisa dipakai tanpa koneksi API.

### 7.2 Upload / Pindahkan ke Drive
- Klik kanan → **Upload ke Drive ▸** berisi tujuan favorit (maks 5) + "Pilih folder…"; **Pindahkan ke
  Drive…** membuka popup yang sama dengan mode Pindahkan terpilih.
- Popup (wireframe di DESIGN.md §8.3): pohon "My Drive" (+ Shared drives jika ada) yang dimuat saat
  di-expand, breadcrumb, cari folder, **+ Folder baru** (dibuat langsung di Drive lalu otomatis terpilih),
  pilihan Salin/Pindahkan, kebijakan konflik, centang "Simpan sebagai tujuan favorit".
- Folder lokal diunggah dengan struktur lengkap (subfolder dibuat lebih dulu, termasuk folder kosong).
- Progress di panel Transfer + toast: nama file, n/total, MB/s, sisa waktu, pause/resume/cancel.
- Selesai: toast "3 file diunggah ke Laporan." [Buka di Drive] (membuka `webViewLink` di browser).

### 7.3 Browse Drive (`cloud.gdrive.browse`)
- Drive di sidebar seperti lokasi biasa; daftar dimuat per halaman saat dibuka (cache metadata LRU + TTL).
- Buka file = unduh ke `cloud-cache\` lalu buka dengan aplikasi default; file Google Docs/Sheets/Slides
  dibuka di browser atau diekspor (docx/xlsx/pdf).
- Drag file Drive ke folder lokal = download; drag file lokal ke folder Drive = upload (mengikuti Drag Guard).

### 7.4 Send To dari Windows Explorer (`integration.sendto_drive`)
Shortcut di folder `shell:sendto` menjalankan `switch-explorer.exe --upload-to-drive <file…>` → popup tujuan
muncul walau pengguna sedang memakai Explorer bawaan.

### 7.5 Nanti: picker + Drive
Di window mode picker (SYSTEM-INTEGRATION.md §13–14), memilih file Drive = unduh ke cache lalu path lokal
dikembalikan ke aplikasi peminta (mis. upload file Drive langsung ke WhatsApp).

## 8. Upload resumable & verifikasi

1. Rencana: hitung ukuran, buat folder tujuan bila perlu, cek konflik nama (list isi folder tujuan sekali
   lalu bandingkan).
2. Mulai sesi resumable → simpan URI sesi + offset di journal (bisa dilanjutkan setelah app restart; sesi
   berlaku sekitar seminggu).
3. Kirim chunk `cloud.transfer.chunk_mib` (default 8 MiB, kelipatan 256 KiB) **dialirkan langsung dari
   disk** dengan buffer kecil; MD5 dihitung inkremental. Retry chunk membaca ulang dari disk (tidak perlu
   menahan chunk di RAM).
4. Gagal jaringan → cek status sesi → lanjut dari offset yang dikonfirmasi server.
5. Selesai → bandingkan `md5Checksum` & `size` dari respons dengan nilai lokal.
6. Mode Pindahkan: hanya item yang terverifikasi yang dibuang ke Recycle Bin (via `sx-ops`, tercatat di
   journal). Gagal verifikasi → file lokal tetap, file remote dipindah ke trash, pengguna diberi tahu.
7. Undo "Pindahkan ke Drive" = pulihkan file lokal dari Recycle Bin (+ opsi memindah salinan Drive ke trash).

## 9. Konflik nama

Drive mengizinkan nama ganda dalam satu folder. Kebijakan: **Simpan keduanya** (default; diberi akhiran
" (1)" agar jelas), **Ganti** (revisi baru lewat `PATCH`, ID & pengaturan berbagi tetap), **Lewati**.

## 10. Error, retry, rate limit

| Kondisi | Tindakan |
|---|---|
| `429`, `403 rateLimitExceeded/userRateLimitExceeded`, `5xx` | Exponential backoff + full jitter (1 s, maks 64 s, maks 8 kali) |
| `401` | Refresh token sekali, ulangi |
| `invalid_grant` saat refresh | Akun perlu dihubungkan ulang → banner di Settings & sidebar |
| `403 storageQuotaExceeded` | Hentikan antrean, tampilkan kuota & sarannya |
| Offline | Jeda antrean, lanjut otomatis saat online |
| File lokal berubah saat upload | Batalkan item, tandai "berubah selama upload", tawarkan ulang |

## 11. Memori & performa

| Komponen | Perkiraan | Kapan ada |
|---|---|---|
| Runtime tokio (2 thread) + klien HTTP rustls | beberapa MB | Hanya saat cloud dipakai; dimatikan setelah idle `cloud.runtime_idle_shutdown_s` |
| Cache metadata | ~1 MB per 2000 item | Saat browse; LRU + TTL 5 menit |
| Buffer upload | ~64 KiB × concurrency | Saat transfer |

Modul tidak dimuat sama sekali sampai pertama kali dipakai. `cloud.enabled = false` menghilangkan menu dan
sidebar Drive sepenuhnya.

## 12. Deteksi Google Drive for desktop

Periksa volume berlabel "Google Drive" dan konfigurasi DriveFS di profil pengguna (verifikasi lokasi registry
per versi saat implementasi). Bila ada, tampilkan di Settings → Akun dan jangan memaksa koneksi API.

## 13. Keamanan

Detail di `SECURITY.md`. Ringkasnya: refresh token hanya di Credential Manager
(`SwitchExplorer/gdrive/<email>`), access token hanya di memori, tidak ada token di log (layer redaksi),
`state` + PKCE diverifikasi, loopback hanya `127.0.0.1`, client secret tidak di-commit.

## 14. Provider lain (setelah v1)

Trait `CloudProvider` dibuat umum: OneDrive (Microsoft Graph, upload session), Dropbox (API v2), dan opsi
jembatan **rclone** (`rclone rcd` + RC API, opt-in) untuk puluhan provider lain termasuk MEGA.

## 15. Checklist implementasi

- [ ] `auth.rs`: PKCE, server loopback, simpan/muat/hapus kredensial, refresh otomatis, revoke.
- [ ] `about` → kartu akun & gauge kuota di Home/sidebar.
- [ ] Popup tujuan: pohon lazy, buat folder, favorit, konflik.
- [ ] Upload resumable + MD5 + resume setelah restart + verifikasi + mode Pindahkan via `sx-ops`.
- [ ] Panel Transfer: pause/resume/cancel/retry, concurrency, batas bandwidth.
- [ ] Browse sidebar + buka/unduh + drag dua arah (Drag Guard berlaku).
- [ ] Runtime lazy + idle shutdown + memory meter menunjukkan 0 saat idle.
- [ ] Test dengan server mock (wiremock) untuk semua jalur error di §10.
