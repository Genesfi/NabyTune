//! What NabyTune does around playback: picking up the last session where it left off, the
//! tray icon, Discord's "Listening to", the equalizer, and each song's lyric timing nudge.

use std::time::{Duration, Instant};

use gpui::{Context, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::{discord, library, settings, tray, youtube, NabyTune, Repeat, Track};

/// NabyTune's Discord application (its name is what Discord shows after "Listening to").
const DISCORD_APP: &str = "1571674595331670026";

impl NabyTune {
    /// A new song started: everything that follows the playing song catches up.
    pub(crate) fn on_track_changed(&mut self, cx: &mut Context<Self>) {
        self.lyric_offset = self.current().and_then(|t| settings::get(&format!("lyrics_offset.{}", t.video_id))).and_then(|v| v.parse().ok()).unwrap_or(0);
        self.listen_start();
        self.prefetch_next(cx);
        self.radio_top_up(cx);
        self.ensure_autoplay(cx);
        self.save_session();
        self.update_tray();
    }

    /// Every tick (a few times a second).
    pub(crate) fn tick_integrations(&mut self, cx: &mut Context<Self>) {
        if self.bridge.is_none() {
            self.bridge = crate::bridge::Bridge::new();
        }
        for event in self.bridge.as_ref().map(|b| b.events()).unwrap_or_default() {
            match event {
                crate::bridge::BridgeEvent::PlayPause => self.toggle(cx),
                crate::bridge::BridgeEvent::Next => self.skip(1, cx),
                crate::bridge::BridgeEvent::Previous => self.skip(-1, cx),
                crate::bridge::BridgeEvent::ToggleWindow => toggle_window(self.hwnd),
                crate::bridge::BridgeEvent::Seek(to) => self.seek_to(to),
            }
            cx.notify();
        }
        for event in self.taskbar.as_ref().map(|tb| tb.events()).unwrap_or_default() {
            match event {
                crate::taskbar::TaskbarEvent::Previous => self.skip(-1, cx),
                crate::taskbar::TaskbarEvent::PlayPause => self.toggle(cx),
                crate::taskbar::TaskbarEvent::Next => self.skip(1, cx),
            }
            cx.notify();
        }
        for event in self.tray.as_ref().map(|t| t.events()).unwrap_or_default() {
            match event {
                tray::TrayEvent::Show => show_window(self.hwnd),
                tray::TrayEvent::PlayPause => self.toggle(cx),
                tray::TrayEvent::Next => self.skip(1, cx),
                tray::TrayEvent::Previous => self.skip(-1, cx),
                tray::TrayEvent::Quit => {
                    self.save_session();
                    self.listen_flush();
                    cx.quit();
                }
            }
            cx.notify();
        }
        self.listen_tick();
        self.update_presence();
        if self.playing && self.session_saved.elapsed() > Duration::from_secs(10) {
            self.save_session();
        }
    }

    // ---- Session -------------------------------------------------------------------

    /// Writes the queue, the song and where in it, for next time.
    pub(crate) fn save_session(&mut self) {
        self.session_saved = Instant::now();
        let Some(index) = self.index.filter(|i| *i < self.queue.len()) else { return };
        // A long queue (a big playlist) is kept around the song: 200 either way is plenty.
        let from = index.saturating_sub(200);
        let to = (index + 200).min(self.queue.len());
        library::Session {
            queue: self.queue[from..to].iter().map(library::Saved::from).collect(),
            index: index - from,
            position_ms: if self.loading { self.resume_at.unwrap_or_default().as_millis() as u64 } else { self.now().as_millis() as u64 },
            shuffle: self.shuffle,
            repeat: match self.repeat {
                Repeat::Off => "off",
                Repeat::All => "all",
                Repeat::One => "one",
            }
            .into(),
        }
        .save();
    }

    /// Picks up what played last time: the queue, paused on its song where it was.
    pub(crate) fn restore_session(&mut self, cx: &mut Context<Self>) {
        if settings::get("resume").is_some_and(|v| v == "0") {
            return;
        }
        let Some(session) = library::Session::load() else { return };
        self.queue = session.queue.iter().map(Track::from).collect();
        self.index = Some(session.index);
        self.shuffle = session.shuffle;
        self.repeat = match session.repeat.as_str() {
            "all" => Repeat::All,
            "one" => Repeat::One,
            _ => Repeat::Off,
        };
        self.playing = false;
        self.resume_at = Some(Duration::from_millis(session.position_ms));
        if let Some(track) = self.current().cloned() {
            self.fetch_art(&track, cx);
        }
        self.start(cx);
    }

    // ---- Tray and window -----------------------------------------------------------

    /// The tray icon, and the window's own handle (for showing it again from the tray).
    pub(crate) fn set_up_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok(handle) = HasWindowHandle::window_handle(window) {
            if let RawWindowHandle::Win32(h) = handle.as_raw() {
                self.hwnd = h.hwnd.get();
            }
        }
        self.tray = tray::Tray::new(include_bytes!("../assets/nabytune.ico"));
        self.update_tray();
        // The close button: to the tray when asked to (and there is one), else quit.
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            let hide = this.update(cx, |app, _| {
                app.save_session();
                app.close_to_tray && app.tray.is_some()
            });
            match hide {
                Ok(true) => {
                    if let Ok(hwnd) = this.read_with(cx, |app, _| app.hwnd) {
                        hide_window(hwnd);
                    }
                    false
                }
                // Closing the main window ends NabyTune, the mini player with it.
                _ => {
                    let _ = this.update(cx, |app, _| app.listen_flush());
                    cx.quit();
                    true
                }
            }
        });
        self.audio.eq().set(self.eq_on, self.eq_gains);
        // The music folders, read again (new files show).
        self.scan_local(cx);
        self.check_updates_quietly(cx);
        // The floating lyrics come back if they were up when NabyTune closed.
        if settings::get("desk_lyrics").is_some_and(|v| v == "1") {
            self.open_desktop_lyrics(cx);
        }
    }

    fn update_tray(&self) {
        let tip = match self.current() {
            Some(t) => format!("{} — {}", t.title, t.artist),
            None => "NabyTune".to_string(),
        };
        if let Some(tray) = &self.tray {
            tray.set_tooltip(&tip);
            tray.set_playing(self.playing);
        }
        if let Some(tb) = &self.taskbar {
            tb.set_tooltip(&tip);
            tb.set_playing(self.playing);
        }
    }

    // ---- Discord -------------------------------------------------------------------

    /// Shows the playing song on Discord ("Listening to"), when that's on.
    fn update_presence(&mut self) {
        if !self.discord_on {
            if let Some(d) = &self.discord {
                d.set(None);
            }
            return;
        }
        let presence = self.current().filter(|_| !self.loading && !self.length.is_zero()).map(|t| discord::Presence {
            title: t.title.to_string(),
            artist: t.artist.to_string(),
            album: t.detail.split(" • ").nth(1).map(str::to_string),
            cover_url: t.thumbnail.as_ref().map(|url| if url.contains("ytimg.com") { url.clone() } else { youtube::sized(url, 544) }),
            position: self.now(),
            length: Some(self.length),
            playing: self.playing,
            url: Some(format!("https://music.youtube.com/watch?v={}", t.video_id)),
        });
        if presence.is_none() && self.discord.is_none() {
            return;
        }
        let id = settings::get("discord_app_id").unwrap_or_else(|| DISCORD_APP.to_string());
        self.discord.get_or_insert_with(|| discord::Discord::start(&id)).set(presence);
    }

    // ---- Equalizer -----------------------------------------------------------------

    pub(crate) fn set_eq(&mut self, on: bool, gains: [f32; 5]) {
        self.eq_on = on;
        self.eq_gains = gains;
        self.audio.eq().set(on, gains);
        settings::set("eq", if on { "1" } else { "0" });
        settings::set("eq_gains", &crate::eq::format(&gains));
    }

    // ---- Lyric timing --------------------------------------------------------------

    /// Where the lyrics are: the song's position, less the listener's nudge for this song.
    pub(crate) fn lyric_now(&self) -> Duration {
        let now = self.now().as_millis() as i64 - self.lyric_offset;
        Duration::from_millis(now.max(0) as u64)
    }

    /// Nudges this song's lyrics later (positive) or earlier, by `ms`; kept for the song.
    pub(crate) fn nudge_lyrics(&mut self, ms: i64) {
        self.lyric_offset = (self.lyric_offset + ms).clamp(-10_000, 10_000);
        if let Some(t) = self.current() {
            let key = format!("lyrics_offset.{}", t.video_id);
            if self.lyric_offset == 0 {
                settings::remove_prefixed(&key);
            } else {
                settings::set(&key, &self.lyric_offset.to_string());
            }
        }
    }
}

