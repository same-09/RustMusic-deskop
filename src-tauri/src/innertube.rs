use serde::Serialize;
use serde_json::{json, Value};

/// Satu lagu. `artist_id` dan `album_id` kosong bila YouTube tidak memberi tautan.
#[derive(Serialize, Debug, PartialEq, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub cover: String,
    pub artist_id: String,
    pub album: String,
    pub album_id: String,
}

/// Satu artis atau album di hasil cari atau di rak halaman.
#[derive(Serialize, Debug, PartialEq, Clone)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub cover: String,
}

/// Satu rak di halaman artis atau album. `kind` = "album" atau "artist".
#[derive(Serialize, Debug, PartialEq)]
pub struct Section {
    pub title: String,
    pub kind: String,
    pub items: Vec<Entry>,
}

/// Halaman artis atau album.
#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub title: String,
    pub subtitle: String,
    pub artist: String,
    pub artist_id: String,
    pub cover: String,
    pub songs: Vec<Song>,
    pub sections: Vec<Section>,
}

const BASE: &str = "https://music.youtube.com/youtubei/v1";
// Ekor filter pencarian YouTube Music. Delapan huruf di depannya menentukan jenis hasil.
const FILTER_TAIL: &str = "AWoKEAkQBRAKEAMQBA%3D%3D";

/// Jenis hasil cari.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Songs,
    Albums,
    Artists,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "songs" => Some(Kind::Songs),
            "albums" => Some(Kind::Albums),
            "artists" => Some(Kind::Artists),
            _ => None,
        }
    }
    fn params(self) -> String {
        let head = match self {
            Kind::Songs => "EgWKAQII",
            Kind::Albums => "EgWKAQIY",
            Kind::Artists => "EgWKAQIg",
        };
        format!("{head}{FILTER_TAIL}")
    }
}

async fn post(path: &str, mut body: Value) -> Result<Value, String> {
    body["context"] = json!({ "client": {
        "clientName": "WEB_REMIX",
        "clientVersion": "1.20241127.01.00",
        "hl": "en"
    }});
    reqwest::Client::new()
        .post(format!("{BASE}/{path}?prettyPrint=false"))
        .header("Origin", "https://music.youtube.com")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

pub async fn search_songs(query: &str) -> Result<Vec<Song>, String> {
    let res = post("search", json!({ "query": query, "params": Kind::Songs.params() })).await?;
    Ok(parse_songs(&res))
}

/// Cari artis atau album.
pub async fn search_entries(query: &str, kind: Kind) -> Result<Vec<Entry>, String> {
    let res = post("search", json!({ "query": query, "params": kind.params() })).await?;
    Ok(parse_entries(&res))
}

const PLAYER_URL: &str = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false";
const VR_UA: &str = "com.google.android.apps.youtube.vr.oculus/1.60.19 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip";

/// URL audio langsung lewat Innertube (klien ANDROID_VR, tanpa yt-dlp). Cepat: satu request.
pub async fn audio_url(video_id: &str) -> Result<String, String> {
    let body = json!({
        "context": { "client": {
            "clientName": "ANDROID_VR",
            "clientVersion": "1.60.19",
            "deviceMake": "Oculus",
            "deviceModel": "Quest 3",
            "androidSdkVersion": 32,
            "osName": "Android",
            "osVersion": "12L",
            "hl": "en"
        }},
        "videoId": video_id,
        "contentCheckOk": true,
        "racyCheckOk": true
    });
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let res: Value = client
        .post(PLAYER_URL)
        .header("User-Agent", VR_UA)
        .header("X-YouTube-Client-Name", "28")
        .header("X-YouTube-Client-Version", "1.60.19")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    parse_audio_url(&res)
}

/// Pilih audio dengan bitrate tertinggi yang punya URL langsung (tanpa signatureCipher).
pub fn parse_audio_url(root: &Value) -> Result<String, String> {
    let status = root
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
        .unwrap_or("");
    if status != "OK" {
        return Err(format!("innertube: lagu tidak bisa diputar ({status})"));
    }
    root.pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|f| {
            f.get("mimeType")
                .and_then(Value::as_str)
                .map_or(false, |m| m.starts_with("audio/"))
        })
        .filter_map(|f| {
            let url = f.get("url").and_then(Value::as_str)?;
            let br = f.get("bitrate").and_then(Value::as_u64).unwrap_or(0);
            Some((br, url))
        })
        .max_by_key(|(br, _)| *br)
        .map(|(_, u)| u.to_string())
        .ok_or_else(|| "innertube: tidak ada URL audio langsung".to_string())
}

