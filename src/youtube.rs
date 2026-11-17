//! YouTube, the way NabyTune's web player asks it (web/app/src/playback/youtube/resolver.ts):
//! the VISIONOS InnerTube client, which needs no PO token and answers with plain stream URLs.
//!
//! - A visitor ID first. Without one VISIONOS answers "Sign in to confirm you're not a bot".
//! - No `adPlaybackContext`: with it, googlevideo cuts streams off after ~1.3 MB.
//! - googlevideo refuses a request without a range, so the file comes down in 1 MB ranges.

use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, bail, Context as _, Result};
use serde_json::{json, Value};

const VISIONOS_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const WEB_REMIX_VERSION: &str = "1.20251020.01.00";

/// A client YouTube's player is asked as: its name, number and version, the user agent its
/// streams are then fetched with, and what else its context carries.
pub struct PlayerClient {
    name: &'static str,
    id: &'static str,
    version: &'static str,
    user_agent: &'static str,
    context: fn() -> Value,
}

/// The clients a song's stream is asked from, in turn; each must hand out plain stream URLs
/// (no signature to work out) that play to the end. (ANDROID_VR answers, but its streams are
/// refused past the first megabyte without a proof-of-origin token.)
const PLAYER_CLIENTS: [PlayerClient; 1] = [
    PlayerClient {
        name: "VISIONOS",
        id: "101",
        version: "1.02",
        user_agent: VISIONOS_UA,
        context: || json!({ "deviceMake": "Apple", "deviceModel": "RealityDevice17,1", "osName": "visionOS", "osVersion": "26.5.23O471" }),
    },
];
const CHUNK: u64 = 1024 * 1024;

/// One song from a YouTube Music search.
#[derive(Clone, Debug)]
pub struct SearchItem {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: String,
    /// The artwork URL; its `=wN-hN` suffix picks the size.
    pub thumbnail: Option<String>,
}

/// An artwork URL at `size` px square (googleusercontent's size suffix).
pub fn sized(url: &str, size: u32) -> String {
    match url.rfind("=w") {
        Some(at) => format!("{}=w{size}-h{size}-l90-rj", &url[..at]),
        None => url.to_string(),
    }
}

fn runs(v: &Value) -> String {
    v["runs"].as_array().map(|r| r.iter().filter_map(|x| x["text"].as_str()).collect()).unwrap_or_default()
}

/// Every `musicResponsiveListItemRenderer` in a response, in order.
fn list_items<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            if let Some(item) = map.get("musicResponsiveListItemRenderer") {
                out.push(item);
                return;
            }
            map.values().for_each(|x| list_items(x, out));
        }
        Value::Array(items) => items.iter().for_each(|x| list_items(x, out)),
        _ => {}
    }
}

/// A playlist, album or mix on a Home shelf.
#[derive(Clone, Debug)]
pub struct Card {
    pub title: String,
    pub subtitle: String,
    pub thumbnail: Option<String>,
    /// What opening it means: a page to list (`browse_id`), or one song (`video_id`).
    pub browse_id: Option<String>,
    pub video_id: Option<String>,
}

#[derive(Clone, Debug)]
pub enum ShelfItem {
    Song(SearchItem),
    Card(Card),
}

#[derive(Clone, Debug)]
pub struct Shelf {
    pub title: String,
    pub items: Vec<ShelfItem>,
}

fn last_thumbnail(v: &Value) -> Option<String> {
    v["thumbnails"].as_array().and_then(|t| t.last()).and_then(|t| t["url"].as_str()).map(str::to_string)
}

pub const KINDS: [&str; 8] = ["Single", "Album", "EP", "Playlist", "Song", "Lagu", "Video", "Track"];

/// Cleans an artist string from category prefixes (e.g. "Song • Aethoro", "Lagu • Aethoro"),
/// view counts, or release years.
pub fn clean_artist(raw: &str) -> String {
    let parts: Vec<&str> = raw.split(" • ").collect();
    let is_kind = |p: &&str| {
        let trimmed = p.trim();
        KINDS.iter().any(|k| trimmed.eq_ignore_ascii_case(k))
            || ["views", "likes", "plays", "ditonton", "penayangan", "suka"].iter().any(|w| trimmed.to_lowercase().contains(w))
            || (trimmed.len() == 4 && trimmed.chars().all(|c| c.is_ascii_digit()))
    };
    let filtered: Vec<&str> = parts.into_iter().filter(|p| !is_kind(p)).collect();
    if let Some(first) = filtered.first() {
        first.trim().to_string()
    } else {
        raw.trim().to_string()
    }
}

/// A song row (`musicResponsiveListItemRenderer`): search results, quick picks, playlist
/// and album tracks. `card` fills in what an album's own rows leave out.
fn song(item: &Value, card: Option<&Card>) -> Option<SearchItem> {
    let video_id = item["playlistItemData"]["videoId"]
        .as_str()
        .or_else(|| {
            item["overlay"]["musicItemThumbnailOverlayRenderer"]["content"]["musicPlayButtonRenderer"]["playNavigationEndpoint"]
                ["watchEndpoint"]["videoId"]
                .as_str()
        })?
        .to_string();
    let column = |i: usize| runs(&item["flexColumns"][i]["musicResponsiveListItemFlexColumnRenderer"]["text"]);
    let title = column(0);
    if title.is_empty() {
        return None;
    }
    // "Artist • Album • 3:33"; an album's rows put the length in a fixed column instead.
    let is_clock = |p: &String| p.contains(':') && p.chars().all(|c| c.is_ascii_digit() || c == ':');
    let is_kind = |p: &str| KINDS.iter().any(|k| p.eq_ignore_ascii_case(k));
    let parts: Vec<String> = column(1).split(" • ").filter(|p| !p.is_empty()).map(str::to_string).collect();
    let duration = parts
        .last()
        .filter(|p| is_clock(p))
        .cloned()
        .or_else(|| Some(runs(&item["fixedColumns"][0]["musicResponsiveListItemFixedColumnRenderer"]["text"])).filter(is_clock));
    let named: Vec<&String> = parts.iter().filter(|p| !is_clock(p) && !is_kind(p)).collect();
    let raw_artist = named.first().map(|s| s.to_string()).or_else(|| card.map(|c| card_artist(&c.subtitle))).unwrap_or_default();
    let artist = clean_artist(&raw_artist);
    Some(SearchItem {
        video_id,
        title,
        artist,
        album: named.get(1).map(|s| s.to_string()).unwrap_or_default(),
        duration: duration.unwrap_or_default(),
        thumbnail: last_thumbnail(&item["thumbnail"]["musicThumbnailRenderer"]["thumbnail"])
            .or_else(|| card.and_then(|c| c.thumbnail.clone())),
    })
}

