//! Update dalam app: cek rilis GitHub, unduh RustMusic.exe baru, ganti exe, buka ulang.
//! Tanpa browser. Hanya mengambil file dari repo di `REPO`.
#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

const REPO: &str = "same-09/RustMusic-deskop";
const ASSET: &str = "RustMusic.exe";

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct UpdateInfo {
    pub version: String,
    pub tag: String,
    pub notes: String,
    pub url: String,
    pub size: u64,
}

#[derive(Serialize, Clone)]
struct UpdateProgress {
    done: u64,
    total: Option<u64>,
}

/// "v1.2.3" -> (1, 2, 3). Tag bukan versi (mis. "build-12") jadi None.
fn parse_ver(tag: &str) -> Option<(u64, u64, u64)> {
    let t = tag.trim().trim_start_matches(|c: char| c == 'v' || c == 'V');
    let core = t.split(|c: char| c == '-' || c == '+').next()?;
    let mut it = core.split('.');
    let part = |s: Option<&str>| -> Option<u64> {
        match s {
            None => Some(0),
            Some(x) => x.parse().ok(),
        }
    };
    let a: u64 = it.next()?.parse().ok()?;
    let b = part(it.next())?;
    let c = part(it.next())?;
    Some((a, b, c))
}

/// Rilis terbaru (versi tertinggi) yang punya RustMusic.exe. Draft dan pra-rilis dilewati.
fn pick_latest(releases: &Value) -> Option<UpdateInfo> {
    releases
        .as_array()?
        .iter()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false) && !r["prerelease"].as_bool().unwrap_or(false))
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            let ver = parse_ver(tag)?;
            let asset = r["assets"]
                .as_array()?
                .iter()
                .find(|a| a["name"].as_str().map_or(false, |n| n.eq_ignore_ascii_case(ASSET)))?;
            let url = asset["browser_download_url"].as_str()?.to_string();
            let notes = r["body"].as_str().unwrap_or("").trim().to_string();
            Some((
                ver,
                UpdateInfo {
                    version: format!("{}.{}.{}", ver.0, ver.1, ver.2),
                    tag: tag.to_string(),
                    notes,
                    url,
                    size: asset["size"].as_u64().unwrap_or(0),
                },
            ))
        })
        .max_by_key(|(v, _)| *v)
        .map(|(_, u)| u)
}

fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Versi app yang sedang jalan.
#[tauri::command]
pub fn app_version() -> String {
    current_version().to_string()
}