/// Ambil halaman artis (id "UC...") atau album (id "MPREb_...").
pub async fn browse(browse_id: &str) -> Result<Page, String> {
    if browse_id.is_empty()
        || browse_id.len() > 64
        || !browse_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("id halaman tidak valid".into());
    }
    let res = post("browse", json!({ "browseId": browse_id })).await?;
    Ok(parse_page(&res, browse_id))
}

/// Kumpulkan semua nilai dengan nama `key`, di mana pun letaknya. Tidak masuk ke dalam nilai yang sudah ketemu.
fn collect<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            if let Some(hit) = map.get(key) {
                out.push(hit);
            } else {
                for child in map.values() {
                    collect(child, key, out);
                }
            }
        }
        Value::Array(list) => {
            for child in list {
                collect(child, key, out);
            }
        }
        _ => {}
    }
}

/// Nilai pertama (pencarian dalam) untuk salah satu nama di `keys`, urut sesuai `keys`.
fn find_first<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    for k in keys {
        let mut hits = Vec::new();
        collect(v, k, &mut hits);
        if let Some(h) = hits.into_iter().next() {
            return Some(h);
        }
    }
    None
}

pub fn parse_songs(root: &Value) -> Vec<Song> {
    let mut items = Vec::new();
    collect(root, "musicResponsiveListItemRenderer", &mut items);
    items.into_iter().filter_map(song_from).collect()
}

/// Run mentah (teks + tautan) dari satu kolom.
fn runs(item: &Value, col: usize) -> Vec<&Value> {
    item.pointer(&format!(
        "/flexColumns/{col}/musicResponsiveListItemFlexColumnRenderer/text/runs"
    ))
    .and_then(Value::as_array)
    .map(|r| r.iter().collect())
    .unwrap_or_default()
}

fn run_text(r: &Value) -> &str {
    r.get("text").and_then(Value::as_str).unwrap_or("")
}

