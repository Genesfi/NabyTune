//! What the listener keeps: songs played lately (for Listen again and the recommendations
//! built from them), and their own playlists. Kept in %APPDATA%\NabyTune\library.json.

use std::path::PathBuf;

use serde_json::{json, Value};

/// How many played songs are remembered.
const HISTORY: usize = 100;

/// A song as kept on disk: enough to show it and play it again.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub detail: String,
    pub duration: String,
    pub thumbnail: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<Saved>,
}

#[derive(Default)]
pub struct Library {
    /// Most recent first, each song once.
    pub history: Vec<Saved>,
    pub playlists: Vec<Playlist>,
    /// Songs given a heart, newest first (also sent to YouTube Music when signed in).
    pub liked: Vec<Saved>,
    /// Songs turned down: skipped, and left out of autoplay and recommendations.
    pub disliked: Vec<String>,
}

/// What played when NabyTune closed, to pick up from next time.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub queue: Vec<Saved>,
    pub index: usize,
    pub position_ms: u64,
    pub shuffle: bool,
    /// "off", "all" or "one".
    pub repeat: String,
}

fn session_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("NabyTune").join("session.json"))
}

/// Writes `v` to `path`, aside first and then moved over: a crash mid-write can't leave half a file.
fn write_json(path: &std::path::Path, v: &Value) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let aside = path.with_extension("json.tmp");
    if std::fs::write(&aside, serde_json::to_string_pretty(v).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&aside, path);
    }
}

impl Session {
    pub fn load() -> Option<Self> {
        let text = std::fs::read_to_string(session_path()?).ok()?;
        Self::from_json(&serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?)
    }

    pub fn save(&self) {
        if let Some(path) = session_path() {
            write_json(&path, &self.to_json());
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "queue": self.queue.iter().map(Saved::to_json).collect::<Vec<_>>(),
            "index": self.index,
            "positionMs": self.position_ms,
            "shuffle": self.shuffle,
            "repeat": self.repeat,
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let queue: Vec<Saved> = v["queue"].as_array()?.iter().filter_map(Saved::from_json).collect();
        let index = v["index"].as_u64()? as usize;
        (index < queue.len()).then(|| Self {
            queue,
            index,
            position_ms: v["positionMs"].as_u64().unwrap_or(0),
            shuffle: v["shuffle"].as_bool().unwrap_or(false),
            repeat: v["repeat"].as_str().unwrap_or("off").to_string(),
        })
    }
}

fn path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("NabyTune").join("library.json"))
}