/// Cek rilis GitHub. Some(info) bila ada versi lebih baru dari yang jalan.
#[tauri::command]
pub async fn check_update() -> Result<Option<UpdateInfo>, String> {
    let err = |e: reqwest::Error| format!("cek update gagal: {e}");
    let client = reqwest::Client::builder()
        .user_agent(format!("RustMusic/{}", current_version()))
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(err)?;
    let res: Value = client
        .get(format!("https://api.github.com/repos/{REPO}/releases?per_page=30"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json()
        .await
        .map_err(err)?;
    let cur = parse_ver(current_version());
    Ok(pick_latest(&res).filter(|u| match (parse_ver(&u.tag), cur) {
        (Some(new), Some(now)) => new > now,
        _ => false,
    }))
}

/// Unduh exe baru, ganti exe yang jalan, buka ulang. Event UI: `update-progress`.
#[tauri::command]
pub async fn apply_update(app: AppHandle, url: String) -> Result<(), String> {
    if !url.starts_with(&format!("https://github.com/{REPO}/")) {
        return Err("alamat update tidak dikenal".into());
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        return Err("Update dalam app hanya untuk Windows".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let cur = std::env::current_exe().map_err(|e| e.to_string())?;
        if cur.to_string_lossy().to_lowercase().contains("\\target\\") {
            return Err("build dev tidak bisa update sendiri".into());
        }
        let new = cur.with_file_name("RustMusic.update.exe");
        let old = cur.with_extension("old");

        if let Err(e) = download(&url, &new, |done, total| {
            let _ = app.emit("update-progress", UpdateProgress { done, total });
        })
        .await
        {
            let _ = std::fs::remove_file(&new);
            return Err(e);
        }

        // Exe yang jalan tidak bisa ditimpa, tapi bisa diganti nama.
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&cur, &old).map_err(|e| format!("ganti file gagal: {e}"))?;
        if let Err(e) = std::fs::rename(&new, &cur) {
            let _ = std::fs::rename(&old, &cur); // kembalikan
            return Err(format!("ganti file gagal: {e}"));
        }

        // Buka versi baru sesudah proses ini keluar.
        let mut c = std::process::Command::new("cmd");
        c.raw_arg(format!(
            "/C ping -n 2 127.0.0.1 >nul & start \"\" \"{}\"",
            cur.display()
        ))
        .creation_flags(0x0800_0000);
        c.spawn().map_err(|e| format!("gagal membuka versi baru: {e}"))?;
        app.exit(0);
        Ok(())
    }
}

#[cfg(windows)]
async fn download(
    url: &str,
    dst: &std::path::Path,
    progress: impl Fn(u64, Option<u64>),
) -> Result<(), String> {
    use std::io::Write;
    let err = |e: reqwest::Error| format!("unduh update gagal: {e}");
    let client = reqwest::Client::builder()
        .user_agent(format!("RustMusic/{}", current_version()))
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(err)?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    let total = resp.content_length();
    let mut f = std::fs::File::create(dst).map_err(|e| format!("buat file update gagal: {e}"))?;
    let mut done: u64 = 0;
    let mut head = [0u8; 2];
    progress(0, total);
    while let Some(chunk) = resp.chunk().await.map_err(err)? {
        if done < 2 {
            for (i, b) in chunk.iter().take(2 - done as usize).enumerate() {
                head[done as usize + i] = *b;
            }
        }
        f.write_all(&chunk).map_err(|e| format!("simpan update gagal: {e}"))?;
        done += chunk.len() as u64;
        progress(done, total);
    }
    f.flush().map_err(|e| e.to_string())?;
    drop(f);
    // Periksa: ukuran cocok dan benar-benar file exe Windows ("MZ").
    if let Some(t) = total {
        if t != done {
            return Err("unduhan update terpotong, coba lagi".into());
        }
    }
    if done < 1_000_000 || &head != b"MZ" {
        return Err("file update tidak valid".into());
    }
    Ok(())
}

/// Dipanggil saat start: buang sisa update sebelumnya.
pub fn cleanup() {
    #[cfg(windows)]
    if let Ok(cur) = std::env::current_exe() {
        let _ = std::fs::remove_file(cur.with_extension("old"));
        let _ = std::fs::remove_file(cur.with_file_name("RustMusic.update.exe"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_ver("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_ver("0.2"), Some((0, 2, 0)));
        assert_eq!(parse_ver("V2"), Some((2, 0, 0)));
        assert_eq!(parse_ver("build-12"), None);
        assert!(parse_ver("v0.10.0") > parse_ver("v0.9.5"));
    }

    fn rel(tag: &str, exe: bool, pre: bool) -> Value {
        let assets = if exe {
            json!([{"name": "RustMusic.exe", "browser_download_url": format!("https://github.com/same-09/RustMusic-deskop/releases/download/{tag}/RustMusic.exe"), "size": 6000000}])
        } else {
            json!([])
        };
        json!({"tag_name": tag, "draft": false, "prerelease": pre, "body": "catatan", "assets": assets})
    }

    #[test]
    fn picks_highest_release_with_exe() {
        let rels = json!([
            rel("build-30", true, false),
            rel("v0.2.0", true, false),
            rel("v0.3.0", false, false),
            rel("v0.4.0", true, true),
            rel("v0.1.0", true, false),
        ]);
        let u = pick_latest(&rels).unwrap();
        assert_eq!(u.tag, "v0.2.0");
        assert_eq!(u.version, "0.2.0");
        assert!(u.url.ends_with("/v0.2.0/RustMusic.exe"));
    }

    #[test]
    fn none_when_no_versioned_release() {
        assert!(pick_latest(&json!([rel("build-1", true, false)])).is_none());
    }
}
