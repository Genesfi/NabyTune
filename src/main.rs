//! NabyTune, natively: GPUI CE + gpui-motion for the UI, a Rust engine for the sound.
//!
//! The layout follows NabyTune's WebView design (web/app/src/shell: TitleBar,
//! DockPlayer, desktop.css): a title bar with tabs and search on the window's
//! Mica, the page on a translucent layer, an acrylic dock with real backdrop
//! blur, and a side panel for lyrics and the queue. It searches YouTube Music
//! and plays for real (youtube.rs → decoder.rs → audio.rs).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;
mod addon;
mod ambient;
mod appearance;
mod audio;
mod bridge;
mod decoder;
mod desktop_lyrics;
mod dsp;
mod eq;
mod i18n;
mod integrations;
mod library;
mod library_page;
mod local;
mod local_files;
mod lyrics;
mod lyrics_sources;
mod media;
mod mini;
mod discord;
mod onboarding;
mod pages;
mod radio;
mod romaji;
mod search_box;
mod browse;
mod chrome;
mod lyrics_view;
mod player;
mod stage;
mod settings;
mod settings_page;
mod shelf;
mod smooth_move;
mod snapshot;
mod stats;
mod stats_page;
mod track_menu;
mod taskbar;
mod tray;
mod updater;
mod updates;
mod visualizer;
mod youtube;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use appearance::{accent, accent_wash, on_accent, Material};
use gpui::{
    actions, deferred, div, img, prelude::*, AnimationExt as _, px, relative, rgb, rgba, size, AnyElement, App, Bounds, Context, Entity, FocusHandle,
    Focusable, FontWeight, KeyBinding, ObjectFit, Pixels, RenderImage, SharedString, StyledImage, Subscription, TitlebarOptions,
    Window, WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowOptions,
};
use gpui_ce_motion::{Easing, GestureState, GestureTargets, Motion, MotionState, Spring, Target, Transition};
use i18n::{songs, t, tf};
use image::RgbaImage;
use chrome::{action, artwork, brand_mark, clock_seconds, icon, short_error, volume_glyph};
use search_box::{Dismissed, SearchBox, Submitted};

// ---- NabyTune's desktop tokens (web/app/src/styles/desktop.css, dark) ---------------------
pub const ICONS: &str = "Segoe Fluent Icons";
const TEXT: &str = "Segoe UI Variable Text";
const DISPLAY: &str = "Segoe UI Variable Display";
/// The page's layer over the backdrop: rgb(58 58 58 / 0.3).
const LAYER: u32 = 0x3a3a_3a4d;
/// Hairlines: white at 6%.
const STROKE: u32 = 0xffff_ff0f;
const SUBTLE: u32 = 0xffff_ff0d;
const SUBTLE_STRONG: u32 = 0xffff_ff14;
/// The dock: rgb(44 44 44 / 0.6) over a 28 px backdrop blur.
const DOCK: u32 = 0x2c2c_2c99;
/// Panels: rgb(40 40 40 / 0.8) over a 30 px backdrop blur.
const GLASS: u32 = 0x2828_28cc;
const SECONDARY: u32 = 0xffff_ff99;
const TERTIARY: u32 = 0xffff_ff66;
/// The window's ground with the Solid material: Windows 11's dark app background.
const SOLID: u32 = 0x202020;
const PANEL: f32 = 340.0;
const SEEK: f32 = 440.0;
/// The dock's volume slider.
const VOLUME: f32 = 92.0;
/// The full-screen player's rail: its distance from the right edge, and the room kept
/// clear for it (lyrics wrap before they reach it).
const RAIL_EDGE: f32 = 20.0;
const RAIL_ROOM: f32 = RAIL_EDGE + 34.0 + 28.0;

actions!(
    nabytune,
    [
        FocusSearch,
        TogglePlay,
        CloseFullScreen,
        SeekBack,
        SeekForward,
        VolumeUp,
        VolumeDown,
        NextTrack,
        PreviousTrack,
        ToggleMute,
        LikeSong,
        ToggleFullScreen,
        OpenSettings,
        GoBack,
        MiniPlayer,
        LyricsEarlier,
        LyricsLater,
        ToggleShuffle,
        CycleRepeat,
        DesktopLyrics
    ]
);

/// Where the one-key shortcuts work: anywhere in the app but a text field.
const PLAYER_KEYS: &str = "NabyTune && !SearchBox";

/// The keyboard shortcuts, as Settings lists them: (keys, what they do).
const SHORTCUTS: [(&str, &str); 18] = [
    ("Space", "Play or pause"),
    ("← / →", "Back or ahead 5 seconds"),
    ("↑ / ↓", "Volume up or down"),
    ("Ctrl + → / ←", "Next or previous song"),
    ("M", "Mute"),
    ("L", "Like the song"),
    ("S", "Shuffle"),
    ("R", "Repeat"),
    ("F", "Full screen"),
    ("Esc", "Leave full screen"),
    ("[ / ]", "Lyrics earlier or later"),
    ("Ctrl + M", "Mini player"),
    ("Ctrl + D", "Lyrics on the desktop"),
    ("Ctrl + F", "Search"),
    ("Ctrl + P", "Play or pause"),
    ("Ctrl + ,", "Settings"),
    ("Alt + ←", "Back"),
    ("Mouse back", "Back"),
];

/// How a lyric column is set: the side panel's, or the full-screen player's.
struct LyricStyle {
    text: f32,
    line_height: f32,
    gap: f32,
    pad: f32,
    /// Where the sung line sits, as a fraction of the view's height.
    anchor: f32,
    /// Blur per line of distance from the sung line, and its ceiling (px).
    blur_step: f32,
    blur_max: f32,
}

const PANEL_LYRICS: LyricStyle =
    LyricStyle { text: 22.0, line_height: 30.0, gap: 16.0, pad: 24.0, anchor: 0.33, blur_step: 0.9, blur_max: 3.0 };
/// Lines further than this from the sung one are off screen: they skip the blur pass.
const BLUR_REACH: usize = 7;
const BLUR_TWEEN: Duration = Duration::from_millis(260);

/// One value per lyric line (its blur, its scale), eased from where it was towards where
/// it's going.
#[derive(Clone, Copy)]
struct Tween {
    from: f32,
    to: f32,
    since: Instant,
}

impl Tween {
    fn at(value: f32) -> Self {
        Self { from: value, to: value, since: Instant::now() - BLUR_TWEEN }
    }
    fn value(&self) -> f32 {
        let t = (self.since.elapsed().as_secs_f32() / BLUR_TWEEN.as_secs_f32()).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.from + (self.to - self.from) * eased
    }
    fn settled(&self) -> bool {
        self.since.elapsed() >= BLUR_TWEEN
    }
    /// Aims at `to`, starting from the current value so a change mid-way doesn't jump.
    fn aim(&mut self, to: f32) {
        if (to - self.to).abs() > 0.01 {
            *self = Self { from: self.value(), to, since: Instant::now() };
        }
    }
}

/// How synced lyrics look and move; picked from the "Aa" menu and saved.
#[derive(Clone, Copy, PartialEq)]
enum Look {
    /// Left-aligned, the far lines out of focus: Apple Music's way.
    Focus,
    /// The sung line lights word by word, spread across its time.
    Karaoke,
    /// Centred, the sung line a size up.
    Center,
    /// One big line at a time, the next one waiting underneath.
    Single,
    /// Smaller, lighter, no blur: lyrics that keep to the background.
    Minimal,
    /// The sung line in the accent.
    Neon,
}

impl Look {
    const ALL: [Look; 6] = [Look::Focus, Look::Karaoke, Look::Center, Look::Single, Look::Minimal, Look::Neon];