/// The artist out of a card's subtitle ("Single • Nabila Maharani & Tri Suaka • 2026"):
/// without the kind of release and the year.
fn card_artist(subtitle: &str) -> String {
    clean_artist(subtitle)
}

/// One item of a shelf: a song row, or a card (`musicTwoRowItemRenderer`).
fn shelf_item(item: &Value) -> Option<ShelfItem> {
    if let Some(row) = item.get("musicResponsiveListItemRenderer") {
        return song(row, None).map(ShelfItem::Song);
    }
    let card = item.get("musicTwoRowItemRenderer")?;
    let nav = &card["navigationEndpoint"];
    let browse_id = nav["browseEndpoint"]["browseId"]
        .as_str()
        .map(str::to_string)
        // A mix opens as a watch playlist; its page is the same id behind "VL".
        .or_else(|| nav["watchPlaylistEndpoint"]["playlistId"].as_str().map(|id| format!("VL{id}")));
    let video_id = nav["watchEndpoint"]["videoId"].as_str().map(str::to_string);
    if browse_id.is_none() && video_id.is_none() {
        return None;
    }
    // Artists' pages aren't lists of songs to play here, and podcasts (shows, episodes,
    // "Episodes for later") don't play in NabyTune.
    if browse_id.as_deref().is_some_and(|id| ["UC", "MPSP", "MPED", "VLSE"].iter().any(|p| id.starts_with(p))) {
        return None;
    }
    Some(ShelfItem::Card(Card {
        title: runs(&card["title"]),
        subtitle: runs(&card["subtitle"]),
        thumbnail: last_thumbnail(&card["thumbnailRenderer"]["musicThumbnailRenderer"]["thumbnail"]),
        browse_id,
        video_id,
    }))
}

/// A song's thumbs on YouTube Music.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Rating {
    Like,
    Dislike,
    Indifferent,
}

/// An artist's page: who, and what of theirs there is.
#[derive(Clone, Debug, Default)]
pub struct ArtistPage {
    pub name: String,
    /// "1.2M subscribers", or what YouTube Music says instead.
    pub subtitle: String,
    pub thumbnail: Option<String>,
    /// Their most played songs.
    pub songs: Vec<SearchItem>,
    /// Albums, singles, "Fans might also like"...: (title, cards).
    pub shelves: Vec<(String, Vec<Card>)>,
    /// The playlist id of the artist's radio (the header's "Mix" button, "RDEM…"), for
    /// `YouTube::radio_start`.
    pub radio: Option<String>,
    /// The playlist id of the header's "Shuffle" button ("RDAO…"): their own songs, shuffled,
    /// endless like a radio.
    pub shuffle: Option<String>,
    /// Channel ID (`UC…`) for subscribing and building a YouTube Music share link.
    pub channel_id: Option<String>,
    /// Whether the signed-in YouTube Music account currently subscribes to this artist.
    pub subscribed: bool,
}

/// One of the chips over a radio's queue ("All", "Popular", "Discover", "Deep cuts", a mood,
/// a decade, a genre...): another flavour of the same radio.
#[derive(Clone, Debug, PartialEq)]
pub struct RadioChip {
    pub label: String,
    /// What to hand `YouTube::radio_start` to switch to it.
    pub playlist_id: String,
    pub params: Option<String>,
    /// Whether this is the flavour the radio plays now ("All" on a fresh radio).
    pub selected: bool,
}

/// A batch of a radio: its songs, the chips to steer it, and how to ask for more.
#[derive(Clone, Debug, Default)]
pub struct RadioPage {
    pub tracks: Vec<SearchItem>,
    /// Empty when the response offers none: on a continuation, and on a radio started from a
    /// chip (YouTube Music keeps the chip row it already shows then).
    pub chips: Vec<RadioChip>,
    /// The token for `YouTube::radio_more`; None when the radio has nothing more to give.
    pub continuation: Option<String>,
    /// The radio's playlist id, as YouTube Music answered it.
    pub playlist_id: String,
}

/// One queue row of a `next` response (`playlistPanelVideoRenderer`, bare or inside the
/// wrapper a song with a music video counterpart comes in).
fn panel_track(item: &Value) -> Option<SearchItem> {
    let p = if item["playlistPanelVideoRenderer"].is_object() {
        &item["playlistPanelVideoRenderer"]
    } else {
        &item["playlistPanelVideoWrapperRenderer"]["primaryRenderer"]["playlistPanelVideoRenderer"]
    };
    let id = p["videoId"].as_str()?;
    // "Artist • Album • Year" for a song; "Artist • 885M views • 6.4M likes" for a video.
    let byline = runs(&p["longBylineText"]);
    let parts: Vec<&str> = byline.split(" • ").collect();
    let counted = |s: &&&str| [" views", " likes", " plays"].iter().any(|w| s.ends_with(w));
    let is_kind = |p: &&str| KINDS.iter().any(|k| p.eq_ignore_ascii_case(k));
    let named: Vec<&&str> = parts.iter().filter(|p| !counted(p) && !is_kind(p)).collect();
    Some(SearchItem {
        video_id: id.to_string(),
        title: runs(&p["title"]),
        artist: clean_artist(named.first().map(|s| s.to_string()).unwrap_or_default().as_str()),
        album: named
            .get(1)
            .filter(|a| !a.chars().all(|c| c.is_ascii_digit()))
            .map(|s| s.to_string())
            .unwrap_or_default(),
        duration: runs(&p["lengthText"]),
        thumbnail: last_thumbnail(&p["thumbnail"]),
    })
}

/// A chip of the queue's chip cloud, when it switches the radio (the header's "Save" chip
/// doesn't).
fn radio_chip(chip: &Value) -> Option<RadioChip> {
    let nav = &chip["navigationEndpoint"];
    let watch = [
        &nav["queueUpdateCommand"]["fetchContentsCommand"]["watchEndpoint"],
        &nav["watchEndpoint"],
        &nav["watchPlaylistEndpoint"],
    ]
    .into_iter()
    .find(|w| w["playlistId"].is_string())?;
    let label = runs(&chip["text"]);
    if label.is_empty() {
        return None;
    }
    Some(RadioChip {
        label,
        playlist_id: watch["playlistId"].as_str()?.to_string(),
        params: watch["params"].as_str().map(str::to_string),
        selected: chip["isSelected"].as_bool().unwrap_or(false),
    })
}

