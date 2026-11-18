//! Lirik dari LRCLIB (lrclib.net): gratis, ada lirik yang jalan sesuai waktu lagu (LRC).

use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Default)]
pub struct Lyrics {
    /// Format LRC: "[01:23.45] teks" per baris. Kosong bila tidak ada.
    synced: Option<String>,
    /// Lirik biasa tanpa waktu.
    plain: Option<String>,
}

/// "Judul (feat. X) - Remix" -> "Judul"
fn clean_title(t: &str) -> String {
    let mut s = t.to_string();
    for sep in [" (", " [", " - ", " – "] {
        if let Some(i) = s.find(sep) {
            s.truncate(i);
        }
    }
    let s = s.trim().to_string();
    if s.is_empty() { t.trim().to_string() } else { s }
}

/// "A, B & C" -> "A"
fn first_artist(a: &str) -> String {
    a.split(|c| c == ',' || c == '&' || c == '•')
        .next()
        .unwrap_or(a)
        .trim()
        .to_string()
}

fn has(v: &Value, key: &str) -> bool {
    v.get(key)
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
}

/// Pilih hasil terbaik: utamakan yang ada waktunya, durasi harus dekat (versi lagu sama).
fn pick(items: &[Value], dur: f64) -> Option<&Value> {
    let gap = |v: &Value| -> f64 {
        if dur > 0.0 {
            (v.get("duration").and_then(Value::as_f64).unwrap_or(0.0) - dur).abs()
        } else {
            0.0
        }
    };
    for key in ["syncedLyrics", "plainLyrics"] {
        let best = items
            .iter()
            .filter(|v| has(v, key) && (dur <= 0.0 || gap(v) <= 6.0))
            .min_by(|a, b| gap(a).partial_cmp(&gap(b)).unwrap_or(std::cmp::Ordering::Equal));
        if best.is_some() {
            return best;
        }
    }
    None
}

async fn search(client: &reqwest::Client, params: &[(&str, &str)]) -> Result<Vec<Value>, String> {
    let err = |e: reqwest::Error| format!("cari lirik gagal: {e}");
    let v: Value = client
        .get("https://lrclib.net/api/search")
        .query(params)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json()
        .await
        .map_err(err)?;
    Ok(v.as_array().cloned().unwrap_or_default())
}

/// Cari lirik lagu. `duration` dalam detik (0 bila belum tahu).
#[tauri::command]
pub async fn lyrics(title: String, artist: String, duration: f64) -> Result<Lyrics, String> {
    let client = reqwest::Client::builder()
        .user_agent("RustMusic/0.1 (https://github.com/same-09/RustMusic-deskop)")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let t = clean_title(&title);
    let a = first_artist(&artist);

    let mut items = search(&client, &[("track_name", t.as_str()), ("artist_name", a.as_str())]).await?;
    if pick(&items, duration).is_none() {
        let q = format!("{t} {a}");
        items = search(&client, &[("q", q.as_str())]).await?;
    }
    if pick(&items, duration).is_none() {
        items = search(&client, &[("q", t.as_str())]).await?;
    }
    let Some(best) = pick(&items, duration) else {
        return Ok(Lyrics::default());
    };
    let text = |k: &str| {
        best.get(k)
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty())
    };
    Ok(Lyrics { synced: text("syncedLyrics"), plain: text("plainLyrics") })
}