    fn name(self) -> &'static str {
        t(match self {
            Look::Focus => "Focus",
            Look::Karaoke => "Karaoke",
            Look::Center => "Center",
            Look::Single => "One line",
            Look::Minimal => "Minimal",
            Look::Neon => "Neon",
        })
    }
    fn hint(self) -> &'static str {
        t(match self {
            Look::Focus => "Far lines fade away",
            Look::Karaoke => "Words light up one by one",
            Look::Center => "Centered, current line grows",
            Look::Single => "One big line, rising in turn",
            Look::Minimal => "Small and quiet",
            Look::Neon => "Current line glows lime",
        })
    }
    fn key(self) -> &'static str {
        match self {
            Look::Focus => "focus",
            Look::Karaoke => "karaoke",
            Look::Center => "center",
            Look::Single => "single",
            Look::Minimal => "minimal",
            Look::Neon => "neon",
        }
    }
    fn from_key(key: &str) -> Option<Look> {
        Look::ALL.into_iter().find(|l| l.key() == key)
    }
    /// The looks with several lines on screen, where the far ones can go out of focus.
    fn blurs(self) -> bool {
        !matches!(self, Look::Single | Look::Minimal)
    }
}

/// How the full-screen player arranges the song; picked from its layout menu and saved.
#[derive(Clone, Copy, PartialEq)]
enum StageLayout {
    Side,
    UpNext,
    Center,
    Bleed,
    Lyrics,
    Card,
}

impl StageLayout {
    const ALL: [StageLayout; 6] = [
        StageLayout::Side,
        StageLayout::UpNext,
        StageLayout::Center,
        StageLayout::Bleed,
        StageLayout::Lyrics,
        StageLayout::Card,
    ];
    fn name(self) -> &'static str {
        t(match self {
            StageLayout::Side => "Side by side",
            StageLayout::UpNext => "Up next",
            StageLayout::Center => "Center",
            StageLayout::Bleed => "Full cover",
            StageLayout::Lyrics => "Lyrics only",
            StageLayout::Card => "Card",
        })
    }
    fn key(self) -> &'static str {
        match self {
            StageLayout::Side => "side",
            StageLayout::UpNext => "upnext",
            StageLayout::Center => "center",
            StageLayout::Bleed => "bleed",
            StageLayout::Lyrics => "lyrics",
            StageLayout::Card => "card",
        }
    }
    fn from_key(key: &str) -> Option<Self> {
        if key == "vinyl" {
            return Some(StageLayout::UpNext);
        }
        Self::ALL.into_iter().find(|l| l.key() == key)
    }
}

/// What the full-screen player stands on.
#[derive(Clone, Copy, PartialEq)]
enum StageBackdrop {
    Ambient,
    SoftCover,
    Cover,
    Gradient,
    Dark,
}

impl StageBackdrop {
    const ALL: [StageBackdrop; 5] =
        [StageBackdrop::Ambient, StageBackdrop::SoftCover, StageBackdrop::Cover, StageBackdrop::Gradient, StageBackdrop::Dark];
    fn name(self) -> &'static str {
        t(match self {
            StageBackdrop::Ambient => "Ambient",
            StageBackdrop::SoftCover => "Blurred cover",
            StageBackdrop::Cover => "Cover",
            StageBackdrop::Gradient => "Gradient",
            StageBackdrop::Dark => "Dark",
        })
    }
    fn key(self) -> &'static str {
        match self {
            StageBackdrop::Ambient => "ambient",
            StageBackdrop::SoftCover => "soft",
            StageBackdrop::Cover => "cover",
            StageBackdrop::Gradient => "gradient",
            StageBackdrop::Dark => "dark",
        }
    }
    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.key() == key)
    }
}

/// A song's big cover, made ready: for the dock and the full-screen player, and the
/// backgrounds and accent colour drawn from it.
#[derive(Clone)]
struct BigCover {
    cover: Arc<RenderImage>,
    cover_large: Arc<RenderImage>,
    glow: Arc<RenderImage>,
    art: StageArt,
    colour: Option<u32>,
    vinyl_base: Arc<RgbaImage>,
}

impl BigCover {
    fn from(image: &RgbaImage) -> Self {
        Self {
            cover: ambient::cover(image, 224),
            cover_large: ambient::cover(image, STAGE_ART),
            glow: ambient::ambient(image, 0.85),
            art: StageArt { soft: ambient::soft(image), colors: ambient::palette(image) },
            colour: ambient::accent_from(image),
            vinyl_base: ambient::vinyl_base(image, 256),
        }
    }
}

/// How many big covers are kept: the last song and the next one (each ~4 MB).
const BIG_COVERS: usize = 2;

/// Backgrounds made from the current cover: lightly blurred, and its two main colours.
#[derive(Clone)]
struct StageArt {
    soft: Arc<RenderImage>,
    colors: [u32; 2],
}

/// Where an "Aa" menu opens from.
#[derive(Clone, Copy, PartialEq)]
enum LookAnchor {
    Stage,
    Panel,
}
const STAGE_COVER: f32 = 440.0;
/// The full-screen cover's pixel size: matches YouTube's 544 px source cover.
const STAGE_ART: u32 = 544;

#[derive(Clone)]
struct Track {
    video_id: String,
    title: SharedString,
    artist: SharedString,
    detail: SharedString,
    duration: SharedString,
    thumbnail: Option<String>,
    /// Artwork bundled with the prototype (covers/), when there is any.
    local: Option<Arc<RgbaImage>>,
}

impl From<&library::Saved> for Track {
    fn from(s: &library::Saved) -> Self {
        let artist = youtube::clean_artist(&s.artist);
        let detail = if s.detail.contains(" • ") {
            let parts: Vec<&str> = s.detail.split(" • ").collect();
            let album = parts.get(1).unwrap_or(&"");
            if album.is_empty() { artist.clone() } else { format!("{} • {}", artist, album) }
        } else {
            artist.clone()
        };
        Self {
            video_id: s.video_id.clone(),
            title: s.title.clone().into(),
            artist: artist.into(),
            detail: detail.into(),
            duration: s.duration.clone().into(),
            thumbnail: s.thumbnail.clone(),
            local: None,
        }
    }
}

impl From<&Track> for library::Saved {
    fn from(t: &Track) -> Self {
        let artist = youtube::clean_artist(&t.artist.to_string());
        Self {
            video_id: t.video_id.clone(),
            title: t.title.to_string(),
            artist: artist.clone(),
            detail: t.detail.to_string(),
            duration: t.duration.to_string(),
            thumbnail: t.thumbnail.clone(),
        }
    }
}

/// Where the "add to playlist" menu is open: for which song, at which point in the window.
struct PlaylistMenu {
    track: Track,
    at: gpui::Point<Pixels>,
}

impl From<youtube::SearchItem> for Track {
    fn from(s: youtube::SearchItem) -> Self {
        let artist = youtube::clean_artist(&s.artist);
        let detail = if s.album.is_empty() { artist.clone() } else { format!("{} • {}", artist, s.album) };
        Self {
            video_id: s.video_id,
            title: s.title.into(),
            artist: artist.into(),
            detail: detail.into(),
            duration: s.duration.into(),
            thumbnail: s.thumbnail,
            local: None,
        }
    }
}

/// A track's small artwork: for rows, and (blurred) for the backdrop.
struct Art {
    small: Arc<RenderImage>,
}

/// Artwork kept in memory at most, in bytes of pixels: past it, the longest-kept goes first
/// (a few hundred thumbnails and a hundred or so cards, more than any page shows at once).
const ART_BUDGET: usize = 64 << 20;

/// Cards show at 168 px: their covers are fetched a little larger and kept at 256 px, sharp up
/// to 150% display scaling (twice the size, as before, cost twice the memory for every cover
/// on a Home of twenty shelves).
const CARD_FETCH: u32 = 226;
const CARD_KEEP: u32 = 176;