/// A `next` response as a radio batch: the first answer (`musicQueueRenderer`) or a
/// continuation (`continuationContents.playlistPanelContinuation`).
fn radio_page(next: &Value, playlist_id: &str) -> RadioPage {
    let queue = &next["contents"]["singleColumnMusicWatchNextResultsRenderer"]["tabbedRenderer"]["watchNextTabbedResultsRenderer"]["tabs"][0]
        ["tabRenderer"]["content"]["musicQueueRenderer"];
    let panel = if next["continuationContents"]["playlistPanelContinuation"].is_object() {
        &next["continuationContents"]["playlistPanelContinuation"]
    } else {
        &queue["content"]["playlistPanelRenderer"]
    };
    let mut tracks: Vec<SearchItem> = Vec::new();
    for item in panel["contents"].as_array().map(Vec::as_slice).unwrap_or_default() {
        if let Some(track) = panel_track(item) {
            if !tracks.iter().any(|t| t.video_id == track.video_id) {
                tracks.push(track);
            }
        }
    }
    // The chip cloud sits under the queue's header (`subHeaderChipCloud`); look through the
    // whole queue in case it moves.
    let mut found = Vec::new();
    find_all(queue, "chipCloudChipRenderer", &mut found);
    let chips = found.into_iter().filter_map(radio_chip).collect();
    let continuation = panel["continuations"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_object)
        .flat_map(|c| c.values())
        .find_map(|c| c["continuation"].as_str())
        .map(str::to_string);
    RadioPage {
        tracks,
        chips,
        continuation,
        playlist_id: panel["playlistId"].as_str().unwrap_or(playlist_id).to_string(),
    }
}

/// A watch button's playlist id, from its `watchEndpoint` or `watchPlaylistEndpoint`.
fn button_playlist(button: &Value) -> Option<String> {
    let nav = &button["buttonRenderer"]["navigationEndpoint"];
    nav["watchEndpoint"]["playlistId"]
        .as_str()
        .or_else(|| nav["watchPlaylistEndpoint"]["playlistId"].as_str())
        .map(str::to_string)
}

/// An album's or playlist's header.
#[derive(Clone, Debug, Default)]
pub struct Header {
    pub title: String,
    /// "Album • 2024", or the playlist's owner and size.
    pub subtitle: String,
    /// The artist, for an album.
    pub byline: String,
    pub thumbnail: Option<String>,
}

/// A card of any kind, artists included (Home's shelves leave artists out; an artist's page
/// keeps them, for "Fans might also like").
fn any_card(card: &Value) -> Option<Card> {
    let nav = &card["navigationEndpoint"];
    let browse_id = nav["browseEndpoint"]["browseId"]
        .as_str()
        .map(str::to_string)
        .or_else(|| nav["watchPlaylistEndpoint"]["playlistId"].as_str().map(|id| format!("VL{id}")));
    let video_id = nav["watchEndpoint"]["videoId"].as_str().map(str::to_string);
    if browse_id.is_none() && video_id.is_none() {
        return None;
    }
    if browse_id.as_deref().is_some_and(|id| ["MPSP", "MPED", "VLSE"].iter().any(|p| id.starts_with(p))) {
        return None;
    }
    Some(Card {
        title: runs(&card["title"]),
        subtitle: runs(&card["subtitle"]),
        thumbnail: last_thumbnail(&card["thumbnailRenderer"]["musicThumbnailRenderer"]["thumbnail"]),
        browse_id,
        video_id,
    })
}

/// Whether a card opens an artist's page.
pub fn is_artist(card: &Card) -> bool {
    card.browse_id.as_deref().is_some_and(|id| id.starts_with("UC"))
}

/// One audio stream of one track.
pub struct Stream {
    pub url: String,
    pub content_length: u64,
    pub length: Duration,
    /// How much louder than YouTube's reference the track is; NabyTune turns it down by as much.
    pub loudness_db: f32,
    /// What the stream is fetched as: the client that was given it.
    pub user_agent: &'static str,
}

pub struct YouTube {
    http: reqwest::blocking::Client,
    visitor: Mutex<Option<String>>,
    /// The signed-in session's cookies (account.rs), when there is one.
    account: Mutex<Option<String>>,
}

/// The signed-in account, as YouTube Music's menu names it.
#[derive(Clone, Debug)]
pub struct Account {
    pub name: String,
    pub handle: String,
    pub photo: Option<String>,
}

/// Where a page's list of songs is: a playlist's shelf when it has one (an editable playlist's
/// page also suggests songs, below it, that aren't in it); else the whole page (an album).
fn song_list(page: &Value) -> &Value {
    let mut shelves = Vec::new();
    find_all(page, "musicPlaylistShelfRenderer", &mut shelves);
    shelves.first().copied().unwrap_or(page)
}

/// The token for a list's next page: a `continuationItemRenderer` closing the list (as
/// YouTube Music sends it now), or `nextContinuationData` (as it used to).
fn next_page_token(list: &Value) -> Option<String> {
    let mut found = Vec::new();
    find_all(list, "continuationItemRenderer", &mut found);
    if let Some(token) = found.iter().find_map(|c| c["continuationEndpoint"]["continuationCommand"]["token"].as_str()) {
        return Some(token.to_string());
    }
    found.clear();
    find_all(list, "nextContinuationData", &mut found);
    found.iter().find_map(|c| c["continuation"].as_str()).map(str::to_string)
}

/// A saved mix's playlist id, from its browse id ("VLRD…" → "RD…"): mixes are radios, with
/// no page of their own to list.
fn mix_id(browse_id: &str) -> Option<&str> {
    let id = browse_id.strip_prefix("VL").unwrap_or(browse_id);
    id.starts_with("RD").then_some(id)
}

/// At most this many pages of Home are fetched (a few shelves each).
const HOME_PAGES: usize = 6;

/// At most this many pages of a list are fetched (about a hundred songs each).
const MAX_LIST_PAGES: usize = 60;

/// Every object under `key` anywhere in `v`, in order.
fn find_all<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if k == key {
                    out.push(child);
                } else {
                    find_all(child, key, out);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|child| find_all(child, key, out)),
        _ => {}
    }
}

impl YouTube {
    pub fn new() -> Self {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .expect("HTTP client");
        Self { http, visitor: Mutex::new(None), account: Mutex::new(None) }
    }

