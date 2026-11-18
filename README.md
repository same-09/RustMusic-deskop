<!-- markdownlint-disable MD033 MD041 -->

> [!NOTE]
> RustMusic bukan produk resmi YouTube atau Google.
> Satu-satunya sumber resmi adalah repo ini. Salinan di tempat lain tidak kami jamin.

<p align="center">
  <img src="ui/logo/app.png" alt="RustMusic" width="120">
</p>

<h1 align="center">RustMusic</h1>

<p align="center">
  <b>Pemutar YouTube Music untuk Windows, ditulis dengan Rust.</b><br>
  Satu file <code>.exe</code>. Tanpa JVM, tanpa terminal. Start cepat.
</p>

<p align="center">
  <a href="https://github.com/same-09/RustMusic-deskop/releases/latest"><img alt="release" src="https://img.shields.io/github/v/release/same-09/RustMusic-deskop?label=release&color=ff3b3b&labelColor=3a3a3a"></a>
  <a href="https://github.com/same-09/RustMusic-deskop/stargazers"><img alt="stars" src="https://img.shields.io/github/stars/same-09/RustMusic-deskop?style=flat&color=ff7a45&labelColor=3a3a3a"></a>
  <img alt="platform" src="https://img.shields.io/badge/platform-Windows%2010%2B-ff3b3b?labelColor=3a3a3a">
  <img alt="rust" src="https://img.shields.io/badge/made%20with-Rust-ff3b3b?labelColor=3a3a3a">
  <img alt="license" src="https://img.shields.io/badge/license-GPL--3.0-ff3b3b?labelColor=3a3a3a">
</p>

<p align="center">
  <a href="https://github.com/same-09/RustMusic-deskop/releases/latest">
    <img alt="Unduh RustMusic" src="https://img.shields.io/badge/%E2%86%93%20UNDUH%20RUSTMUSIC-3a3a3a?style=for-the-badge"><img alt="Rilis terbaru" src="https://img.shields.io/badge/RILIS%20TERBARU-ff3b3b?style=for-the-badge">
  </a>
</p>

---

> [!WARNING]
> **Status: awal.** Kode baru (update dalam app, jalur cepat Innertube, progress titik) belum pernah
> di-compile atau dijalankan di Windows. Mesin FLAC sudah dites di Linux, tapi belum ada tes bunyi
> di kartu suara asli. Build lewat GitHub Actions. Error kecil mungkin muncul, dan laporannya sangat membantu.

## Isi