fn run_browse(r: &Value) -> &str {
    r.pointer("/navigationEndpoint/browseEndpoint/browseId")
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn join_runs(runs: &[&Value]) -> String {
    runs.iter().map(|r| run_text(r)).collect::<String>().trim().to_string()
}

/// URL gambar terbesar dari larik "thumbnails" pertama di dalam `v`.
fn last_thumb(v: &Value) -> String {
    let mut hits = Vec::new();
    collect(v, "thumbnails", &mut hits);
    hits.into_iter()
        .next()
        .and_then(Value::as_array)
        .and_then(|t| t.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn song_from(item: &Value) -> Option<Song> {
    let id = item
        .pointer("/playlistItemData/videoId")
        .and_then(Value::as_str)?
        .to_string();
    let title = runs(item, 0).first().map(|r| run_text(r).to_string())?;
    // Kolom 2: "Artist • Album • 3:21". Artis dan album punya tautan bila ada.
    let col = runs(item, 1);
    let artist_run = col
        .iter()
        .find(|r| run_browse(r).starts_with("UC"))
        .or_else(|| col.first());
    let mut artist = artist_run.map(|r| run_text(r).to_string()).unwrap_or_default();
    // Di halaman artis kolom ini kadang berisi jumlah putar ("1.2M plays"), bukan nama.
    if artist.ends_with(" plays") {
        artist.clear();
    }
    let artist_id = artist_run
        .map(|r| run_browse(r))
        .filter(|b| b.starts_with("UC"))
        .unwrap_or("")
        .to_string();
    let album_run = col.iter().find(|r| run_browse(r).starts_with("MPRE"));
    let album = album_run.map(|r| run_text(r).to_string()).unwrap_or_default();
    let album_id = album_run.map(|r| run_browse(r).to_string()).unwrap_or_default();
    let cover = item
        .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .and_then(|t| t.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(Song { id, title, artist, cover, artist_id, album, album_id })
}

/// Baris hasil cari artis/album: ada `browseId`, tidak ada `videoId`.
pub fn parse_entries(root: &Value) -> Vec<Entry> {
    let mut items = Vec::new();
    collect(root, "musicResponsiveListItemRenderer", &mut items);
    items
        .into_iter()
        .filter_map(|item| {
            let id = item
                .pointer("/navigationEndpoint/browseEndpoint/browseId")
                .and_then(Value::as_str)?
                .to_string();
            if !(id.starts_with("UC") || id.starts_with("MPRE")) {
                return None;
            }
            let title = runs(item, 0).first().map(|r| run_text(r).to_string())?;
            let subtitle = join_runs(&runs(item, 1));
            let cover = last_thumb(item.get("thumbnail").unwrap_or(&Value::Null));
            Some(Entry { id, title, subtitle, cover })
        })
        .collect()
}

fn two_row(item: &Value) -> Option<(Entry, &'static str)> {
    let id = item
        .pointer("/navigationEndpoint/browseEndpoint/browseId")
        .and_then(Value::as_str)?
        .to_string();
    let kind = if id.starts_with("MPRE") {
        "album"
    } else if id.starts_with("UC") {
        "artist"
    } else {
        return None;
    };
    let title = item.pointer("/title/runs/0/text").and_then(Value::as_str)?.to_string();
    let subtitle = item
        .pointer("/subtitle/runs")
        .and_then(Value::as_array)
        .map(|r| r.iter().map(|x| run_text(x)).collect::<String>())
        .unwrap_or_default()
        .trim()
        .to_string();
    let cover = last_thumb(item.get("thumbnailRenderer").unwrap_or(&Value::Null));
    Some((Entry { id, title, subtitle, cover }, kind))
}

/// Susun halaman artis/album dari JSON `browse`. Tahan terhadap beda susunan antar versi YouTube.
pub fn parse_page(root: &Value, browse_id: &str) -> Page {
    let header = find_first(
        root,
        &[
            "musicImmersiveHeaderRenderer",
            "musicVisualHeaderRenderer",
            "musicResponsiveHeaderRenderer",
            "musicDetailHeaderRenderer",
        ],
    );
    let is_album = browse_id.starts_with("MPRE");
    let mut page = Page::default();
    if let Some(h) = header {
        page.title = h
            .pointer("/title/runs/0/text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        page.subtitle = h
            .pointer("/subtitle/runs")
            .and_then(Value::as_array)
            .map(|r| r.iter().map(|x| run_text(x)).collect::<String>())
            .unwrap_or_default()
            .trim()
            .to_string();
        page.artist = h
            .pointer("/straplineTextOne/runs/0/text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        page.cover = last_thumb(h);
        // Artis album: dari baris "strapline", atau dari tautan artis di subjudul (susunan lama).
        let strap = h.pointer("/straplineTextOne/runs/0");
        let sub_runs: Vec<&Value> = h
            .pointer("/subtitle/runs")
            .and_then(Value::as_array)
            .map(|r| r.iter().collect())
            .unwrap_or_default();
        let link = strap
            .filter(|r| run_browse(r).starts_with("UC"))
            .or_else(|| sub_runs.iter().copied().find(|r| run_browse(r).starts_with("UC")));
        if let Some(r) = link {
            page.artist_id = run_browse(r).to_string();
            if page.artist.is_empty() {
                page.artist = run_text(r).to_string();
            }
        }
    }
    if !is_album {
        page.artist = page.title.clone();
        page.artist_id = browse_id.to_string();
    }
    let mut seen = std::collections::HashSet::new();
    for mut s in parse_songs(root) {
        if !seen.insert(s.id.clone()) {
            continue;
        }
        if s.cover.is_empty() {
            s.cover = page.cover.clone();
        }
        if s.artist.is_empty() {
            s.artist = page.artist.clone();
        }
        if is_album && s.album.is_empty() {
            s.album = page.title.clone();
            s.album_id = browse_id.to_string();
        }
        if s.artist_id.is_empty() && !page.artist_id.is_empty() && s.artist == page.artist {
            s.artist_id = page.artist_id.clone();
        }
        page.songs.push(s);
    }
    let mut shelves = Vec::new();
    collect(root, "musicCarouselShelfRenderer", &mut shelves);
    for shelf in shelves {
        let title = shelf
            .pointer("/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut items: Vec<Entry> = Vec::new();
        let mut kind = "";
        for c in shelf.get("contents").and_then(Value::as_array).into_iter().flatten() {
            let Some(row) = c.get("musicTwoRowItemRenderer") else { continue };
            let Some((entry, k)) = two_row(row) else { continue };
            if kind.is_empty() {
                kind = k;
            }
            if k == kind {
                items.push(entry);
            }
        }
        if !items.is_empty() {
            page.sections.push(Section { title, kind: kind.to_string(), items });
        }
    }
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(runs: Value) -> Value {
        json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": runs}}})
    }

    fn link(text: &str, id: &str) -> Value {
        json!({"text": text, "navigationEndpoint": {"browseEndpoint": {"browseId": id}}})
    }

    #[test]
    fn picks_best_audio_url() {
        let data = json!({
            "playabilityStatus": {"status": "OK"},
            "streamingData": {"adaptiveFormats": [
                {"mimeType": "video/mp4", "bitrate": 9000000, "url": "video"},
                {"mimeType": "audio/mp4", "bitrate": 128000, "url": "kecil"},
                {"mimeType": "audio/webm", "bitrate": 160000, "url": "besar"},
                {"mimeType": "audio/webm", "bitrate": 250000, "signatureCipher": "x"}
            ]}
        });
        assert_eq!(parse_audio_url(&data), Ok("besar".to_string()));
    }

    #[test]
    fn rejects_unplayable() {
        let data = json!({"playabilityStatus": {"status": "LOGIN_REQUIRED"}});
        assert!(parse_audio_url(&data).is_err());
    }

    #[test]
    fn parses_one_song() {
        let data = json!({"contents": [{"musicResponsiveListItemRenderer": {
            "playlistItemData": {"videoId": "abc123"},
            "flexColumns": [
                col(json!([{"text": "Judul"}])),
                col(json!([{"text": "Penyanyi"}, {"text": " • "}]))
            ],
            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
                {"url": "kecil"}, {"url": "besar"}
            ]}}}
        }}]});
        assert_eq!(
            parse_songs(&data),
            vec![Song {
                id: "abc123".into(),
                title: "Judul".into(),
                artist: "Penyanyi".into(),
                cover: "besar".into(),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn song_keeps_artist_and_album_links() {
        let data = json!({"x": {"musicResponsiveListItemRenderer": {
            "playlistItemData": {"videoId": "v1"},
            "flexColumns": [
                col(json!([{"text": "Lagu"}])),
                col(json!([link("Band", "UCband"), {"text": " • "}, link("Album Satu", "MPREb_abc"), {"text": " • "}, {"text": "3:21"}]))
            ]
        }}});
        let s = &parse_songs(&data)[0];
        assert_eq!(s.artist, "Band");
        assert_eq!(s.artist_id, "UCband");
        assert_eq!(s.album, "Album Satu");
        assert_eq!(s.album_id, "MPREb_abc");
    }

    #[test]
    fn parses_artist_search_rows() {
        let data = json!({"a": [{"musicResponsiveListItemRenderer": {
            "navigationEndpoint": {"browseEndpoint": {"browseId": "UCxyz"}},
            "flexColumns": [col(json!([{"text": "Band"}])), col(json!([{"text": "Artist"}, {"text": " • "}, {"text": "2M subscribers"}]))],
            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [{"url": "a"}, {"url": "b"}]}}}
        }}, {"musicResponsiveListItemRenderer": {
            "navigationEndpoint": {"browseEndpoint": {"browseId": "VLPLxxx"}},
            "flexColumns": [col(json!([{"text": "Playlist"}]))]
        }}]});
        assert_eq!(
            parse_entries(&data),
            vec![Entry {
                id: "UCxyz".into(),
                title: "Band".into(),
                subtitle: "Artist • 2M subscribers".into(),
                cover: "b".into()
            }]
        );
    }

    fn two_row_item(id: &str, title: &str, sub: &str) -> Value {
        json!({"musicTwoRowItemRenderer": {
            "title": {"runs": [{"text": title}]},
            "subtitle": {"runs": [{"text": sub}]},
            "navigationEndpoint": {"browseEndpoint": {"browseId": id}},
            "thumbnailRenderer": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [{"url": "kecil"}, {"url": "besar"}]}}}
        }})
    }

    #[test]
    fn parses_artist_page() {
        let data = json!({"header": {"musicImmersiveHeaderRenderer": {
            "title": {"runs": [{"text": "Band"}]},
            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [{"url": "foto"}]}}}
        }}, "contents": {"sections": [
            {"musicShelfRenderer": {"contents": [{"musicResponsiveListItemRenderer": {
                "playlistItemData": {"videoId": "s1"},
                "flexColumns": [col(json!([{"text": "Hit"}])), col(json!([{"text": "1.2M plays"}]))]
            }}]}},
            {"musicCarouselShelfRenderer": {
                "header": {"musicCarouselShelfBasicHeaderRenderer": {"title": {"runs": [{"text": "Albums"}]}}},
                "contents": [two_row_item("MPREb_1", "Album Satu", "Album • 2020"), two_row_item("VLPL1", "Daftar", "Playlist")]
            }},
            {"musicCarouselShelfRenderer": {
                "header": {"musicCarouselShelfBasicHeaderRenderer": {"title": {"runs": [{"text": "Fans might also like"}]}}},
                "contents": [two_row_item("UCmirip", "Band Mirip", "1M subscribers")]
            }}
        ]}});
        let p = parse_page(&data, "UCband");
        assert_eq!(p.title, "Band");
        assert_eq!(p.cover, "foto");
        assert_eq!(p.songs.len(), 1);
        assert_eq!(p.songs[0].artist, "Band");
        assert_eq!(p.songs[0].artist_id, "UCband");
        assert_eq!(p.songs[0].cover, "foto");
        assert_eq!(p.sections.len(), 2);
        assert_eq!(p.sections[0].kind, "album");
        assert_eq!(p.sections[0].items.len(), 1);
        assert_eq!(p.sections[0].items[0].cover, "besar");
        assert_eq!(p.sections[1].kind, "artist");
    }

    #[test]
    fn parses_album_page() {
        let data = json!({"contents": {"twoColumn": {
            "tabs": [{"musicResponsiveHeaderRenderer": {
                "title": {"runs": [{"text": "Album Satu"}]},
                "straplineTextOne": {"runs": [link("Band", "UCband")]},
                "subtitle": {"runs": [{"text": "Album"}, {"text": " • "}, {"text": "2020"}]},
                "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [{"url": "sampul"}]}}}
            }}],
            "secondary": {"musicShelfRenderer": {"contents": [
                {"musicResponsiveListItemRenderer": {"playlistItemData": {"videoId": "t1"}, "flexColumns": [col(json!([{"text": "Lagu 1"}]))]}},
                {"musicResponsiveListItemRenderer": {"playlistItemData": {"videoId": "t2"}, "flexColumns": [col(json!([{"text": "Lagu 2"}]))]}},
                {"musicResponsiveListItemRenderer": {"playlistItemData": {"videoId": "t2"}, "flexColumns": [col(json!([{"text": "Lagu 2"}]))]}}
            ]}}
        }}});
        let p = parse_page(&data, "MPREb_abc");
        assert_eq!(p.title, "Album Satu");
        assert_eq!(p.artist, "Band");
        assert_eq!(p.subtitle, "Album • 2020");
        assert_eq!(p.songs.len(), 2);
        assert_eq!(p.songs[0].cover, "sampul");
        assert_eq!(p.songs[0].artist, "Band");
        assert_eq!(p.songs[0].album, "Album Satu");
        assert_eq!(p.songs[0].album_id, "MPREb_abc");
        assert_eq!(p.artist_id, "UCband");
        assert_eq!(p.songs[0].artist_id, "UCband");
    }

    #[test]
    fn album_page_old_layout_finds_artist_in_subtitle() {
        let data = json!({"h": {"musicDetailHeaderRenderer": {
            "title": {"runs": [{"text": "Album Dua"}]},
            "subtitle": {"runs": [{"text": "Album"}, {"text": " • "}, link("Grup", "UCgrup"), {"text": " • "}, {"text": "2019"}]}
        }}, "l": {"musicResponsiveListItemRenderer": {"playlistItemData": {"videoId": "z1"}, "flexColumns": [col(json!([{"text": "Lagu"}]))]}}});
        let p = parse_page(&data, "MPREb_xyz");
        assert_eq!(p.artist, "Grup");
        assert_eq!(p.songs[0].artist_id, "UCgrup");
    }
}
