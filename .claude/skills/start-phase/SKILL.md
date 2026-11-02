---
name: start-phase
description: Mulai atau lanjutkan satu fase roadmap Switch Explorer. Dipakai saat pengguna meminta mengerjakan fase tertentu.
disable-model-invocation: true
---
# Mulai fase $ARGUMENTS

1. Baca bagian **Fase $ARGUMENTS** di `docs/ROADMAP.md`, lalu dokumen yang dirujuk acceptance criteria-nya
   (gunakan tabel "Peta dokumen" di CLAUDE.md). Jangan memuat dokumen yang tidak relevan.
2. Periksa kondisi repo: apa yang sudah ada, test yang ada, status `cargo check`.
3. Tulis rencana kerja bernomor: urutan langkah kecil, file/crate yang disentuh, cara verifikasi tiap langkah,
   risiko. Tandai item acceptance criteria yang dicakup tiap langkah. Minta persetujuan sebelum mulai.
4. Setelah disetujui: kerjakan per langkah, `cargo check` setelah tiap langkah, tambah test, jalankan
   fmt + clippy + test.
5. Akhiri dengan: daftar acceptance criteria yang sudah/ belum tercentang, langkah verifikasi manual untuk
   pengguna, dan usulan langkah berikutnya. Perbarui baris "Status" di CLAUDE.md bila fase selesai.