impl Saved {
    fn to_json(&self) -> Value {
        json!({
            "videoId": self.video_id,
            "title": self.title,
            "artist": self.artist,
            "detail": self.detail,
            "duration": self.duration,
            "thumbnail": self.thumbnail,
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let text = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let video_id = v["videoId"].as_str().filter(|s| !s.is_empty())?.to_string();
        let artist = crate::youtube::clean_artist(&text("artist"));
        let raw_detail = text("detail");
        let detail = if raw_detail.contains(" • ") {
            let parts: Vec<&str> = raw_detail.split(" • ").collect();
            let album = parts.get(1).unwrap_or(&"");
            if album.is_empty() { artist.clone() } else { format!("{} • {}", artist, album) }
        } else {
            artist.clone()
        };
        Some(Self {
            video_id,
            title: text("title"),
            artist,
            detail,
            duration: text("duration"),
            thumbnail: v["thumbnail"].as_str().map(str::to_string),
        })
    }
}

impl Library {
    pub fn load() -> Self {
        let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) else { return Self::default() };
        let Ok(v) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { return Self::default() };
        let songs = |v: &Value| v.as_array().map(|a| a.iter().filter_map(Saved::from_json).collect()).unwrap_or_default();
        Self {
            history: songs(&v["history"]),
            playlists: v["playlists"]
                .as_array()
                .map(|a| a.iter().map(|p| Playlist { name: p["name"].as_str().unwrap_or("Playlist").to_string(), tracks: songs(&p["tracks"]) }).collect())
                .unwrap_or_default(),
            liked: songs(&v["liked"]),
            disliked: v["disliked"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        }
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        let v = json!({
            "history": self.history.iter().map(Saved::to_json).collect::<Vec<_>>(),
            "playlists": self.playlists.iter().map(|p| json!({
                "name": p.name,
                "tracks": p.tracks.iter().map(Saved::to_json).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "liked": self.liked.iter().map(Saved::to_json).collect::<Vec<_>>(),
            "disliked": self.disliked,
        });
        write_json(&path, &v);
    }

    pub fn is_liked(&self, video_id: &str) -> bool {
        self.liked.iter().any(|s| s.video_id == video_id)
    }

    pub fn is_disliked(&self, video_id: &str) -> bool {
        self.disliked.iter().any(|id| id == video_id)
    }

    /// Hearts a song (or takes the heart back). Whether it's liked now. Liking undoes a dislike.
    pub fn set_liked(&mut self, song: Saved, liked: bool) {
        self.liked.retain(|s| s.video_id != song.video_id);
        if liked {
            self.disliked.retain(|id| *id != song.video_id);
            self.liked.insert(0, song);
        }
    }

    /// Turns a song down (or takes that back). Disliking takes away a heart.
    pub fn set_disliked(&mut self, video_id: &str, disliked: bool) {
        self.disliked.retain(|id| id != video_id);
        if disliked {
            self.liked.retain(|s| s.video_id != video_id);
            self.disliked.push(video_id.to_string());
        }
    }

    /// A song started playing: first in the history, once.
    pub fn played(&mut self, song: Saved) {
        self.history.retain(|s| s.video_id != song.video_id);
        self.history.insert(0, song);
        self.history.truncate(HISTORY);
    }

    /// A new playlist named "Playlist N" (the first N not taken); its index.
    pub fn new_playlist(&mut self) -> usize {
        let name = (1..).map(|n| format!("Playlist {n}")).find(|name| !self.playlists.iter().any(|p| &p.name == name)).unwrap();
        self.playlists.push(Playlist { name, tracks: Vec::new() });
        self.playlists.len() - 1
    }

    /// Adds a song to playlist `i`, unless it's there already. Whether it was added.
    pub fn add(&mut self, i: usize, song: Saved) -> bool {
        match self.playlists.get_mut(i) {
            Some(p) if !p.tracks.iter().any(|s| s.video_id == song.video_id) => {
                p.tracks.push(song);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str) -> Saved {
        Saved { video_id: id.into(), title: id.into(), artist: "A".into(), detail: "A".into(), duration: "3:00".into(), thumbnail: None }
    }

    #[test]
    fn keeps_history_recent_and_unique() {
        let mut lib = Library::default();
        lib.played(song("a"));
        lib.played(song("b"));
        lib.played(song("a"));
        let ids: Vec<&str> = lib.history.iter().map(|s| s.video_id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn names_and_fills_playlists() {
        let mut lib = Library::default();
        let first = lib.new_playlist();
        let second = lib.new_playlist();
        assert_eq!(lib.playlists[first].name, "Playlist 1");
        assert_eq!(lib.playlists[second].name, "Playlist 2");
        assert!(lib.add(first, song("a")));
        assert!(!lib.add(first, song("a")));
        assert_eq!(lib.playlists[first].tracks.len(), 1);
    }

    #[test]
    fn round_trips_through_json() {
        let s = song("x");
        assert_eq!(Saved::from_json(&s.to_json()), Some(s));
        let session = Session { queue: vec![song("a"), song("b")], index: 1, position_ms: 61_500, shuffle: true, repeat: "all".into() };
        assert_eq!(Session::from_json(&session.to_json()), Some(session));
        // An index past the queue is no session.
        assert_eq!(Session::from_json(&json!({ "queue": [], "index": 0 })), None);
    }

    #[test]
    fn likes_and_dislikes_exclude_each_other() {
        let mut lib = Library::default();
        lib.set_liked(song("a"), true);
        assert!(lib.is_liked("a"));
        lib.set_disliked("a", true);
        assert!(!lib.is_liked("a") && lib.is_disliked("a"));
        lib.set_liked(song("a"), true);
        assert!(lib.is_liked("a") && !lib.is_disliked("a"));
        lib.set_liked(song("a"), false);
        assert!(!lib.is_liked("a"));
    }
}