pub(crate) fn show_window(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        let hwnd_ptr = hwnd as windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
            SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
        };
        use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};

        ShowWindow(hwnd_ptr, SW_RESTORE);
        ShowWindow(hwnd_ptr, SW_SHOW);
        BringWindowToTop(hwnd_ptr);

        let fg = GetForegroundWindow();
        let cur_thread = GetCurrentThreadId();
        let fg_thread = GetWindowThreadProcessId(fg, std::ptr::null_mut());

        if fg_thread != 0 && fg_thread != cur_thread {
            AttachThreadInput(cur_thread, fg_thread, 1);
            BringWindowToTop(hwnd_ptr);
            SetForegroundWindow(hwnd_ptr);
            AttachThreadInput(cur_thread, fg_thread, 0);
        } else {
            BringWindowToTop(hwnd_ptr);
            SetForegroundWindow(hwnd_ptr);
        }
    }
}

pub(crate) fn toggle_window(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        let hwnd_ptr = hwnd as windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, IsIconic, IsWindowVisible, ShowWindow, SW_MINIMIZE,
        };
        let fg = GetForegroundWindow();
        let is_visible = IsWindowVisible(hwnd_ptr) != 0;
        let is_minimized = IsIconic(hwnd_ptr) != 0;

        if fg == hwnd_ptr && is_visible && !is_minimized {
            ShowWindow(hwnd_ptr, SW_MINIMIZE);
        } else {
            show_window(hwnd);
        }
    }
}

pub(crate) fn minimize_window(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_MINIMIZE};
    if hwnd != 0 {
        unsafe {
            ShowWindow(hwnd as _, SW_MINIMIZE);
        }
    }
}

pub(crate) fn hide_window(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    if hwnd != 0 {
        unsafe {
            ShowWindow(hwnd as _, SW_HIDE);
        }
    }
}
