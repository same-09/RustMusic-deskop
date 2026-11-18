//! Kenali lagu dari rekaman mikrofon lewat AudD (api.audd.io).
//! UI merekam ~12 detik, mengubahnya ke WAV, lalu memanggil `recognize`.
//! Audio itu dikirim ke AudD. Butuh token API dari dashboard.audd.io (diisi di Pengaturan).

use base64::Engine;
use reqwest::multipart::{Form, Part};
use serde::Serialize;
use serde_json::Value;

const API: &str = "https://api.audd.io/";
/// Batas ukuran rekaman (WAV mono 22 kHz 16 bit ~ 44 KB/detik, jadi ini cukup untuk ~45 detik).
const MAX_BYTES: usize = 2_000_000;

#[derive(Serialize, Debug, PartialEq)]
pub struct Found {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: String,
}

/// Baca balasan AudD. `Ok(None)` = tidak ada lagu yang cocok.
pub fn parse_reply(v: &Value) -> Result<Option<Found>, String> {
    if v.get("status").and_then(Value::as_str) == Some("error") {
        let msg = v
            .pointer("/error/error_message")
            .and_then(Value::as_str)
            .unwrap_or("galat tidak dikenal");
        return Err(format!("AudD: {msg}"));
    }
    let Some(r) = v.get("result").filter(|r| r.is_object()) else { return Ok(None) };
    let get = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let title = get("title");
    if title.is_empty() {
        return Ok(None);
    }
    let year = get("release_date").chars().take(4).collect();
    Ok(Some(Found { title, artist: get("artist"), album: get("album"), year }))
}

#[tauri::command]
pub async fn recognize(audio_b64: String, token: String) -> Result<Option<Found>, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(audio_b64.trim())
        .map_err(|e| format!("rekaman rusak: {e}"))?;
    if bytes.len() < 1000 {
        return Err("rekaman terlalu pendek".into());
    }
    if bytes.len() > MAX_BYTES {
        return Err("rekaman terlalu besar".into());
    }
    let part = Part::bytes(bytes)
        .file_name("rekaman.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let form = Form::new().text("api_token", token.trim().to_string()).part("file", part);
    let reply: Value = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(40))
        .build()
        .map_err(|e| e.to_string())?
        .post(API)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("tidak bisa menghubungi AudD: {e}"))?
        .json()
        .await
        .map_err(|e| format!("balasan AudD tidak terbaca: {e}"))?;
    parse_reply(&reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_match() {
        let v = json!({"status": "success", "result": {"artist": "Band", "title": "Lagu", "album": "Album", "release_date": "2020-05-01"}});
        assert_eq!(
            parse_reply(&v).unwrap(),
            Some(Found { title: "Lagu".into(), artist: "Band".into(), album: "Album".into(), year: "2020".into() })
        );
    }

    #[test]
    fn no_match_is_none() {
        assert_eq!(parse_reply(&json!({"status": "success", "result": null})).unwrap(), None);
    }

    #[test]
    fn api_error_is_err() {
        let v = json!({"status": "error", "error": {"error_code": 901, "error_message": "token salah"}});
        assert_eq!(parse_reply(&v).unwrap_err(), "AudD: token salah");
    }
}
