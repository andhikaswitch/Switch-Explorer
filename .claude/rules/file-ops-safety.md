---
paths:
  - "crates/sx-ops/**"
  - "crates/sx-fs/**"
  - "crates/sx-tools/**"
  - "crates/sx-cloud/**"
---
# Aturan keselamatan data

- Delete default ke Recycle Bin; hapus permanen hanya dengan konfirmasi eksplisit.
- Tidak pernah menimpa tanpa keputusan pengguna; konflik selalu lewat kebijakan yang dipilih.
- Journal write-ahead (sebelum & sesudah tiap item); semua operasi bisa di-undo dan dipulihkan setelah crash.
- Move antar volume = copy → verifikasi → hapus sumber. Pindah ke Drive = upload → cek checksum → baru
  buang lokal ke Recycle Bin.
- Hormati `safety.readonly_mode` dan `safety.protected_paths` di lapisan ini, bukan hanya di UI.
- Validasi nama file & cegah path traversal (template Organizer/Bulk Rename tidak boleh keluar folder tujuan).
- Cancel selalu dihormati; operasi panjang melaporkan progress (≤ 10 Hz) dan bisa di-pause bila memungkinkan.
- Test memakai folder sementara (`tempfile`), tidak pernah folder pengguna nyata.
- Rujukan: `docs/SECURITY.md` §2, `docs/FEATURES.md` (F-OPS), `docs/CLOUD-GOOGLE-DRIVE.md` §8.