/// Fetches what `start` needs to play a track: lossless from the addons when one has it,
/// else YouTube's stream; and its big cover when `big` names one to fetch.
fn fetch_loaded(youtube: &youtube::YouTube, request: &LoadRequest) -> anyhow::Result<Loaded> {
    // The listener's own file: read from disk, its cover from its tags or folder.
    if let Some(path) = local_files::path_of(&request.video_id) {
        let bytes = local_files::read(&path)?;
        let big = local_files::cover(&path).map(|img| BigCover::from(&img));
        let kind = local_files::extension_hint(&path).unwrap_or_default().to_uppercase();
        return anyhow::Ok(Loaded {
            bytes,
            length: request.seconds.map_or(Duration::ZERO, Duration::from_secs_f64),
            volume: 1.0,
            cover: big.as_ref().map(|b| b.cover.clone()),
            cover_large: big.as_ref().map(|b| b.cover_large.clone()),
            quality: tf("{} · on this PC", &[&kind]).into(),
            lossless_from: None,
            big,
        });
    }
    // Large enough for the full-screen player's biggest cover, and a full-bleed backdrop.
    // The backgrounds too: the song may have started before its row's artwork arrived.
    let big = request.big.as_ref().and_then(|url| youtube.cover_image(url).ok()).map(|img| BigCover::from(&img));
    let (cover, cover_large) = (big.as_ref().map(|b| b.cover.clone()), big.as_ref().map(|b| b.cover_large.clone()));
    let http = youtube.client();
    for addon in &request.addons {
        let found = (|| -> anyhow::Result<Option<Loaded>> {
            let Some(hit) = addon::find(&http, addon, &request.title, &request.artist, request.seconds, request.tier)? else { return Ok(None) };
            let stream = addon.stream(&http, &hit.id, request.tier)?;
            let bytes = addon::download(&http, &stream)?;
            let length = hit.duration.or(request.seconds).map_or(Duration::ZERO, Duration::from_secs_f64);
            audio::log(format!("lossless: {} from {} (listed as {}; {} bytes)", stream.label(), addon.name, hit.quality, bytes.len()));
            Ok(Some(Loaded {
                bytes,
                length,
                volume: 1.0,
                cover: cover.clone(),
                cover_large: cover_large.clone(),
                quality: stream.label().into(),
                lossless_from: Some(addon.name.clone()),
                big: big.clone(),
            }))
        })();
        match found {
            Ok(Some(loaded)) => return anyhow::Ok(loaded),
            Ok(None) => audio::log(format!("lossless: \"{}\" isn't on {}", request.title, addon.name)),
            Err(e) => audio::log(format!("lossless: {} failed: {e:#}", addon.name)),
        }
    }
    let (stream, stand_in) = match youtube.resolve(&request.video_id) {
        Ok(stream) => (stream, false),
        // Not playable here (taken down, blocked in this country, or refused to this player):
        // the same song from another upload, when YouTube Music has one.
        Err(e) => match stand_in(youtube, request) {
            Some(stream) => (stream, true),
            None => return Err(e),
        },
    };
    let bytes = youtube.download(&stream)?;
    // YouTube's loudness normalisation: louder-than-reference tracks are turned down.
    let volume = 10f32.powf(-stream.loudness_db.max(0.0) / 20.0);
    let quality: SharedString = if stand_in { t("AAC 128 kbps · another upload").into() } else { "AAC 128 kbps".into() };
    anyhow::Ok(Loaded { bytes, length: stream.length, volume, cover, cover_large, quality, lossless_from: None, big })
}

/// Another upload of the song `request` names, when its own won't play: YouTube Music's search
/// for its title and artist, taking the first result that is the same song (its title in the
/// result's, the artist too, and as long within 20 seconds when both lengths are known).
fn stand_in(youtube: &youtube::YouTube, request: &LoadRequest) -> Option<youtube::Stream> {
    let fold = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>();
    let (title, artist) = (fold(&request.title), fold(&request.artist));
    let first_artist = artist.split_whitespace().next().unwrap_or("").to_string();
    let results = youtube.search(&format!("{} {}", request.title, request.artist)).ok()?;
    for item in results.iter().filter(|i| i.video_id != request.video_id).take(6) {
        let same_title = fold(&item.title).contains(title.trim()) || title.contains(fold(&item.title).trim());
        let same_artist = first_artist.is_empty() || fold(&item.artist).contains(&first_artist) || fold(&item.title).contains(&first_artist);
        let close = match (request.seconds, clock_seconds(&item.duration)) {
            (Some(a), Some(b)) => (a - b).abs() <= 20.0,
            _ => true,
        };
        if !(same_title && same_artist && close) {
            continue;
        }
        if let Ok(stream) = youtube.resolve(&item.video_id) {
            audio::log(format!("stand-in: \"{}\" ({}) plays as {} ({})", request.title, request.video_id, item.video_id, item.title));
            return Some(stream);
        }
    }
    None
}

/// What fetching a track takes.
#[derive(Clone)]
struct LoadRequest {
    video_id: String,
    title: String,
    artist: String,
    seconds: Option<f64>,
    /// The big cover to fetch, when it isn't at hand already.
    big: Option<String>,
    addons: Vec<addon::Addon>,
    tier: addon::Tier,
}

/// A track ready to hand to the engine.
struct Loaded {
    bytes: Vec<u8>,
    length: Duration,
    volume: f32,
    cover: Option<Arc<RenderImage>>,
    cover_large: Option<Arc<RenderImage>>,
    /// What plays: "FLAC 24-bit/96 kHz" from an addon, or YouTube's "AAC 128 kbps".
    quality: SharedString,
    /// The addon it came from, when it did.
    lossless_from: Option<String>,
    /// The big cover, made ready, once fetched.
    big: Option<BigCover>,
}

enum Lyrics {
    Idle,
    Loading,
    Missing,
    Ready(Vec<lyrics::Line>),
}

/// What sits under each lyric line when asked for: the line in Latin letters.
#[derive(Default)]
struct LyricExtras {
    romaji: Option<Vec<Option<String>>>,
    romaji_pending: bool,
    /// Why it isn't there (a failure).
    note: Option<SharedString>,
}

/// The lyric view's moving parts: one opacity per line, the column's scroll,
/// and the measured layout the scroll aims with.
struct LyricMotion {
    lines: Vec<MotionState>,
    blur: Vec<Tween>,
    scale: Vec<Tween>,
    /// The single-line look's entrance, restarted for each new line.
    single: MotionState,
    single_line: Option<usize>,
    scroll: MotionState,
    /// Each line's top and height, relative to the first line.
    layout: Rc<RefCell<Vec<(f32, f32)>>>,
    viewport: Rc<Cell<f32>>,
}

impl LyricMotion {
    fn new(count: usize) -> Self {
        Self {
            lines: (0..count).map(|_| MotionState::default()).collect(),
            blur: vec![Tween::at(0.0); count],
            scale: vec![Tween::at(1.0); count],
            single: MotionState::default(),
            single_line: None,
            scroll: MotionState::default(),
            layout: Rc::new(RefCell::new(Vec::new())),
            viewport: Rc::new(Cell::new(0.0)),
        }
    }
}

/// Which list a song row belongs to: what plays when it's clicked.
#[derive(Clone, Copy, PartialEq)]
enum Source {
    Seeds,
    Results,
    Shelf(usize),
    /// "Recommended for you" on Home.
    Recommended,
    /// Library › Recently played.
    Recent,
    /// One of the listener's playlists.
    Playlist(usize),
    /// The artist page's top songs.
    Artist,
    /// The open album or playlist.
    Collection,
    /// Library › On this PC.
    Local,
}

impl Source {
    fn key(self) -> String {
        match self {
            Source::Seeds => "seeds".into(),
            Source::Results => "results".into(),
            Source::Shelf(n) => format!("shelf{n}"),
            Source::Recommended => "recommended".into(),
            Source::Recent => "recent".into(),
            Source::Playlist(n) => format!("playlist{n}"),
            Source::Artist => "artist".into(),
            Source::Collection => "collection".into(),
            Source::Local => "local".into(),
        }
    }
}

