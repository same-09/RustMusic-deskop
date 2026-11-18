//! Unduhan offline: simpan audio lagu ke `%LOCALAPPDATA%\RustMusic\music\<id>.<ext>` lewat yt-dlp.
//! UI memutar file itu lewat protokol aset Tauri, jadi lagu terunduh jalan tanpa internet.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

use crate::stream;

/// Ekstensi audio yang dianggap file lagu jadi (bukan sisa unduhan seperti .part).
const AUDIO_EXT: [&str; 9] = ["webm", "m4a", "mp3", "opus", "ogg", "aac", "flac", "wav", "mp4"];

pub fn music_dir() -> PathBuf {
    stream::data_dir().join("music")
}

fn valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[derive(Serialize, Clone)]
struct Progress {
    id: String,
    percent: f32,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Downloaded {
    id: String,
    path: String,
}

/// "[download]  45.3% of 3.45MiB at ..." -> 45.3
fn parse_percent(line: &str) -> Option<f32> {
    let rest = line.strip_prefix("[download]")?.trim_start();
    let (num, _) = rest.split_once('%')?;
    num.trim().parse().ok()
}

/// Id lagu dari nama file jadi ("abc123.webm" -> "abc123"). `None` untuk sisa unduhan.
fn id_of(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !AUDIO_EXT.contains(&ext.as_str()) {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    valid(stem).then(|| stem.to_string())
}

fn scan(dir: &Path) -> Vec<Downloaded> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Downloaded> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            let id = id_of(&p)?;
            Some(Downloaded { id, path: p.to_string_lossy().into_owned() })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn find_file(id: &str) -> Option<String> {
    scan(&music_dir()).into_iter().find(|d| d.id == id).map(|d| d.path)
}

/// Unduh audio terbaik satu lagu. Kirim event `download-progress` {id, percent}. Hasil: path file.
#[tauri::command]
pub async fn download_song(app: AppHandle, video_id: String) -> Result<String, String> {
    if !valid(&video_id) {
        return Err("id lagu tidak valid".into());
    }
    if let Some(p) = find_file(&video_id) {
        return Ok(p);
    }
    let dir = music_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("buat folder musik gagal: {e}"))?;
    let bin = stream::ytdlp().await?;
    let mut cmd = Command::new(&bin);
    cmd.args(["-f", "bestaudio", "--no-playlist", "--no-warnings", "--newline", "--no-mtime"])
        .arg("-P")
        .arg(&dir)
        .arg("-o")
        .arg(format!("{video_id}.%(ext)s"))
        .arg(format!("https://music.youtube.com/watch?v={video_id}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    stream::quiet(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("yt-dlp tidak jalan: {e}"))?;
    let stdout = child.stdout.take().ok_or("tidak bisa baca keluaran yt-dlp")?;
    let stderr = child.stderr.take().ok_or("tidak bisa baca galat yt-dlp")?;
    // Baca stderr di tugas lain supaya pipa tidak penuh dan yt-dlp macet.
    let err_task = tokio::spawn(async move {
        let mut s = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut s).await;
        s
    });
    let mut lines = BufReader::new(stdout).lines();
    let mut last = -1.0_f32;
    let _ = app.emit("download-progress", Progress { id: video_id.clone(), percent: 0.0 });
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(p) = parse_percent(&line) {
            if p - last >= 1.0 || p >= 100.0 {
                last = p;
                let _ = app.emit("download-progress", Progress { id: video_id.clone(), percent: p });
            }
        }
    }
    let status = child.wait().await.map_err(|e| format!("yt-dlp gagal: {e}"))?;
    let err = err_task.await.unwrap_or_default();
    if !status.success() {
        let msg = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unduh gagal");
        return Err(msg.trim().to_string());
    }
    find_file(&video_id).ok_or_else(|| "file hasil unduhan tidak ditemukan".to_string())
}

/// Semua lagu yang sudah terunduh (hasil pindai folder musik).
#[tauri::command]
pub fn list_downloads() -> Vec<Downloaded> {
    scan(&music_dir())
}

/// Hapus file unduhan satu lagu (dan sisa .part bila ada).
#[tauri::command]
pub fn delete_download(video_id: String) -> Result<(), String> {
    if !valid(&video_id) {
        return Err("id lagu tidak valid".into());
    }
    let dir = music_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else { return Ok(()) };
    let prefix = format!("{video_id}.");
    for e in rd.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) {
            std::fs::remove_file(e.path()).map_err(|err| format!("hapus gagal (sedang diputar?): {err}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_percent() {
        assert_eq!(parse_percent("[download]  45.3% of 3.45MiB at 1.2MiB/s ETA 00:02"), Some(45.3));
        assert_eq!(parse_percent("[download] 100% of 3.45MiB in 00:03"), Some(100.0));
        assert_eq!(parse_percent("[download] Destination: x.webm"), None);
        assert_eq!(parse_percent("[info] abc"), None);
    }

    #[test]
    fn only_finished_audio_files_count() {
        assert_eq!(id_of(Path::new("/m/abc-_1.webm")), Some("abc-_1".into()));
        assert_eq!(id_of(Path::new("/m/abc.m4a")), Some("abc".into()));
        assert_eq!(id_of(Path::new("/m/abc.webm.part")), None);
        assert_eq!(id_of(Path::new("/m/abc.f251.webm")), None);
        assert_eq!(id_of(Path::new("/m/abc.txt")), None);
    }

    #[test]
    fn scan_and_delete_roundtrip() {
        let dir = std::env::temp_dir().join(format!("rm-offline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("aaa.webm"), b"x").unwrap();
        std::fs::write(dir.join("bbb.webm.part"), b"x").unwrap();
        let got = scan(&dir);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "aaa");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
