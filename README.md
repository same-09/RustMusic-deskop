# RustMusic

Fork dari OpenTune (justshivv). Lisensi GPL-3.0, lihat `LICENSE` dan `THIRD_PARTY_NOTICES.md`.

Inti: Rust (Tauri 2). UI: HTML + CSS + JS. Python: yt-dlp untuk URL audio.
Tidak ada JVM, jadi start cepat.

## Syarat (Windows)

- Rust (rustup) dengan toolchain MSVC
- WebView2 (sudah ada di Windows 10/11)
- `yt-dlp.exe`: dicari di PATH, sebelah app, atau `%LOCALAPPDATA%\RustMusic`. Kalau tidak ada, layar setup muncul saat app dibuka dan mengunduh sendiri sekali saja dari GitHub (ada bar kemajuan, tombol Coba lagi / Lewati bila gagal).

## Isi

- `engine/`: mesin FLAC (crate `rustmusic-engine`). Putar lossless pada rate dan bit depth asli, mulai saat unduhan masih jalan, seek, dither 24 ke 16 bit bila kartu suara hanya 16 bit, volume lunak. Keluaran lewat trait `PcmOut`; `CpalOut` untuk kartu suara (shared mode).
- `src-tauri/`: app Tauri. Perintah `setup` (event `setup-progress`), `search`, `resolve`, dan `flac_*` (event `flac-started`, `flac-ended`, `flac-error`). Jendela konsol disembunyikan di Windows, debug maupun release.
- `ui/`: HTML + CSS + JS tanpa framework. `index.html` (kerangka + ikon), `style.css` (tema terang/gelap), `app.js` (layar, pemutar, ekualiser). Juga `i18n/` (35 bahasa, belum dipakai), `logo/`, `font/`.
- `assets/po_token.html`: halaman pembuat PO token dari app Android lama, untuk dipakai port Rust.

## Jalankan

```powershell
cargo test --workspace   # tes mesin FLAC dan parser
cargo run -p rustmusic   # jalan
cargo install tauri-cli --version "^2"
cd src-tauri
cargo tauri build        # buat MSI + NSIS
```

Linux butuh `libasound2-dev` untuk `cpal`, dan `libwebkit2gtk-4.1-dev` untuk Tauri.

## Status build

Dikompilasi dan dites di Linux dengan Rust 1.100: `cargo check -p rustmusic` lolos dan 10 tes mesin FLAC lolos
(16 bit noise dan 24 bit 96 kHz sama persis dengan decoder `claxon`, seek, unduhan lambat, dither, volume, dispose).
Belum dites di Windows dan belum ada tes bunyi di kartu suara asli.