/// YouTube Music's Home, as loaded.
enum Feed {
    Loading,
    Failed(String),
    Ready(Vec<FeedShelf>),
}

struct FeedShelf {
    title: SharedString,
    songs: Vec<Track>,
    cards: Vec<youtube::Card>,
}

/// The artwork key for a card: its page, or its song.
fn card_key(card: &youtube::Card) -> String {
    format!("card:{}", card.browse_id.as_deref().or(card.video_id.as_deref()).unwrap_or(&card.title))
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Home,
    Search,
    Library,
    Settings,
    /// An artist's page (pages.rs).
    Artist,
    /// An album's or playlist's page.
    Collection,
    /// What the listener played (stats_page.rs).
    Stats,
}

/// Settings › Playback › Sleep timer: pause after some minutes, or when the song ends.
/// Not kept across restarts.
#[derive(Clone, Copy, PartialEq)]
enum Sleep {
    Off,
    Minutes(u64),
    EndOfSong,
}

#[derive(Clone, Copy, PartialEq)]
enum Panel {
    None,
    Lyrics,
    UpNext,
}

#[derive(Clone, Copy, PartialEq)]
enum Repeat {
    Off,
    All,
    One,
}

/// A list on screen and the motion its rows rise in with.
struct Listing {
    tracks: Vec<Track>,
    motion: Vec<MotionState>,
}

impl Listing {
    fn new(tracks: Vec<Track>) -> Self {
        Self { motion: tracks.iter().map(|_| MotionState::default()).collect(), tracks }
    }
}

struct NabyTune {
    root: FocusHandle,
    search: Entity<SearchBox>,
    _search: Vec<Subscription>,

    /// Windows' media controls and keys.
    media: Option<media::Media>,
    /// FocusGrow & DynamicNotification WebSocket bridge.
    bridge: Option<bridge::Bridge>,
    /// Windows Taskbar thumbnail preview media buttons (Prev / Play-Pause / Next).
    taskbar: Option<taskbar::Taskbar>,

    // ---- Pages
    tab: Tab,
    home: Listing,
    feed: Feed,
    feed_motion: Vec<MotionState>,
    /// A playlist or album being opened from a card, by title.
    opening: Option<SharedString>,
    results: Listing,
    query: SharedString,
    searching: bool,
    search_error: Option<String>,
    art: HashMap<String, Art>,
    /// The artwork's keys, the longest-kept first, and the bytes it all takes.
    art_order: std::collections::VecDeque<String>,
    art_bytes: usize,
    art_pending: HashSet<String>,
    /// When the images nothing uses any more were last let go of.
    images_swept: Instant,

    // ---- What plays
    queue: Vec<Track>,
    index: Option<usize>,
    cover: Option<Arc<RenderImage>>,
    /// The full-screen player's cover, at its own size.
    cover_large: Option<Arc<RenderImage>>,
    ambient: Option<Arc<RenderImage>>,
    previous_ambient: Option<Arc<RenderImage>>,
    youtube: Arc<youtube::YouTube>,
    audio: audio::Audio,
    /// Counts loads, so a slow answer for a track already skipped is dropped.
    generation: u64,
    loading: bool,
    error: Option<String>,
    playing: bool,
    length: Duration,
    shuffle: bool,
    repeat: Repeat,
    muted: bool,

    // ---- The full-screen player
    full_screen: bool,
    /// Showing the full playlist/queue instead of lyrics in the full-screen stage.
    stage_queue: bool,
    stage_layout: StageLayout,
    stage_backdrop: StageBackdrop,
    /// What moves with the music behind the full-screen player.
    visual: visualizer::Visual,
    stage_art: Option<StageArt>,
    layout_menu: bool,
    layout_menu_closed: Instant,
    /// The full-screen player's volume: a button, its slider showing only once clicked.
    stage_volume_open: bool,
    stage_volume_closed: Instant,
    /// Closing, since when: the player slides down and fades before it's gone. The clock
    /// starts with the first frame drawn closing (`stage_close_drawn`), so a slow frame
    /// can't eat the start of the slide.
    stage_closing: Option<Instant>,
    /// Songs in a row that couldn't be played: past a few, it stops skipping on to the next.
    failed_in_row: u32,
    stage_close_drawn: bool,
    stage_motion: MotionState,
    stage_cover_motion: MotionState,
    stage_seek: Rc<Cell<(f32, f32)>>,
    stage_gestures: [GestureState; 3],

    /// Lines away from the sung one go out of focus (on by default; costs GPU while lyrics move).
    blur_lyrics: bool,
    /// Who had the lyrics, and whether they're timed (YouTube Music's own text isn't).
    lyrics_source: SharedString,
    lyrics_synced: bool,
    /// Every version of this song's lyrics found, best first; the listener can pick another
    /// when the first is off.
    lyric_options: Vec<lyrics::Found>,
    extras: LyricExtras,
    /// Romaji already worked out, per song and lyric source.
    extras_cache: HashMap<String, Vec<Option<String>>>,
    show_romaji: bool,
    look: Look,
    look_menu: Option<LookAnchor>,
    /// When a click outside last closed the menu: the same click mustn't reopen it from "Aa".
    look_menu_closed: Instant,

    // ---- The side panel
    panel: Panel,
    panel_motion: MotionState,
    lyrics: Lyrics,
    lyric_motion: LyricMotion,
    /// The tick repaints when the sung line changes, or every 250 ms for the seek line.
    last_line: Option<usize>,
    last_paint: Instant,

    // ---- YouTube Music account
    account: Option<youtube::Account>,
    /// The sign-in window is open.
    signing_in: bool,
    account_error: Option<String>,
    /// The account's playlists, "Liked music" first.
    account_playlists: Vec<youtube::Card>,

    // ---- Pages opened from others
    artist_view: Option<pages::ArtistView>,
    collection_view: Option<pages::CollectionView>,
    /// The pages before this one, for the way back.
    back_stack: Vec<Tab>,
    /// The right-click menu of a song.
    track_menu: Option<track_menu::TrackMenu>,
    track_menu_closed: Instant,

    // ---- Integrations
    discord: Option<discord::Discord>,
    /// Show what plays on Discord.
    discord_on: bool,
    tray: Option<tray::Tray>,
    /// The mini player's window, while it's open.
    mini: Option<mini::MiniHandle>,
    /// How the mini player writes the sung line: as written, with romaji, or romaji alone.
    mini_romaji: mini::MiniRomaji,
    /// The lyrics floating on the desktop, while they're up, and whether clicks go through them.
    desk_lyrics: Option<desktop_lyrics::DesktopLyricsHandle>,
    desk_locked: bool,
    /// The window's own handle (Win32), to show it again from the tray.
    hwnd: isize,
    /// The close button hides NabyTune to the tray instead of quitting.
    close_to_tray: bool,
    /// When the session (queue and position) was last written.
    session_saved: Instant,
    /// The equalizer: on, and its five gains.
    eq_on: bool,
    eq_gains: [f32; 5],
    eq_bars: Rc<RefCell<Vec<(f32, f32)>>>,
    /// The lyric timing nudge for the playing song, in ms (positive: lyrics later).
    lyric_offset: i64,