- [Fitur](#fitur)
- [Pasang](#pasang)
- [Update](#update)
- [Cara kerja pemutaran](#cara-kerja-pemutaran)
- [Kenali musik](#kenali-musik)
- [Build dari source](#build-dari-source)
- [Lokasi data](#lokasi-data)
- [Lisensi](#lisensi)

## Fitur

**Putar**
- Cari lagu, artis, dan album di YouTube Music. Halaman artis dan album lengkap.
- Lagu pertama cepat: URL audio diambil langsung lewat Innertube, `yt-dlp` hanya cadangan. Lagu berikutnya disiapkan lebih dulu.
- Mesin FLAC lossless: putar pada sample rate dan bit depth asli, mulai saat unduhan masih jalan, seek, dither 24 ke 16 bit bila kartu suara hanya 16 bit, volume lunak.
- Ekualiser 10 band dan lirik.

**Pustaka**
- Lagu disukai, riwayat, statistik, dan playlist buatan sendiri.
- Unduhan offline, diputar tanpa internet.
- Kenali musik lewat mikrofon.

**Desktop**
- Ikon tray, tutup ke tray, dan tombol media global (putar/jeda, berikutnya, sebelumnya).
- Tema sistem, terang, atau gelap.
- Setup bawaan: pasang ke komputer dan buat shortcut. Hapus bersih lewat Apps & Features.
- Update langsung di dalam app, tanpa browser.

## Pasang

1. Unduh `RustMusic.exe` dari [Releases](https://github.com/same-09/RustMusic-deskop/releases/latest).
2. Klik dua kali. Layar **Setup** muncul: pilih **Setup** untuk memasang dan membuat shortcut, atau **Jalankan tanpa pasang**.
3. Saat pertama dibuka, app mengunduh `yt-dlp.exe` sekali saja (ada progress titik, tombol *Coba lagi* dan *Lewati* bila gagal).

Windows 10 atau lebih baru. WebView2 sudah ada di Windows 10 dan 11.

**Hapus RustMusic**, dua cara, hasilnya sama (hapus app, shortcut, dan entri uninstall):

1. Pengaturan Windows > Apps > RustMusic > Uninstall
2. Terminal: `RustMusic.exe --uninstall`

## Update

Saat start dan tiap 6 jam, RustMusic cek rilis di GitHub. Kalau ada versi lebih baru, dialog **Update** muncul dengan catatan rilis.

- **Update sekarang** mengunduh `RustMusic.exe` baru (progress titik), memeriksa ukuran dan header file, mengganti exe yang jalan, lalu membuka ulang. Tanpa browser.
- **Nanti** menutup dialog sampai app dibuka lagi.
- Cek manual: **Pengaturan > Cek update**.

Hanya Windows. Build dev (`cargo run`) tidak bisa update sendiri.

## Cara kerja pemutaran

```
klik lagu
  └─ cache URL?  ── ya ──► putar
        │ tidak
        ├─ Innertube (satu request, klien ANDROID_VR) ──► putar
        └─ gagal ──► yt-dlp ──► putar
                                 └─ URL ditolak saat putar? ulangi lewat yt-dlp
```

URL audio disimpan di cache, dan lagu berikutnya di antrean dicari di latar belakang. Itu sebabnya lagu kedua dan seterusnya mulai hampir langsung.

## Kenali musik

<details>
<summary><b>Cara pakai</b></summary>

Halaman **Kenali musik** merekam beberapa detik lewat mikrofon, lalu mencocokkannya di layanan [AudD](https://audd.io).

1. Daftar di dashboard.audd.io dan ambil token API.
2. Isi token di **Pengaturan > Token AudD**. Token disimpan di komputer ini.
3. Izinkan akses mikrofon untuk app di Windows (Pengaturan > Privasi > Mikrofon).
4. Tekan tombol mikrofon.

Rekaman dikirim ke AudD untuk dicocokkan, tidak ke tempat lain.

</details>

## Build dari source

Butuh Rust (rustup) dengan toolchain MSVC.

```powershell
cargo test --workspace   # tes mesin FLAC dan parser
cargo run -p rustmusic   # jalan
cargo install tauri-cli --version "^2"
cd src-tauri
cargo tauri build        # buat MSI + NSIS
```

Linux butuh `libasound2-dev` untuk `cpal`, dan `libwebkit2gtk-4.1-dev` untuk Tauri.

**Rilis.** Buat tag `vX.Y.Z`. GitHub Actions membangun dan melampirkan `RustMusic.exe` ke rilis, dan versi app disamakan dengan tag. Updater mencari nama file itu dan mengabaikan tag lain (misalnya `build-12` dari tombol manual).

<details>
<summary><b>Peta kode</b></summary>

```
engine/                  mesin FLAC (crate rustmusic-engine), keluaran lewat trait PcmOut
src-tauri/src/main.rs    pintu masuk dan daftar perintah
src-tauri/src/innertube.rs  cari lagu/artis/album, ambil URL audio langsung
src-tauri/src/stream.rs  yt-dlp (cadangan), cache URL, setup awal
src-tauri/src/player.rs  perintah flac_* dan event pemutar
src-tauri/src/offline.rs unduhan offline
src-tauri/src/lyrics.rs  lirik
src-tauri/src/recognize.rs  Kenali musik (AudD)
src-tauri/src/desktop.rs tray, tombol media, tutup ke tray
src-tauri/src/install.rs pasang, shortcut, uninstall
src-tauri/src/update.rs  cek rilis GitHub, unduh, ganti exe sendiri
ui/                      HTML + CSS + JS tanpa framework (index.html, style.css, app.js)
assets/po_token.html     pembuat PO token dari app Android lama, untuk port Rust
```

</details>

## Lokasi data

| Apa | Di mana |
|---|---|
| App terpasang | `%LOCALAPPDATA%\Programs\RustMusic\` |
| Unduhan offline | `%LOCALAPPDATA%\RustMusic\music\` |
| `yt-dlp.exe` | `%LOCALAPPDATA%\RustMusic\` (atau di PATH, atau sebelah app) |
| Playlist, suka, pengaturan | `localStorage` WebView2 |

## Lisensi

GPL-3.0. Lihat `LICENSE` dan `THIRD_PARTY_NOTICES.md`.
