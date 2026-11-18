// Sembunyikan jendela konsol di Windows, debug maupun release.
// (Dulu hanya release, jadi `cargo run` atau build debug membuka terminal.)
#![cfg_attr(windows, windows_subsystem = "windows")]

mod innertube;
mod player;
mod stream;

fn main() {
    tauri::Builder::default()
        .manage(player::Player::default())
        .invoke_handler(tauri::generate_handler![
            search,
            resolve,
            stream::setup,
            player::flac_play,
            player::flac_pause,
            player::flac_resume,
            player::flac_seek,
            player::flac_status,
            player::flac_volume,
            player::flac_stop,
        ])
        .run(tauri::generate_context!())
        .expect("RustMusic gagal start");
}

/// Cari lagu di YouTube Music.
#[tauri::command]
async fn search(query: String) -> Result<Vec<innertube::Song>, String> {
    innertube::search_songs(&query).await
}

/// Ambil URL audio untuk satu lagu.
#[tauri::command]
async fn resolve(video_id: String) -> Result<String, String> {
    stream::audio_url(&video_id).await
}