    // ---- Library
    library: library::Library,
    /// Songs like the ones played lately, for Home.
    recommended: Vec<Track>,
    /// The playlist open in Library, if one is.
    open_playlist: Option<usize>,
    /// Renaming the open playlist: the name field.
    name_field: Entity<SearchBox>,
    renaming: bool,
    playlist_menu: Option<PlaylistMenu>,
    playlist_menu_closed: Instant,
    /// A playlist whose Delete was clicked once: the next click deletes it.
    delete_armed: Option<usize>,
    /// When the queue runs low, vibe-matched recommended songs automatically line up next.
    autoplay: bool,
    /// Video ID currently being fetched for autoplay recommendations (prevents duplicate calls).
    autoplay_fetching: Option<String>,
    /// The radio playing, if one is: it keeps the queue topped up.
    radio: Option<radio::RadioState>,
    /// The song being listened to, counted for Stats; and the Stats page.
    listening: Option<stats_page::Listening>,
    stats: stats_page::StatsView,
    /// The listener's music folders, the songs found in them, and whether they're being read.
    local_folders: Vec<PathBuf>,
    local_tracks: Vec<local_files::LocalTrack>,
    local_scanning: bool,
    local_show_all: bool,
    /// What's known about a newer NabyTune.
    update: updates::Update,
    library_motion: Vec<MotionState>,

    // ---- Lossless addons
    /// The addresses added, in order of preference.
    addon_urls: Vec<String>,
    /// Their manifests, once read (by address).
    addons: Vec<addon::Addon>,
    /// Why an address didn't answer as an addon (by address).
    addon_errors: HashMap<String, String>,
    lossless: bool,
    tier: addon::Tier,
    addon_field: Entity<SearchBox>,
    adding_addon: bool,
    /// What the playing song plays as, and from which addon.
    quality: SharedString,
    lossless_from: Option<String>,

    // ---- Settings
    /// "system" (Windows' accent) or "#rrggbb".
    accent_choice: String,
    material: Material,
    /// The current cover's glow behind the whole window.
    show_ambient: bool,
    /// YouTube's loudness normalisation: loud masters turned down to a common level.
    normalize: bool,
    /// The listener's volume, 0 to 1, on top of normalisation.
    volume: f32,
    volume_bar: Rc<Cell<(f32, f32)>>,
    stage_volume_bar: Rc<Cell<(f32, f32)>>,
    /// Where Settings' colour strip sits, for clicks along it.
    hue_bar: Rc<Cell<(f32, f32)>>,
    light_bar: Rc<Cell<(f32, f32)>>,
    /// The playing track's normalising gain, to apply or undo when the switch flips.
    track_gain: f32,
    sleep: Sleep,
    /// When the timed sleep pauses playback.
    sleep_at: Option<Instant>,
    /// The lyric source to show first when a song has it ("" for the best one).
    preferred_source: String,
    settings_motion: Vec<MotionState>,

    /// Big covers made ready, by song, newest last: the next song's is fetched while one
    /// plays, so skipping shows a sharp cover at once.
    big_covers: Vec<(String, BigCover)>,
    /// The cover shown is the big one (or the song's own file): until then the row's small
    /// artwork stands in, softened rather than shown pixelated.
    cover_sharp: bool,

    /// The next song, fetched while this one plays (by song).
    prefetched: Option<(String, Loaded)>,
    prefetching: Option<String>,
    /// Shuffle's pick for the song after this one, made when this one starts.
    shuffle_next: Option<usize>,
    /// The next start crossfades from what plays (an automatic change, near a song's end).
    fade_next: bool,
    /// Seconds songs overlap by when one runs into the next (0: off).
    crossfade: u32,
    /// The generation a crossfade was started from, so it starts once.
    crossfaded: u64,
    /// Where to pick up the song restored from the last session, once it loads.
    resume_at: Option<Duration>,

    /// Quick setup's step, while it shows (first run).
    setup_step: Option<usize>,
    setup_motion: MotionState,

    /// Brief floating notification toast (e.g. "Playing next", "Added to queue").
    toast: Option<(SharedString, Instant)>,
    /// Real-time vinyl rotation base image, transient GPU frames, and animation state.
    vinyl_base: Option<Arc<RgbaImage>>,
    vinyl_frame: Option<(f32, Arc<RenderImage>)>,
    vinyl_prev_frame: Option<Arc<RenderImage>>,
    vinyl_mode: bool,
    vinyl_rotate: bool,
    vinyl_speed: f32,
    vinyl_state: Rc<Cell<(f32, Instant)>>,
    vinyl_speed_bar: Rc<Cell<(f32, f32)>>,

    // ---- Motion
    dock_motion: MotionState,
    seek_bar: Rc<Cell<(f32, f32)>>,
    spring: Spring,
    cover_motion: MotionState,
    ambient_motion: MotionState,
    play_gesture: GestureState,
    prev_gesture: GestureState,
    next_gesture: GestureState,
    /// Each sideways row's scroll, by the row's name.
    shelves: HashMap<SharedString, shelf::Shelf>,
    /// The last press dragged a row: it doesn't open the card it began on.
    shelf_dragged: bool,
}

