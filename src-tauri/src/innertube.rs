use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize, Debug, PartialEq)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub cover: String,
}

const URL: &str = "https://music.youtube.com/youtubei/v1/search?prettyPrint=false";
// Filter "Songs" di YouTube Music.
const SONGS_FILTER: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D";

pub async fn search_songs(query: &str) -> Result<Vec<Song>, String> {
    let body = json!({
        "context": { "client": {
            "clientName": "WEB_REMIX",
            "clientVersion": "1.20241127.01.00",
            "hl": "en"
        }},
        "query": query,
        "params": SONGS_FILTER
    });
    let res: Value = reqwest::Client::new()
        .post(URL)
        .header("Origin", "https://music.youtube.com")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(parse_songs(&res))
}

/// Cari semua "musicResponsiveListItemRenderer" di JSON, apa pun letaknya.
pub fn parse_songs(root: &Value) -> Vec<Song> {
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

fn walk(v: &Value, out: &mut Vec<Song>) {
    match v {
        Value::Object(map) => {
            if let Some(item) = map.get("musicResponsiveListItemRenderer") {
                if let Some(song) = song_from(item) {
                    out.push(song);
                }
            } else {
                for child in map.values() {
                    walk(child, out);
                }
            }
        }
        Value::Array(list) => {
            for child in list {
                walk(child, out);
            }
        }
        _ => {}
    }
}

fn runs(item: &Value, col: usize) -> Vec<String> {
    item.pointer(&format!(
        "/flexColumns/{col}/musicResponsiveListItemFlexColumnRenderer/text/runs"
    ))
    .and_then(Value::as_array)
    .map(|r| {
        r.iter()
            .filter_map(|x| x.get("text").and_then(Value::as_str).map(String::from))
            .collect()
    })
    .unwrap_or_default()
}

fn song_from(item: &Value) -> Option<Song> {
    let id = item
        .pointer("/playlistItemData/videoId")
        .and_then(Value::as_str)?
        .to_string();
    let title = runs(item, 0).into_iter().next()?;
    // Kolom 2: "Artist • Album • 3:21". Ambil bagian pertama saja.
    let artist = runs(item, 1).into_iter().next().unwrap_or_default();
    let cover = item
        .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .and_then(|t| t.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(Song { id, title, artist, cover })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_song() {
        let data = json!({"contents": [{"musicResponsiveListItemRenderer": {
            "playlistItemData": {"videoId": "abc123"},
            "flexColumns": [
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Judul"}]}}},
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Penyanyi"}, {"text": " • "}]}}}
            ],
            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
                {"url": "kecil"}, {"url": "besar"}
            ]}}}
        }}]});
        assert_eq!(
            parse_songs(&data),
            vec![Song { id: "abc123".into(), title: "Judul".into(), artist: "Penyanyi".into(), cover: "besar".into() }]
        );
    }
}