    /// A visitor ID, asked for once: any WEB_REMIX response carries one.
    fn visitor_data(&self) -> Result<String> {
        if let Some(v) = self.visitor.lock().unwrap().clone() {
            return Ok(v);
        }
        let response: Value = self
            .http
            .post("https://music.youtube.com/youtubei/v1/music/get_search_suggestions?prettyPrint=false")
            .header("origin", "https://music.youtube.com")
            .json(&json!({
                "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": WEB_REMIX_VERSION, "hl": "en" } },
                "input": "",
            }))
            .send()?
            .error_for_status()?
            .json()?;
        let visitor = response["responseContext"]["visitorData"]
            .as_str()
            .ok_or_else(|| anyhow!("no visitorData in YouTube's answer"))?
            .to_string();
        *self.visitor.lock().unwrap() = Some(visitor.clone());
        Ok(visitor)
    }

    /// VISIONOS's player response for `video_id`, and its AAC stream (itag 140, else 139).
    /// A song's audio stream. YouTube's player answers differently by client: a video one
    /// client is told is "not available" another can often play, so each is tried in turn.
    pub fn resolve(&self, video_id: &str) -> Result<Stream> {
        let visitor = self.visitor_data()?;
        let mut failed = Vec::new();
        // NABYTUNE_PROTO_PLAYER=<client name>: only that client, to try one on its own.
        let only = std::env::var("NABYTUNE_PROTO_PLAYER").ok();
        for client in PLAYER_CLIENTS.iter().filter(|c| only.as_deref().is_none_or(|n| n == c.name)) {
            match self.resolve_as(client, video_id, &visitor) {
                Ok(stream) => return Ok(stream),
                Err(e) => {
                    crate::audio::log(format!("resolve {video_id} as {}: {e:#}", client.name));
                    failed.push(e);
                }
            }
        }
        // The first client's answer says it best ("This video is not available", ...).
        Err(failed.into_iter().next().unwrap_or_else(|| anyhow!("no player client")))
    }

    fn resolve_as(&self, client: &PlayerClient, video_id: &str, visitor: &str) -> Result<Stream> {
        let mut context = json!({ "clientName": client.name, "clientVersion": client.version, "hl": "en", "visitorData": visitor });
        if let (Some(target), Some(extra)) = (context.as_object_mut(), (client.context)().as_object().cloned()) {
            target.extend(extra);
        }
        let player: Value = self
            .http
            .post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false")
            .header("user-agent", client.user_agent)
            .header("x-youtube-client-name", client.id)
            .header("x-youtube-client-version", client.version)
            .header("x-goog-visitor-id", visitor)
            .json(&json!({ "context": { "client": context }, "videoId": video_id, "contentCheckOk": true, "racyCheckOk": true }))
            .send()?
            .error_for_status()?
            .json()?;

        let status = player["playabilityStatus"]["status"].as_str().unwrap_or("?");
        if status != "OK" {
            let reason = player["playabilityStatus"]["reason"].as_str().unwrap_or("");
            bail!("YouTube: {status} {reason}");
        }
        let formats = player["streamingData"]["adaptiveFormats"].as_array().context("no adaptiveFormats")?;
        let format = [140, 139]
            .iter()
            .find_map(|itag| formats.iter().find(|f| f["itag"] == *itag && f["url"].is_string()))
            .context("no AAC audio stream with a plain URL")?;
        let content_length = format["contentLength"].as_str().and_then(|s| s.parse().ok()).context("no contentLength")?;
        let length = player["videoDetails"]["lengthSeconds"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0);
        Ok(Stream {
            url: format["url"].as_str().unwrap().to_string(),
            content_length,
            length: Duration::from_secs(length),
            loudness_db: player["playerConfig"]["audioConfig"]["loudnessDb"].as_f64().unwrap_or(0.0) as f32,
            user_agent: client.user_agent,
        })
    }

    /// Songs matching `query`, as YouTube Music's search lists them (the "Songs" filter).
    pub fn search(&self, query: &str) -> Result<Vec<SearchItem>> {
        let response: Value = self
            .http
            .post("https://music.youtube.com/youtubei/v1/search?prettyPrint=false")
            .header("origin", "https://music.youtube.com")
            .json(&json!({
                "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": WEB_REMIX_VERSION, "hl": "en" } },
                "query": query,
                "params": "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D",
            }))
            .send()?
            .error_for_status()?
            .json()?;
        if let Some(v) = response["responseContext"]["visitorData"].as_str() {
            self.visitor.lock().unwrap().get_or_insert_with(|| v.to_string());
        }
        let mut items = Vec::new();
        list_items(&response, &mut items);
        Ok(items.into_iter().filter_map(|item| song(item, None)).collect())
    }

    /// The HTTP client, for other services (lossless addons) to share its connections.
    pub fn client(&self) -> reqwest::blocking::Client {
        self.http.clone()
    }

    /// Signs requests with this session from now on (None: as a visitor again).
    pub fn set_account(&self, cookie: Option<String>) {
        *self.account.lock().unwrap() = cookie;
    }

    pub fn signed_in(&self) -> bool {
        self.account.lock().unwrap().is_some()
    }

    /// A WEB_REMIX call to `endpoint`, signed as the account when one is signed in.
    fn music(&self, endpoint: &str, body: Value) -> Result<Value> {
        let mut request = json!({
            "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": WEB_REMIX_VERSION, "hl": "en", "gl": "ID" } },
        });
        if let (Some(target), Some(extra)) = (request.as_object_mut(), body.as_object()) {
            target.extend(extra.clone());
        }
        let mut post = self
            .http
            .post(format!("https://music.youtube.com/youtubei/v1/{endpoint}?prettyPrint=false"))
            .header("origin", "https://music.youtube.com")
            .header("x-origin", "https://music.youtube.com")
            .header("referer", "https://music.youtube.com/");
        if let Some(cookie) = self.account.lock().unwrap().clone() {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            post = post.header("cookie", cookie.as_str()).header("x-goog-authuser", "0");
            if let Some(auth) = crate::account::authorization(&cookie, now) {
                post = post.header("authorization", auth);
            }
        }
        let response = post.json(&request).send()?;
        if matches!(response.status().as_u16(), 401 | 403) && self.signed_in() {
            bail!("YouTube Music turned the sign-in away: sign in again in Settings");
        }
        Ok(response.error_for_status()?.json()?)
    }

    fn browse(&self, body: Value) -> Result<Value> {
        self.music("browse", body)
    }

    /// A browse answer as it comes, for the examples that look into YouTube Music's pages.
    #[allow(dead_code)]
    pub fn browse_raw(&self, body: Value) -> Result<Value> {
        self.browse(body)
    }

    /// Who is signed in: their name, handle and photo.
    pub fn account_info(&self) -> Result<Account> {
        let menu = self.music("account/account_menu", json!({}))?;
        let header = &menu["actions"][0]["openPopupAction"]["popup"]["multiPageMenuRenderer"]["header"]["activeAccountHeaderRenderer"];
        let name = runs(&header["accountName"]);
        anyhow::ensure!(!name.is_empty(), "YouTube Music didn't name the account");
        Ok(Account { name, handle: runs(&header["channelHandle"]), photo: last_thumbnail(&header["accountPhoto"]) })
    }

    /// The account's playlists (its own and saved ones), as cards; "Liked music" first.
    pub fn account_playlists(&self) -> Result<Vec<Card>> {
        let page = self.browse(json!({ "browseId": "FEmusic_liked_playlists" }))?;
        let mut found = Vec::new();
        find_all(&page, "musicTwoRowItemRenderer", &mut found);
        let mut cards: Vec<Card> = found
            .into_iter()
            .filter_map(|card| match shelf_item(&json!({ "musicTwoRowItemRenderer": card })) {
                Some(ShelfItem::Card(c)) if c.browse_id.is_some() => Some(c),
                _ => None,
            })
            .collect();
        // "Liked music" is VLLM; keep it at the front, once.
        cards.retain(|c| c.browse_id.as_deref() != Some("VLLM"));
        cards.insert(0, Card { title: "Liked music".into(), subtitle: "Auto playlist".into(), thumbnail: None, browse_id: Some("VLLM".into()), video_id: None });
        Ok(cards)
    }

    /// Adds a song to one of the account's playlists (`playlist_id` with or without "VL").
    pub fn add_to_playlist(&self, playlist_id: &str, video_id: &str) -> Result<()> {
        let id = playlist_id.trim_start_matches("VL");
        let done = self.music(
            "browse/edit_playlist",
            json!({ "playlistId": id, "actions": [{ "action": "ACTION_ADD_VIDEO", "addedVideoId": video_id, "dedupeOption": "DEDUPE_OPTION_SKIP" }] }),
        )?;
        anyhow::ensure!(done["status"].as_str() == Some("STATUS_SUCCEEDED"), "YouTube Music didn't add the song");
        Ok(())
    }

    /// YouTube Music's Home (the account's, when signed in), every shelf of it, then the
    /// newest albums as one more shelf. Home comes a few shelves at a time: the first page,
    /// then more after a continuation token each, as YouTube Music's own page loads them on
    /// scrolling down.
    pub fn home(&self) -> Result<Vec<Shelf>> {
        let first = self.browse(json!({ "browseId": "FEmusic_home" }))?;
        let mut token = next_page_token(&first);
        let mut pages = vec![first];
        let mut seen = std::collections::HashSet::new();
        while let Some(t) = token.take() {
            if pages.len() >= HOME_PAGES || !seen.insert(t.clone()) {
                break;
            }
            let Ok(page) = self.browse(json!({ "continuation": t })) else { break };
            token = next_page_token(&page);
            pages.push(page);
        }
        let mut shelves: Vec<Shelf> = Vec::new();
        for page in &pages {
            let mut carousels = Vec::new();
            find_all(page, "musicCarouselShelfRenderer", &mut carousels);
            for shelf in carousels {
                let title = runs(&shelf["header"]["musicCarouselShelfBasicHeaderRenderer"]["title"]);
                // Podcast shelves ("Shows for you", "Podcasts", episodes): not playable here.
                let lower = title.to_lowercase();
                if title.is_empty() || ["podcast", "shows", "episode"].iter().any(|w| lower.contains(w)) {
                    continue;
                }
                let Some(contents) = shelf["contents"].as_array() else { continue };
                let items: Vec<ShelfItem> = contents.iter().filter_map(shelf_item).collect();
                // A shelf that comes twice (the same title) is kept once.
                if !items.is_empty() && !shelves.iter().any(|s| s.title == title) {
                    shelves.push(Shelf { title, items });
                }
            }
        }
        // The newest albums, from YouTube Music's own "new releases" page. Optional: Home stands without it.
        if let Ok(releases) = self.browse(json!({ "browseId": "FEmusic_new_releases_albums" })) {
            let items: Vec<ShelfItem> = releases["contents"]["singleColumnBrowseResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]
                ["sectionListRenderer"]["contents"][0]["gridRenderer"]["items"]
                .as_array()
                .map(|items| items.iter().take(16).filter_map(shelf_item).collect())
                .unwrap_or_default();
            // Home may have its own "New releases" by now: one is plenty.
            if !items.is_empty() && !shelves.iter().any(|s| s.title == "New releases") {
                shelves.push(Shelf { title: "New releases".into(), items });
            }
        }
        anyhow::ensure!(!shelves.is_empty(), "YouTube Music's Home came back empty");
        Ok(shelves)
    }

    /// A list's first page and every page after it: YouTube Music sends a playlist about a
    /// hundred songs at a time, the rest after a continuation token each.
    fn list_pages(&self, first: Value) -> Vec<Value> {
        let mut token = next_page_token(song_list(&first));
        let mut pages = vec![first];
        let mut seen = std::collections::HashSet::new();
        while let Some(t) = token.take() {
            if pages.len() >= MAX_LIST_PAGES || !seen.insert(t.clone()) {
                break;
            }
            // A page that doesn't come keeps what came before: better most of a playlist than none.
            let Ok(page) = self.browse(json!({ "continuation": t })) else { break };
            token = next_page_token(&page);
            pages.push(page);
        }
        pages
    }

    /// The songs on a list's pages, the first page's from its song list only.
    fn songs_of(pages: &[Value], card: Option<&Card>) -> Vec<SearchItem> {
        let mut items = Vec::new();
        for (i, page) in pages.iter().enumerate() {
            list_items(if i == 0 { song_list(page) } else { page }, &mut items);
        }
        // A song in a playlist twice is listed once.
        let mut seen = std::collections::HashSet::new();
        items.into_iter().filter_map(|item| song(item, card)).filter(|t| seen.insert(t.video_id.clone())).collect()
    }

    /// The songs of a playlist or an album page, every page of them. Album tracks carry no
    /// artwork or artist of their own; the card's stand in.
    pub fn tracks(&self, browse_id: &str, card: &Card) -> Result<Vec<SearchItem>> {
        if let Some(mix) = mix_id(browse_id) {
            return self.mix(mix, &card.title);
        }
        let page = self.browse(json!({ "browseId": browse_id }))?;
        let tracks = Self::songs_of(&self.list_pages(page), Some(card));
        anyhow::ensure!(!tracks.is_empty(), "no songs in \"{}\"", card.title);
        Ok(tracks)
    }

    /// Likes, dislikes or clears a song's rating on the account.
    pub fn rate(&self, video_id: &str, rating: Rating) -> Result<()> {
        let endpoint = match rating {
            Rating::Like => "like/like",
            Rating::Dislike => "like/dislike",
            Rating::Indifferent => "like/removelike",
        };
        self.music(endpoint, json!({ "target": { "videoId": video_id } }))?;
        Ok(())
    }

    /// Subscribes to or unsubscribes from an artist channel on YouTube Music (signed in).
    pub fn subscribe(&self, channel_id: &str, subscribed: bool) -> Result<()> {
        let endpoint = if subscribed { "subscription/subscribe" } else { "subscription/unsubscribe" };
        self.music(endpoint, json!({ "channelIds": [channel_id] }))?;
        Ok(())
    }

    /// The account's liked songs, all of them.
    pub fn liked_songs(&self) -> Result<Vec<SearchItem>> {
        let card = Card { title: "Liked music".into(), subtitle: String::new(), thumbnail: None, browse_id: Some("VLLM".into()), video_id: None };
        self.tracks("VLLM", &card)
    }

    /// Where a song's artist and album pages are: (artist, album) browse ids, as its row on
    /// YouTube Music links them.
    pub fn song_links(&self, video_id: &str) -> Result<(Option<String>, Option<String>)> {
        let next = self.music("next", json!({ "videoId": video_id, "isAudioOnly": true }))?;
        let mut panels = Vec::new();
        find_all(&next, "playlistPanelVideoRenderer", &mut panels);
        let mut found = (None, None);
        if let Some(p) = panels.into_iter().find(|p| p["videoId"].as_str() == Some(video_id)) {
            for run in p["longBylineText"]["runs"].as_array().map(Vec::as_slice).unwrap_or_default() {
                let browse = &run["navigationEndpoint"]["browseEndpoint"];
                let Some(id) = browse["browseId"].as_str() else { continue };
                let kind = browse["browseEndpointContextSupportConfigs"]["browseEndpointContextMusicConfig"]["pageType"].as_str().unwrap_or("");
                if kind.ends_with("ARTIST") || id.starts_with("UC") {
                    found.0.get_or_insert_with(|| id.to_string());
                } else if kind.ends_with("ALBUM") || id.starts_with("MPRE") {
                    found.1.get_or_insert_with(|| id.to_string());
                }
            }
        }
        Ok(found)
    }

    /// The artist best matching `name`, from YouTube Music's artist search.
    pub fn find_artist(&self, name: &str) -> Result<Option<String>> {
        let page = self.music("search", json!({ "query": name, "params": "EgWKAQIgAWoKEAkQBRAKEAMQBA%3D%3D" }))?;
        let mut items = Vec::new();
        list_items(&page, &mut items);
        let artists: Vec<(String, String)> = items
            .iter()
            .filter_map(|item| {
                let id = item["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str()?;
                Some((runs(&item["flexColumns"][0]["musicResponsiveListItemFlexColumnRenderer"]["text"]), id.to_string()))
            })
            .collect();
        let wanted = name.to_lowercase();
        Ok(artists.iter().find(|(n, _)| n.to_lowercase() == wanted).or(artists.first()).map(|(_, id)| id.clone()))
    }

    /// The album best matching `name` by `artist`, from YouTube Music's album search.
    pub fn find_album(&self, name: &str, artist: &str) -> Result<Option<String>> {
        let page = self.music("search", json!({ "query": format!("{name} {artist}"), "params": "EgWKAQIYAWoKEAkQBRAKEAMQBA%3D%3D" }))?;
        let mut items = Vec::new();
        list_items(&page, &mut items);
        let albums: Vec<(String, String)> = items
            .iter()
            .filter_map(|item| {
                let id = item["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str()?;
                Some((runs(&item["flexColumns"][0]["musicResponsiveListItemFlexColumnRenderer"]["text"]), id.to_string()))
            })
            .collect();
        let wanted = name.to_lowercase();
        Ok(albums.iter().find(|(n, _)| n.to_lowercase() == wanted).or(albums.first()).map(|(_, id)| id.clone()))
    }

    /// An artist's page: top songs, then their albums, singles and the like.
    pub fn artist(&self, browse_id: &str) -> Result<ArtistPage> {
        let page = self.browse(json!({ "browseId": browse_id }))?;
        let header = ["musicImmersiveHeaderRenderer", "musicVisualHeaderRenderer", "musicHeaderRenderer"]
            .iter()
            .map(|k| &page["header"][*k])
            .find(|h| h.is_object())
            .cloned()
            .unwrap_or(Value::Null);
        let sub_btn = &header["subscriptionButton"]["subscribeButtonRenderer"];
        let mut out = ArtistPage {
            name: runs(&header["title"]),
            subtitle: runs(&sub_btn["longSubscriberCountText"])
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            thumbnail: last_thumbnail(&header["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]),
            channel_id: sub_btn["channelId"]
                .as_str()
                .map(str::to_string)
                .or_else(|| browse_id.starts_with("UC").then(|| browse_id.to_string())),
            subscribed: sub_btn["subscribed"].as_bool().unwrap_or(false),
            ..Default::default()
        };
        if out.subtitle.is_empty() {
            out.subtitle = runs(&sub_btn["subscriberCountText"]);
        }
        // "Mix" (startRadioButton, RDEM…) and "Shuffle" (playButton, RDAO…); failing those,
        // any watch link in the header with the same kind of id.
        out.radio = button_playlist(&header["startRadioButton"]);
        out.shuffle = button_playlist(&header["playButton"]).filter(|id| id.starts_with("RD"));
        if out.radio.is_none() || out.shuffle.is_none() {
            let mut ids = Vec::new();
            find_all(&header, "playlistId", &mut ids);
            let ids: Vec<&str> = ids.into_iter().filter_map(Value::as_str).collect();
            if out.radio.is_none() {
                out.radio = ids.iter().find(|id| id.starts_with("RDEM")).map(|id| id.to_string());
            }
            if out.shuffle.is_none() {
                out.shuffle = ids.iter().find(|id| id.starts_with("RDAO")).map(|id| id.to_string());
            }
        }
        let sections = page["contents"]["singleColumnBrowseResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]["sectionListRenderer"]["contents"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for section in &sections {
            if let Some(shelf) = section.get("musicShelfRenderer") {
                let mut items = Vec::new();
                list_items(shelf, &mut items);
                out.songs.extend(items.into_iter().filter_map(|item| song(item, None)));
            } else if let Some(shelf) = section.get("musicCarouselShelfRenderer") {
                let title = runs(&shelf["header"]["musicCarouselShelfBasicHeaderRenderer"]["title"]);
                let lower = title.to_lowercase();
                // Videos play as songs elsewhere; podcasts and episodes don't play here.
                if lower.is_empty() || ["video", "podcast", "episode"].iter().any(|w| lower.contains(w)) {
                    continue;
                }
                let cards: Vec<Card> = shelf["contents"]
                    .as_array()
                    .map(|items| items.iter().filter_map(|i| i.get("musicTwoRowItemRenderer")).filter_map(any_card).collect())
                    .unwrap_or_default();
                if !cards.is_empty() {
                    out.shelves.push((title, cards));
                }
            }
        }
        anyhow::ensure!(!out.name.is_empty() || !out.songs.is_empty(), "YouTube Music has no page for this artist");
        Ok(out)
    }

    /// An album's or playlist's header and songs.
    pub fn collection(&self, browse_id: &str, card: &Card) -> Result<(Header, Vec<SearchItem>)> {
        // A saved mix has no page to list: it's a radio, played from its start.
        if let Some(mix) = mix_id(browse_id) {
            let tracks = self.mix(mix, &card.title)?;
            let header = Header { title: card.title.clone(), subtitle: card.subtitle.clone(), byline: String::new(), thumbnail: card.thumbnail.clone() };
            return Ok((header, tracks));
        }
        let page = self.browse(json!({ "browseId": browse_id }))?;
        let mut headers = Vec::new();
        for key in ["musicResponsiveHeaderRenderer", "musicDetailHeaderRenderer", "musicEditablePlaylistDetailHeaderRenderer"] {
            find_all(&page, key, &mut headers);
            if !headers.is_empty() {
                break;
            }
        }
        let h = headers.first().copied().cloned().unwrap_or(Value::Null);
        // An editable playlist's header wraps the plain one.
        let h = if h["header"]["musicResponsiveHeaderRenderer"].is_object() { h["header"]["musicResponsiveHeaderRenderer"].clone() } else { h };
        let thumbnail = last_thumbnail(&h["thumbnail"]["musicThumbnailRenderer"]["thumbnail"])
            .or_else(|| last_thumbnail(&h["thumbnail"]["croppedSquareThumbnailRenderer"]["thumbnail"]))
            .or_else(|| card.thumbnail.clone());
        let header = Header {
            title: Some(runs(&h["title"])).filter(|t| !t.is_empty()).unwrap_or_else(|| card.title.clone()),
            subtitle: Some(runs(&h["subtitle"])).filter(|t| !t.is_empty()).unwrap_or_else(|| card.subtitle.clone()),
            byline: runs(&h["straplineTextOne"]),
            thumbnail,
        };
        let filled = Card { thumbnail: header.thumbnail.clone(), subtitle: if header.byline.is_empty() { card.subtitle.clone() } else { header.byline.clone() }, ..card.clone() };
        let tracks = Self::songs_of(&self.list_pages(page), Some(&filled));
        anyhow::ensure!(!tracks.is_empty(), "no songs in \"{}\"", header.title);
        Ok((header, tracks))
    }

    /// A mix's songs (a saved "Mix" or radio, `RD…`): its first batch or two as YouTube
    /// Music's player would queue them.
    fn mix(&self, playlist_id: &str, title: &str) -> Result<Vec<SearchItem>> {
        let first = self.radio_start(None, playlist_id, None)?;
        let mut tracks = first.tracks;
        if let Some(token) = first.continuation.filter(|_| tracks.len() < 80) {
            if let Ok(more) = self.radio_more(playlist_id, &token) {
                tracks.extend(more.tracks);
            }
        }
        let mut seen = std::collections::HashSet::new();
        tracks.retain(|t| seen.insert(t.video_id.clone()));
        anyhow::ensure!(!tracks.is_empty(), "no songs in \"{title}\"");
        Ok(tracks)
    }

    /// Songs like this one: YouTube Music's radio for it (what plays after it on autoplay),
    /// without the song itself.
    pub fn radio(&self, video_id: &str) -> Result<Vec<SearchItem>> {
        let page = self.radio_start(Some(video_id), &format!("RDAMVM{video_id}"), None)?;
        Ok(page.tracks.into_iter().filter(|t| t.video_id != video_id).collect())
    }

    /// The first batch of a radio (about 50 songs; about 25 for a chip's flavour), with its
    /// chips and continuation. `playlist_id` is a song's radio ("RDAMVM" + its id, with
    /// `video_id` the song, which then comes first), an artist's (`ArtistPage::radio` or
    /// `shuffle`, no `video_id` needed), or a chip's `playlist_id` and `params`.
    pub fn radio_start(&self, video_id: Option<&str>, playlist_id: &str, params: Option<&str>) -> Result<RadioPage> {
        let mut body = json!({ "playlistId": playlist_id, "isAudioOnly": true, "enablePersistentPlaylistPanel": true });
        if let Some(id) = video_id {
            body["videoId"] = json!(id);
        }
        if let Some(params) = params {
            body["params"] = json!(params);
        }
        let next = self.music("next", body)?;
        Ok(radio_page(&next, playlist_id))
    }

    /// The radio's next batch, from the last batch's `continuation`. Batches overlap a lot:
    /// YouTube Music's own player drops what its queue already holds, and so must the caller
    /// (`radio_refill` does). No chips here.
    pub fn radio_more(&self, playlist_id: &str, continuation: &str) -> Result<RadioPage> {
        let next = self.music(
            "next",
            json!({ "playlistId": playlist_id, "continuation": continuation, "isAudioOnly": true, "enablePersistentPlaylistPanel": true }),
        )?;
        Ok(radio_page(&next, playlist_id))
    }

    /// At least `want` songs for a radio that aren't in `seen` (the queue so far, oldest
    /// first), when YouTube Music has them. Follows the continuation for up to three batches;
    /// a radio's continuations circle the same hundred-odd songs, so when they run dry it
    /// starts over from the newest song (`seen`'s last, or the newest new one), in the same
    /// flavour when `chip` names one ("Discover"...).
    ///
    /// The answer holds only new songs. Its `playlist_id` and `continuation` are where to go
    /// on from (a new radio's after a restart); its `chips` are empty unless the radio
    /// restarted, and then they're the new seed's, with the one now playing selected.
    pub fn radio_refill(
        &self,
        playlist_id: &str,
        continuation: Option<&str>,
        chip: Option<&str>,
        seen: &[String],
        want: usize,
    ) -> Result<RadioPage> {
        let fresh = |tracks: Vec<SearchItem>, out: &mut Vec<SearchItem>| {
            for t in tracks {
                if !seen.contains(&t.video_id) && !out.iter().any(|o| o.video_id == t.video_id) {
                    out.push(t);
                }
            }
        };
        let mut out = RadioPage { playlist_id: playlist_id.to_string(), continuation: continuation.map(str::to_string), ..Default::default() };
        let mut tries = 0;
        while out.tracks.len() < want && tries < 3 {
            let Some(token) = out.continuation.clone() else { break };
            tries += 1;
            let before = out.tracks.len();
            let batch = self.radio_more(&out.playlist_id, &token)?;
            out.continuation = batch.continuation;
            fresh(batch.tracks, &mut out.tracks);
            // Two new songs or fewer in a batch of ~50: the radio has gone round.
            if out.tracks.len() - before <= 2 {
                break;
            }
        }
        if out.tracks.len() >= want {
            return Ok(out);
        }
        let Some(seed) = out.tracks.last().map(|t| t.video_id.clone()).or_else(|| seen.last().cloned()) else {
            return Ok(out);
        };
        let mut page = self.radio_start(Some(seed.as_str()), &format!("RDAMVM{seed}"), None)?;
        let wanted = chip.map(str::to_lowercase).filter(|c| c != "all");
        if let Some(found) = wanted.and_then(|w| page.chips.iter().find(|c| c.label.to_lowercase() == w && !c.selected).cloned()) {
            let flavoured = self.radio_start(None, &found.playlist_id, found.params.as_deref())?;
            let chips = page.chips.iter().map(|c| RadioChip { selected: c.label == found.label, ..c.clone() }).collect();
            page = RadioPage { chips, ..flavoured };
        }
        fresh(page.tracks, &mut out.tracks);
        out.playlist_id = page.playlist_id;
        out.continuation = page.continuation;
        out.chips = page.chips;
        Ok(out)
    }

    /// The playlist id of an artist's radio ("RDEM…"), from their page.
    #[allow(dead_code)] // examples/radio.rs; the artist page carries its radio already
    pub fn artist_radio(&self, browse_id: &str) -> Result<Option<String>> {
        Ok(self.artist(browse_id)?.radio)
    }

    /// Every version of a track's lyrics there is, best first: synced from the lyric
    /// providers, then YouTube Music's own text. All asked at once.
    pub fn lyrics(&self, video_id: &str, title: &str, artist: &str, length: Duration) -> Vec<crate::lyrics::Found> {
        let (mut found, plain) = std::thread::scope(|s| {
            let plain = s.spawn(|| self.plain_lyrics(video_id).ok().flatten());
            (crate::lyrics::fetch(&self.http, title, artist, length), plain.join().ok().flatten())
        });
        found.extend(plain);
        found
    }

    /// The "Lyrics" tab of the song on YouTube Music: whole text, no timings.
    fn plain_lyrics(&self, video_id: &str) -> Result<Option<crate::lyrics::Found>> {
        let next: Value = self
            .http
            .post("https://music.youtube.com/youtubei/v1/next?prettyPrint=false")
            .header("origin", "https://music.youtube.com")
            .json(&json!({
                "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": WEB_REMIX_VERSION, "hl": "en", "gl": "ID" } },
                "videoId": video_id,
            }))
            .send()?
            .error_for_status()?
            .json()?;
        // The lyrics tab's page id starts "MPLYt".
        let text = next.to_string();
        let Some(at) = text.find("\"MPLYt") else { return Ok(None) };
        let browse_id: String = text[at + 1..].chars().take_while(|c| *c != '"').collect();
        let page = self.browse(json!({ "browseId": browse_id }))?;
        let body = runs(&page["contents"]["sectionListRenderer"]["contents"][0]["musicDescriptionShelfRenderer"]["description"]);
        let lines: Vec<crate::lyrics::Line> = body
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| crate::lyrics::Line::new(Duration::ZERO, l.to_string()))
            .collect();
        Ok((!lines.is_empty()).then_some(crate::lyrics::Found { lines, synced: false, source: "YouTube Music", length: None }))
    }


    /// An image, decoded.
    /// A song's cover, as large as there is: an album's at 800 px, or for a music video its
    /// HD thumbnail (YouTube lists the 480 px one; 1280 px or 640 px ones usually exist).
    pub fn cover_image(&self, url: &str) -> Result<image::RgbaImage> {
        if let Some(id) = url.split("/vi/").nth(1).or_else(|| url.split("/vi_webp/").nth(1)).and_then(|rest| rest.split('/').next()) {
            for name in ["maxresdefault", "sddefault", "hqdefault"] {
                // A missing size answers 404, or a 120 px grey placeholder.
                if let Ok(img) = self.image(&format!("https://i.ytimg.com/vi/{id}/{name}.jpg")) {
                    if img.width() > 200 {
                        return Ok(img);
                    }
                }
            }
        }
        self.image(&sized(url, 800))
    }

    pub fn image(&self, url: &str) -> Result<image::RgbaImage> {
        let bytes = self.http.get(url).send()?.error_for_status()?.bytes()?;
        Ok(image::load_from_memory(&bytes)?.to_rgba8())
    }

    /// The whole stream, in 1 MB ranges.
    pub fn download(&self, stream: &Stream) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(stream.content_length as usize);
        let mut start = 0;
        while start < stream.content_length {
            let end = (start + CHUNK).min(stream.content_length) - 1;
            let chunk = self
                .http
                .get(format!("{}&range={start}-{end}", stream.url))
                .header("user-agent", stream.user_agent)
                .send()?
                .error_for_status()
                .with_context(|| format!("googlevideo refused bytes {start}-{end}"))?
                .bytes()?;
            if chunk.is_empty() {
                bail!("googlevideo sent nothing for bytes {start}-{end}");
            }
            start += chunk.len() as u64;
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}
