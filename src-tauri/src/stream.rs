use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::process::Command;

/// Jalur yt-dlp yang sudah terbukti jalan (disimpan supaya tidak dicari lagi).
static YTDLP: Mutex<Option<PathBuf>> = Mutex::new(None);

#[cfg(windows)]
const DOWNLOAD_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
#[cfg(not(windows))]
const DOWNLOAD_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp";

/// Simpan URL audio hasil yt-dlp, supaya lagu yang sama tidak diminta lagi (yt-dlp lambat).
static URL_CACHE: Mutex<Option<HashMap<String, (Instant, String)>>> = Mutex::new(None);
const URL_TTL: Duration = Duration::from_secs(3 * 3600);

fn url_get(id: &str) -> Option<String> {
    let g = URL_CACHE.lock().ok()?;
    let (t, u) = g.as_ref()?.get(id)?;
    if t.elapsed() < URL_TTL { Some(u.clone()) } else { None }
}

fn url_put(id: &str, url: &str) {
    if let Ok(mut g) = URL_CACHE.lock() {
        let m = g.get_or_insert_with(HashMap::new);
        if m.len() > 64 {
            m.retain(|_, v| v.0.elapsed() < URL_TTL);
            if m.len() > 64 {
                m.clear();
            }
        }
        m.insert(id.to_string(), (Instant::now(), url.to_string()));
    }
}

fn cached() -> Option<PathBuf> {
    YTDLP.lock().ok().and_then(|g| g.clone())
}

fn remember(p: &PathBuf) {
    if let Ok(mut g) = YTDLP.lock() {
        *g = Some(p.clone());
    }
}

/// Jangan munculkan jendela cmd hitam di Windows.
pub(crate) fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

async fn works(path: &OsStr) -> bool {
    let mut c = Command::new(path);
    c.arg("--version");
    quiet(&mut c);
    matches!(c.output().await, Ok(o) if o.status.success())
}

/// Folder data RustMusic: %LOCALAPPDATA%\RustMusic, atau folder app bila tidak ada.
pub(crate) fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("RustMusic"))
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|p| p.to_path_buf()))
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Cari yt-dlp yang sudah ada: PATH, sebelah app, folder data. Tidak mengunduh.
async fn find() -> Option<PathBuf> {
    if let Some(p) = cached() {
        return Some(p);
    }
    let mut cands: Vec<PathBuf> = vec![PathBuf::from("yt-dlp")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            cands.push(d.join("yt-dlp.exe"));
        }
    }
    cands.push(data_dir().join("yt-dlp.exe"));
    for c in &cands {
        if works(c.as_os_str()).await {
            remember(c);
            return Some(c.clone());
        }
    }
    None
}

/// Unduh yt-dlp resmi dari GitHub (sekali saja). `progress(sudah, total)` dipanggil tiap potongan.
#[cfg_attr(not(windows), allow(dead_code))]
async fn download(
    dir: &std::path::Path,
    progress: impl Fn(u64, Option<u64>),
) -> Result<PathBuf, String> {
    let err = |e: reqwest::Error| format!("unduh yt-dlp gagal: {e}");
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(err)?;
    let mut resp = client
        .get(DOWNLOAD_URL)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    let total = resp.content_length();
    let mut bytes: Vec<u8> = Vec::with_capacity(total.unwrap_or(0) as usize);
    progress(0, total);
    while let Some(chunk) = resp.chunk().await.map_err(err)? {
        bytes.extend_from_slice(&chunk);
        progress(bytes.len() as u64, total);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("buat folder gagal: {e}"))?;
    let tmp = dir.join("yt-dlp.part");
    let dst = dir.join("yt-dlp.exe");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("simpan yt-dlp gagal: {e}"))?;
    std::fs::rename(&tmp, &dst).map_err(|e| format!("simpan yt-dlp gagal: {e}"))?;
    Ok(dst)
}

/// Pastikan yt-dlp ada. Kalau belum, unduh (Windows) dan lapor kemajuan lewat `progress`.
async fn ensure(progress: impl Fn(u64, Option<u64>)) -> Result<PathBuf, String> {
    if let Some(p) = find().await {
        return Ok(p);
    }
    #[cfg(windows)]
    {
        let p = download(&data_dir(), progress).await?;
        if works(p.as_os_str()).await {
            remember(&p);
            return Ok(p);
        }
        return Err("yt-dlp sudah diunduh tapi tidak bisa jalan".into());
    }
    #[cfg(not(windows))]
    {
        let _ = progress;
        Err("yt-dlp tidak ada di PATH. Pasang yt-dlp dulu.".into())
    }
}

/// Dipakai saat putar lagu: kalau setup awal terlewat, tetap unduh diam-diam.
pub(crate) async fn ytdlp() -> Result<PathBuf, String> {
    ensure(|_, _| {}).await
}

#[derive(Serialize, Clone)]
pub struct SetupProgress {
    /// "cek" | "unduh" | "selesai"
    stage: &'static str,
    done: u64,
    total: Option<u64>,
}

/// Setup awal: cek yt-dlp, unduh bila belum ada. Event UI: `setup-progress`.
#[tauri::command]
pub async fn setup(app: AppHandle) -> Result<(), String> {
    let emit = |stage: &'static str, done: u64, total: Option<u64>| {
        let _ = app.emit("setup-progress", SetupProgress { stage, done, total });
    };
    emit("cek", 0, None);
    // Batasi ~12x per detik supaya UI tidak dibanjiri event.
    let last = Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(1));
    ensure(|done, total| {
        let mut t = last.lock().unwrap_or_else(|e| e.into_inner());
        let finished = total.is_some_and(|n| done >= n);
        if finished || t.elapsed() >= std::time::Duration::from_millis(80) {
            *t = std::time::Instant::now();
            emit("unduh", done, total);
        }
    })
    .await?;
    emit("selesai", 0, None);
    Ok(())
}

/// URL audio terbaik: ambil dari cache kalau ada, kalau belum tanya yt-dlp.
pub async fn audio_url(video_id: &str) -> Result<String, String> {
    if let Some(u) = url_get(video_id) {
        return Ok(u);
    }
    check_id(video_id)?;
    // Jalur cepat: Innertube langsung. Kalau gagal, baru yt-dlp.
    let u = match crate::innertube::audio_url(video_id).await {
        Ok(u) => u,
        Err(_) => audio_url_fresh(video_id).await?,
    };
    url_put(video_id, &u);
    Ok(u)
}

/// Paksa yt-dlp (dipakai UI kalau URL Innertube gagal diputar).
pub async fn audio_url_slow(video_id: &str) -> Result<String, String> {
    check_id(video_id)?;
    let u = audio_url_fresh(video_id).await?;
    url_put(video_id, &u);
    Ok(u)
}

fn check_id(video_id: &str) -> Result<(), String> {
    if video_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        Ok(())
    } else {
        Err("id lagu tidak valid".into())
    }
}

/// Minta yt-dlp untuk URL audio terbaik (lambat, beberapa detik).
async fn audio_url_fresh(video_id: &str) -> Result<String, String> {
    if !video_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("id lagu tidak valid".into());
    }
    let bin = ytdlp().await?;
    let mut cmd = Command::new(&bin);
    cmd.args(["-g", "-f", "bestaudio", "--no-playlist", "--no-warnings"])
        .arg(format!("https://music.youtube.com/watch?v={video_id}"));
    quiet(&mut cmd);
    let out = cmd
        .output()
        .await
        .map_err(|e| format!("yt-dlp tidak jalan: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(str::to_string)
        .ok_or_else(|| "yt-dlp tidak kasih URL".into())
}
