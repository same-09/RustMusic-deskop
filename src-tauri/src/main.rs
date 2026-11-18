// Sembunyikan jendela konsol di Windows, debug maupun release.
// (Dulu hanya release, jadi `cargo run` atau build debug membuka terminal.)
#![cfg_attr(windows, windows_subsystem = "windows")]

mod desktop;
mod innertube;
mod install;
mod lyrics;
mod offline;
mod player;
mod recognize;
mod stream;

use tauri::Manager;

fn main() {
    if std::env::args().any(|a| a == "--uninstall") {
        install::uninstall();
        return;
    }
    tauri::Builder::default()
        .plugin(desktop::media_plugin())
        .manage(player::Player::default())
        .setup(|app| {
            // Lagu terunduh diputar lewat protokol aset: izinkan folder musik saja.
            // Pastikan jendela muncul di tengah layar.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.center();
            }
            let music = offline::music_dir();
            let _ = std::fs::create_dir_all(&music);
            let _ = app.asset_protocol_scope().allow_directory(&music, true);
            desktop::register_media_keys(app.handle());
            if let Err(e) = desktop::setup_tray(app.handle()) {
                eprintln!("tray gagal dibuat: {e}");
            }
            Ok(())
        })
        .on_window_event(desktop::on_window_event)
        .invoke_handler(tauri::generate_handler![
            search,
            search_entries,
            browse,
            resolve,
            resolve_slow,
            prefetch,
            lyrics::lyrics,
            stream::setup,
            install::install_state,
            install::install_app,
            player::flac_play,
            player::flac_pause,
            player::flac_resume,
            player::flac_seek,
            player::flac_status,
            player::flac_volume,
            player::flac_stop,
            offline::download_song,
            offline::list_downloads,
            offline::delete_download,
            recognize::recognize,
            desktop::set_media_keys,
            desktop::set_close_to_tray,
            desktop::set_tooltip,
        ])
        .run(tauri::generate_context!())
        .expect("RustMusic gagal start");
}

/// Cari lagu di YouTube Music.
#[tauri::command]
async fn search(query: String) -> Result<Vec<innertube::Song>, String> {
    innertube::search_songs(&query).await
}

/// Cari artis ("artists") atau album ("albums").
#[tauri::command]
async fn search_entries(query: String, kind: String) -> Result<Vec<innertube::Entry>, String> {
    let kind = innertube::Kind::parse(&kind).ok_or("jenis pencarian tidak dikenal")?;
    innertube::search_entries(&query, kind).await
}

/// Halaman artis (id "UC...") atau album (id "MPREb_...").
#[tauri::command]
async fn browse(browse_id: String) -> Result<innertube::Page, String> {
    innertube::browse(&browse_id).await
}

/// Ambil URL audio untuk satu lagu.
#[tauri::command]
async fn resolve(video_id: String) -> Result<String, String> {
    stream::audio_url(&video_id).await
}

/// Sama seperti `resolve`, tapi paksa yt-dlp (cadangan bila URL Innertube gagal diputar).
#[tauri::command]
async fn resolve_slow(video_id: String) -> Result<String, String> {
    stream::audio_url_slow(&video_id).await
}

/// Siapkan URL audio lagu berikutnya di latar belakang (hasil masuk cache).
#[tauri::command]
async fn prefetch(video_id: String) {
    let _ = stream::audio_url(&video_id).await;
}