impl NabyTune {
    fn new(seeds: Vec<Track>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // The UI's language first, so every word drawn from here on is in it.
        i18n::set_language(&settings::get("language").unwrap_or_else(|| "en".into()));
        let search = cx.new(SearchBox::new);
        let name_field = cx.new(|cx| SearchBox::field("Playlist name", 300.0, cx));
        let addon_field = cx.new(|cx| SearchBox::field("Addon address (https://…)", 340.0, cx));
        let subscription = vec![
            cx.subscribe(&search, |this, _, Submitted(query), cx| this.run_search(query.clone(), cx)),
            // Escape in an empty box hands the keyboard back to the app (Ctrl+P and the rest).
            cx.subscribe_in(&search, window, |this, _, _: &Dismissed, window, cx| this.root.focus(window, cx)),
            // The playlist name field: Enter renames, Escape gives up.
            cx.subscribe(&name_field, |this, _, Submitted(name), cx| {
                if let Some(p) = this.open_playlist.and_then(|i| this.library.playlists.get_mut(i)) {
                    p.name = name.trim().to_string();
                    this.library.save();
                }
                this.renaming = false;
                cx.notify();
            }),
            cx.subscribe(&addon_field, |this, _, Submitted(url), cx| this.add_addon(url.clone(), cx)),
            cx.subscribe_in(&name_field, window, |this, _, _: &Dismissed, window, cx| {
                this.renaming = false;
                this.root.focus(window, cx);
                cx.notify();
            }),
        ];

        // Four repaints a second move the seek line, a line change repaints at once while
        // lyrics show; the same tick notices a track's end.
        cx.spawn(async move |this, cx| loop {
            let fast = this.update(cx, |app, _| (app.lyrics_on_screen() || app.desk_lyrics.is_some()) && app.playing).unwrap_or(false);
            cx.background_executor().timer(Duration::from_millis(if fast { 100 } else { 250 })).await;
            let alive = this.update(cx, |app, cx| {
                // Media keys and Windows' media flyout.
                for event in app.media.as_ref().map(|m| m.events()).unwrap_or_default() {
                    app.media_event(event, cx);
                }
                let status = app.audio.status();
                if status.generation == app.generation && status.ended {
                    app.track_ended(cx);
                }
                app.maybe_crossfade(cx);
                app.tick_integrations(cx);
                // The sleep timer ran out: pause, and the timer is done.
                if app.sleep_at.is_some_and(|at| Instant::now() >= at) {
                    app.sleep = Sleep::Off;
                    app.sleep_at = None;
                    if app.playing {
                        app.toggle(cx);
                    }
                    cx.notify();
                }
                if app.playing || app.loading {
                    let line = if app.lyrics_on_screen() || app.desk_lyrics.is_some() { app.sung_line() } else { None };
                    // Karaoke lights words between line changes: it needs every tick. So do
                    // the desktop lyrics, which light theirs the same way.
                    let karaoke = (app.lyrics_on_screen() && app.look == Look::Karaoke) || app.desk_lyrics.is_some();
                    if karaoke || line != app.last_line || app.last_paint.elapsed() >= Duration::from_millis(240) {
                        app.last_line = line;
                        app.last_paint = Instant::now();
                        cx.notify();
                    }
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();

        let mut art = HashMap::new();
        for track in &seeds {
            if let Some(local) = &track.local {
                art.insert(track.video_id.clone(), Art { small: ambient::cover(local, 120) });
            }
        }
        let root = cx.focus_handle();
        root.focus(window, cx);
        let mut app = Self {
            root,
            search,
            _search: subscription,
            media: None,
            bridge: None,
            taskbar: None,
            tab: Tab::Home,
            home: Listing::new(seeds),
            feed: Feed::Loading,
            feed_motion: Vec::new(),
            opening: None,
            results: Listing::new(Vec::new()),
            query: SharedString::default(),
            searching: false,
            search_error: None,
            art_bytes: art.values().map(|a: &Art| ambient::bytes_of(&a.small)).sum(),
            art_order: art.keys().cloned().collect(),
            images_swept: Instant::now(),
            art,
            art_pending: HashSet::new(),
            queue: Vec::new(),
            index: None,
            cover: None,
            cover_large: None,
            ambient: None,
            previous_ambient: None,
            youtube: Arc::new(youtube::YouTube::new()),
            audio: audio::Audio::new(),
            generation: 0,
            loading: false,
            error: None,
            playing: false,
            length: Duration::ZERO,
            shuffle: false,
            repeat: Repeat::Off,
            muted: std::env::var_os("NABYTUNE_PROTO_MUTE").is_some(),
            full_screen: false,
            stage_queue: false,
            stage_layout: settings::get("stage_layout").and_then(|k| StageLayout::from_key(&k)).unwrap_or(StageLayout::Side),
            stage_backdrop: settings::get("stage_backdrop").and_then(|k| StageBackdrop::from_key(&k)).unwrap_or(StageBackdrop::Ambient),
            visual: settings::get("visualizer").and_then(|k| visualizer::Visual::from_key(&k)).unwrap_or(visualizer::Visual::Pulse),
            toast: None,
            vinyl_base: None,
            vinyl_frame: None,
            vinyl_prev_frame: None,
            vinyl_mode: settings::get("vinyl_mode")
                .map_or_else(|| settings::get("stage_layout").as_deref() == Some("vinyl"), |v| v != "0"),
            vinyl_rotate: settings::get("vinyl_rotate").map_or(true, |v| v != "0"),
            vinyl_speed: settings::get("vinyl_speed").and_then(|v| v.parse().ok()).unwrap_or(8.0),
            vinyl_state: Rc::new(Cell::new((0.0, Instant::now()))),
            vinyl_speed_bar: Rc::new(Cell::new((0.0, 180.0))),
            stage_art: None,
            layout_menu: false,
            layout_menu_closed: Instant::now(),
            stage_volume_open: false,
            stage_volume_closed: Instant::now(),
            stage_closing: None,
            failed_in_row: 0,
            stage_close_drawn: false,
            stage_motion: MotionState::default(),
            stage_cover_motion: MotionState::default(),
            stage_seek: Rc::new(Cell::new((0.0, STAGE_COVER))),
            stage_gestures: [GestureState::default(), GestureState::default(), GestureState::default()],
            blur_lyrics: settings::get("blur_lyrics").is_none_or(|v| v != "0"),
            lyrics_source: SharedString::default(),
            lyrics_synced: true,
            lyric_options: Vec::new(),
            extras: LyricExtras::default(),
            extras_cache: HashMap::new(),
            show_romaji: settings::get("lyrics_romaji").is_some_and(|v| v == "1"),
            look: settings::get("lyric_look").and_then(|k| Look::from_key(&k)).unwrap_or(Look::Focus),
            look_menu: None,
            look_menu_closed: Instant::now(),
            panel: Panel::None,
            panel_motion: MotionState::default(),
            lyrics: Lyrics::Idle,
            lyric_motion: LyricMotion::new(0),
            last_line: None,
            last_paint: Instant::now(),
            addon_urls: (0..16).filter_map(|i| settings::get(&format!("addon.{i}"))).filter(|u| !u.is_empty()).collect(),
            addons: Vec::new(),
            addon_errors: HashMap::new(),
            lossless: settings::get("lossless").is_none_or(|v| v != "0"),
            tier: addon::Tier::from_key(&settings::get("lossless_tier").unwrap_or_default()),
            addon_field,
            adding_addon: false,
            quality: SharedString::default(),
            lossless_from: None,
            account: None,
            signing_in: false,
            account_error: None,
            account_playlists: Vec::new(),
            artist_view: None,
            collection_view: None,
            back_stack: Vec::new(),
            track_menu: None,
            track_menu_closed: Instant::now(),
            discord: None,
            discord_on: settings::get("discord").is_none_or(|v| v != "0"),
            tray: None,
            mini: None,
            mini_romaji: settings::get("mini_romaji").and_then(|k| mini::MiniRomaji::from_key(&k)).unwrap_or(mini::MiniRomaji::Original),
            desk_lyrics: None,
            desk_locked: settings::get("desk_lyrics_locked").is_some_and(|v| v == "1"),
            hwnd: 0,
            close_to_tray: settings::get("close_to_tray").is_some_and(|v| v == "1"),
            session_saved: Instant::now(),
            eq_on: settings::get("eq").is_some_and(|v| v == "1"),
            eq_gains: settings::get("eq_gains").and_then(|v| eq::parse(&v)).unwrap_or([0.0; 5]),
            eq_bars: Rc::new(RefCell::new(Vec::new())),
            lyric_offset: 0,
            library: library::Library::load(),
            recommended: Vec::new(),
            open_playlist: None,
            name_field,
            renaming: false,
            playlist_menu: None,
            playlist_menu_closed: Instant::now(),
            delete_armed: None,
            autoplay: settings::get("autoplay").is_none_or(|v| v != "0"),
            autoplay_fetching: None,
            radio: None,
            listening: None,
            stats: stats_page::StatsView::default(),
            local_folders: local::saved_folders(),
            local_tracks: Vec::new(),
            local_scanning: false,
            local_show_all: false,
            update: updates::Update::Idle,
            library_motion: Vec::new(),
            // "cover" was once a choice of its own; it's a switch beside the pick now.
            accent_choice: settings::get("accent").filter(|k| k != "cover").unwrap_or_else(|| format!("#{:06x}", appearance::DEFAULT)),
            material: settings::get("material").and_then(|k| Material::from_key(&k)).unwrap_or(Material::Mica),
            show_ambient: settings::get("ambient").is_none_or(|v| v != "0"),
            normalize: settings::get("normalize").is_none_or(|v| v != "0"),
            track_gain: 1.0,
            volume: settings::get("volume").and_then(|v| v.parse().ok()).map_or(1.0, |v: f32| v.clamp(0.0, 1.0)),
            volume_bar: Rc::new(Cell::new((0.0, VOLUME))),
            stage_volume_bar: Rc::new(Cell::new((0.0, VOLUME))),
            hue_bar: Rc::new(Cell::new((0.0, 1.0))),
            light_bar: Rc::new(Cell::new((0.0, 1.0))),
            sleep: Sleep::Off,
            sleep_at: None,
            preferred_source: settings::get("lyrics_preferred").unwrap_or_default(),
            settings_motion: Vec::new(),
            big_covers: Vec::new(),
            cover_sharp: false,
            prefetched: None,
            prefetching: None,
            shuffle_next: None,
            fade_next: false,
            crossfade: settings::get("crossfade").and_then(|v| v.parse().ok()).unwrap_or(0).min(12),
            crossfaded: 0,
            resume_at: None,
            setup_step: NabyTune::setup_pending().then_some(0),
            setup_motion: MotionState::default(),
            dock_motion: MotionState::default(),
            seek_bar: Rc::new(Cell::new((0.0, SEEK))),
            spring: Spring::visual(Duration::from_millis(420), 0.22).expect("valid spring"),
            cover_motion: MotionState::default(),
            ambient_motion: MotionState::default(),
            play_gesture: GestureState::default(),
            prev_gesture: GestureState::default(),
            next_gesture: GestureState::default(),
            shelves: HashMap::new(),
            shelf_dragged: false,
        };
        appearance::set_accent(&app.accent_choice);
        appearance::set_follow_cover(match settings::get("accent_cover") {
            Some(v) => v == "1",
            None => settings::get("accent").is_some_and(|k| k == "cover"),
        });
        app.material.apply(window);
        smooth_move::smooth_moves(window);
        // Listen again: what was played lately, once there is some.
        if !app.library.history.is_empty() {
            let recent: Vec<Track> = app.library.history.iter().take(8).map(Track::from).collect();
            for track in &recent {
                app.fetch_art(track, cx);
            }
            app.home = Listing::new(recent);
        }
        app.recommend(cx);
        app.connect_addons(cx);
        // A session kept from last time: Home, library and requests as that account.
        if let Some(cookie) = account::load() {
            app.youtube.set_account(Some(cookie));
            app.load_account(cx);
        }
        app.load_feed(cx);
        app.media = media::Media::new(window);
        app.bridge = bridge::Bridge::new();
        app.set_up_window(window, cx);
        app.taskbar = taskbar::Taskbar::new(window);
        app.restore_session(cx);
        app
    }
}

thread_local! {
    static FRAMES: Cell<(u32, Option<Instant>)> = const { Cell::new((0, None)) };
}

/// NABYTUNE_PROTO_FPS=1: logs renders per second to %TEMP%\nabytune-proto.log.
fn count_frame() {
    if std::env::var_os("NABYTUNE_PROTO_FPS").is_none() {
        return;
    }
    FRAMES.with(|f| {
        let (n, since) = f.get();
        let since = since.unwrap_or_else(Instant::now);
        if since.elapsed() >= Duration::from_secs(1) {
            audio::log(format!("{} renders/s", n + 1));
            f.set((0, Some(Instant::now())));
        } else {
            f.set((n + 1, Some(since)));
        }
    });
}

impl Render for NabyTune {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        count_frame();
        // Images nothing uses any more leave the GPU's atlas (GPUI keeps every image it drew).
        if self.images_swept.elapsed() > Duration::from_secs(5) {
            self.images_swept = Instant::now();
            for image in ambient::unused_images() {
                cx.drop_image(image, Some(window));
            }
        }
        if !self.full_screen || !self.vinyl_mode {
            if let Some((_, f)) = self.vinyl_frame.take() {
                cx.drop_image(f, Some(window));
            }
            if let Some(f) = self.vinyl_prev_frame.take() {
                cx.drop_image(f, Some(window));
            }
        }
        // ---- Ambient backdrop: the new cover's glow fades in over the last one.
        let fade = Transition::Tween(Duration::from_millis(900), Easing::EaseInOut);
        let ambient_frame = self.ambient_motion.frame(
            Motion::new(Target::default().opacity(1.0)).initial(Target::default().opacity(0.0)).transition(fade),
            window,
            cx,
        );
        let layer = |image: Arc<RenderImage>| img(image).object_fit(ObjectFit::Cover).size_full();
        let mut backdrop = div().absolute().inset_0().overflow_hidden();
        // Solid: no see-through material, so the window paints its own dark ground.
        if self.material == Material::Solid {
            backdrop = backdrop.bg(rgb(SOLID));
        }
        if self.show_ambient {
            // Over a see-through material the glow is a tint, not a wall: drawn opaque it
            // would hide Mica, Mica Alt and Acrylic alike.
            let mut glow = div().absolute().inset_0().when(self.material != Material::Solid, |d| d.opacity(0.55));
            if let Some(previous) = self.previous_ambient.clone() {
                glow = glow.child(div().absolute().inset_0().child(layer(previous)));
            }
            if let Some(current) = self.ambient.clone() {
                glow = glow.child(div().absolute().inset_0().child(ambient_frame.apply(layer(current))));
            }
            backdrop = backdrop.child(glow);
        }

        let page = match self.tab {
            Tab::Home => self.home_page(window, cx),
            Tab::Search => self.search_page(window, cx),
            Tab::Library => self.library_page(window, cx),
            Tab::Stats => self.stats_page(window, cx),
            Tab::Settings => self.settings_page(window, cx),
            Tab::Artist => self.artist_page(window, cx),
            Tab::Collection => self.collection_page(cx),
        };
        let setup = self.setup_overlay(window, cx);
        // The playlist menu, by the mouse, kept inside the window.
        let menu = self.playlist_menu_card(cx).map(|card| {
            let at = self.playlist_menu.as_ref().map(|m| m.at).unwrap_or_default();
            let viewport = window.viewport_size();
            let online = self.account_playlists.iter().filter(|c| c.browse_id.as_deref() != Some("VLLM")).count();
            let rows = self.library.playlists.len() as f32 + 1.0 + online as f32 + if online > 0 { 1.0 } else { 0.0 };
            let height = (44.0 + rows * 37.0).min(440.0);
            let x = (f32::from(at.x) - 250.0).clamp(8.0, f32::from(viewport.width) - 268.0);
            let y = if f32::from(at.y) + 8.0 + height > f32::from(viewport.height) - 8.0 { f32::from(at.y) - height - 8.0 } else { f32::from(at.y) + 8.0 };
            deferred(div().absolute().left(px(x)).top(px(y.max(8.0))).child(card)).with_priority(2)
        });
        let track_menu = self.track_menu_overlay(window, cx);
        let panel = self.side_panel(window, cx);
        let titlebar = self.titlebar(cx);
        // No dock until something plays: an empty player bar is clutter. It rises in with the
        // first song.
        let dock = self.index.is_some().then(|| {
            let rise = self.dock_motion.frame(
                Motion::new(Target::default().y(0.0).opacity(1.0)).initial(Target::default().y(28.0).opacity(0.0)).transition(Transition::Spring(self.spring)),
                window,
                cx,
            );
            let dock = self.dock(window, cx);
            rise.apply(div().child(dock))
        });
        let stage = self.stage(window, cx);
        let toast = self.toast.as_ref().and_then(|(msg, at)| {
            let elapsed = at.elapsed();
            if elapsed >= Duration::from_millis(2400) {
                return None;
            }
            window.request_animation_frame();
            let alpha = if elapsed > Duration::from_millis(2000) {
                1.0 - (elapsed.as_secs_f32() - 2.0) / 0.4
            } else {
                1.0
            };
            Some(
                deferred(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(if self.index.is_some() && !self.full_screen { 96.0 } else { 28.0 }))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .px(px(16.0))
                                .py(px(8.0))
                                .rounded_full()
                                .bg(rgba(GLASS))
                                .backdrop_blur(px(24.0))
                                .border_1()
                                .border_color(rgba(STROKE))
                                .shadow_lg()
                                .opacity(alpha.clamp(0.0, 1.0))
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().size(px(7.0)).rounded_full().bg(rgb(accent())))
                                .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).text_color(rgb(0xffffff)).child(msg.clone())),
                        ),
                )
                .with_priority(3),
            )
        });
        if self.toast.as_ref().is_some_and(|(_, at)| at.elapsed() >= Duration::from_millis(2400)) {
            self.toast = None;
        }
        div()
            .id("nabytune")
            .key_context("NabyTune")
            .track_focus(&self.root)
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| this.search.focus_handle(cx).focus(window, cx)))
            .on_action(cx.listener(|this, _: &TogglePlay, _, cx| this.toggle(cx)))
            .on_action(cx.listener(|this, _: &CloseFullScreen, _, cx| {
                this.set_full_screen(false);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SeekBack, _, cx| {
                this.seek_to(this.now().saturating_sub(Duration::from_secs(5)));
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SeekForward, _, cx| {
                this.seek_to(this.now() + Duration::from_secs(5));
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &VolumeUp, _, cx| {
                this.set_volume(this.volume + 0.05);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &VolumeDown, _, cx| {
                this.set_volume(this.volume - 0.05);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NextTrack, _, cx| this.skip(1, cx)))
            .on_action(cx.listener(|this, _: &PreviousTrack, _, cx| this.skip(-1, cx)))
            .on_action(cx.listener(|this, _: &ToggleMute, _, cx| {
                this.muted = !this.muted;
                this.audio.mute(this.muted);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &LikeSong, _, cx| this.toggle_like_current(cx)))
            .on_action(cx.listener(|this, _: &ToggleFullScreen, _, cx| {
                if this.index.is_some() {
                    let open = !this.stage_open();
                    this.set_full_screen(open);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                this.set_full_screen(false);
                this.go_to(Tab::Settings);
                this.settings_motion.clear();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &GoBack, _, cx| {
                if this.full_screen {
                    this.set_full_screen(false);
                } else {
                    this.go_back();
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &MiniPlayer, _, cx| this.open_mini_player(cx)))
            .on_action(cx.listener(|this, _: &DesktopLyrics, _, cx| this.toggle_desktop_lyrics(cx)))
            .on_action(cx.listener(|this, _: &LyricsEarlier, _, cx| {
                this.nudge_lyrics(-250);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &LyricsLater, _, cx| {
                this.nudge_lyrics(250);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleShuffle, _, cx| {
                this.shuffle = !this.shuffle;
                this.queue_changed(cx);
            }))
            .on_action(cx.listener(|this, _: &CycleRepeat, _, cx| {
                this.repeat = match this.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
                if this.repeat != Repeat::Off {
                    this.autoplay_fetching = None;
                }
                this.queue_changed(cx);
            }))
            // The mouse's back button: the page before, or out of full screen.
            .on_mouse_down(
                gpui::MouseButton::Navigate(gpui::NavigationDirection::Back),
                cx.listener(|this, _, _, cx| {
                    if this.full_screen {
                        this.set_full_screen(false);
                    } else {
                        this.go_back();
                    }
                    cx.notify();
                }),
            )
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .font_family(TEXT)
            .text_color(rgb(0xffffff))
            .child(backdrop)
            .child(titlebar)
            // The page's layer: translucent over the backdrop, its edges a hairline.
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .mx_2()
                    .relative()
                    .rounded(px(12.0))
                    .overflow_hidden()
                    .bg(rgba(LAYER))
                    .border_1()
                    .border_color(rgba(STROKE))
                    .child(page)
                    .children(panel),
            )
            .children(dock)
            .children(stage)
            .children(menu)
            .children(track_menu)
            .children(setup)
            .children(toast)
    }
}

/// Songs to start with, for development only: `NABYTUNE_PROTO_SEEDS` names a folder holding
/// `tracks.txt` (`cover|title|artist|videoId` per line) and the covers. Anything missing or
/// malformed is skipped: an installed NabyTune starts with none, and that's the normal case.
fn load_seeds() -> Vec<Track> {
    let Some(dir) = std::env::var_os("NABYTUNE_PROTO_SEEDS").map(PathBuf::from) else { return Vec::new() };
    let Ok(list) = std::fs::read_to_string(dir.join("tracks.txt")) else { return Vec::new() };
    list.lines()
        .map(|line| line.trim_start_matches('\u{feff}'))
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() != 4 {
                audio::log(format!("seeds: skipped a bad line: {line}"));
                return None;
            }
            let cover = image::open(dir.join(parts[0])).ok()?.to_rgba8();
            Some(Track {
                video_id: parts[3].trim().to_string(),
                title: parts[1].to_string().into(),
                artist: parts[2].to_string().into(),
                detail: parts[2].to_string().into(),
                duration: SharedString::default(),
                thumbnail: None,
                local: Some(Arc::new(cover)),
            })
        })
        .collect()
}

/// Where NabyTune writes what went wrong: `%LOCALAPPDATA%\NabyTune\errors.log`.
fn error_log() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("NabyTune");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("errors.log"))
}

fn log_error(what: &str) {
    use std::io::Write as _;
    if let Some(path) = error_log() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let _ = writeln!(f, "[{at}] NabyTune {}: {what}", env!("CARGO_PKG_VERSION"));
        }
    }
}

/// A message box, for the failures that stop NabyTune before it has a window: without one
/// (it has no console) the app would just never appear.
fn show_fatal(what: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let text: Vec<u16> =
        format!("NabyTune couldn't start.\n\n{what}\n\nDetails are in %LOCALAPPDATA%\\NabyTune\\errors.log.\0").encode_utf16().collect();
    let title: Vec<u16> = "NabyTune\0".encode_utf16().collect();
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_ICONERROR | MB_OK);
    }
}

fn main() {
    // A panic anywhere is written down; on the main thread (which ends the app) it's shown too,
    // rather than the window just vanishing.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let what = format!("{info}");
        log_error(&format!("panic: {what}"));
        if std::thread::current().name() == Some("main") {
            show_fatal(&what);
        }
        default_hook(info);
    }));
    if let Err(e) = run() {
        let what = format!("{e:#}");
        log_error(&what);
        show_fatal(&what);
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    // Installed: the same app id as the Start Menu shortcut, so the taskbar groups them and
    // Windows' media flyout names NabyTune. (Run from elsewhere, no shortcut names the id.)
    let installed = std::env::current_exe().ok().is_some_and(|exe| exe.to_string_lossy().to_lowercase().contains("\\programs\\nabytune\\"));
    #[cfg(windows)]
    if installed {
        unsafe {
        let id: Vec<u16> = "NabyTune.Player\0".encode_utf16().collect();
        windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(id.as_ptr());
        }
    }
    let seeds = load_seeds();

    gpui_platform::application().run(move |cx: &mut App| {
        search_box::bind_keys(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-f", FocusSearch, None),
            KeyBinding::new("ctrl-p", TogglePlay, None),
            KeyBinding::new("escape", CloseFullScreen, Some("NabyTune")),
            // Single keys only while no text field has the keyboard.
            KeyBinding::new("space", TogglePlay, Some(PLAYER_KEYS)),
            KeyBinding::new("left", SeekBack, Some(PLAYER_KEYS)),
            KeyBinding::new("right", SeekForward, Some(PLAYER_KEYS)),
            KeyBinding::new("up", VolumeUp, Some(PLAYER_KEYS)),
            KeyBinding::new("down", VolumeDown, Some(PLAYER_KEYS)),
            KeyBinding::new("m", ToggleMute, Some(PLAYER_KEYS)),
            KeyBinding::new("l", LikeSong, Some(PLAYER_KEYS)),
            KeyBinding::new("s", ToggleShuffle, Some(PLAYER_KEYS)),
            KeyBinding::new("r", CycleRepeat, Some(PLAYER_KEYS)),
            KeyBinding::new("f", ToggleFullScreen, Some(PLAYER_KEYS)),
            KeyBinding::new("[", LyricsEarlier, Some(PLAYER_KEYS)),
            KeyBinding::new("]", LyricsLater, Some(PLAYER_KEYS)),
            KeyBinding::new("ctrl-right", NextTrack, Some(PLAYER_KEYS)),
            KeyBinding::new("ctrl-left", PreviousTrack, Some(PLAYER_KEYS)),
            KeyBinding::new("ctrl-m", MiniPlayer, None),
            KeyBinding::new("ctrl-d", DesktopLyrics, None),
            KeyBinding::new("ctrl-,", OpenSettings, None),
            KeyBinding::new("alt-left", GoBack, None),
        ]);
        // The last window closed (the main one, or the mini player with the main one hidden).
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1280.0), px(820.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(960.0), px(620.0))),
                titlebar: Some(TitlebarOptions { title: Some("NabyTune".into()), appears_transparent: true, ..Default::default() }),
                window_background: WindowBackgroundAppearance::MicaBackdrop,
                ..Default::default()
            },
            |window, cx| cx.new(|cx| NabyTune::new(seeds, window, cx)),
        )
        .expect("open NabyTune window");
        cx.activate(true);
    });
    Ok(())
}
