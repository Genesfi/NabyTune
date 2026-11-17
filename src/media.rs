//! Windows' media controls (System Media Transport Controls): the keyboard's media keys,
//! the media flyout by the volume indicator, and the lock screen. They show what plays
//! and send play, pause, next, previous and seeks back; the app reads those on its tick.

use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};

pub use souvlaki::MediaControlEvent as Event;
pub use souvlaki::SeekDirection;

pub struct Media {
    controls: MediaControls,
    events: Receiver<MediaControlEvent>,
}

impl Media {
    /// Controls bound to `window`. None where the platform has none to offer.
    pub fn new(window: &gpui::Window) -> Option<Self> {
        // Through the trait: GPUI's own `window_handle()` is a different thing.
        let hwnd = match HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as *mut std::ffi::c_void),
            _ => None,
        };
        let mut controls = MediaControls::new(PlatformConfig { display_name: "NabyTune", dbus_name: "nabytune_proto", hwnd }).ok()?;
        let (sender, events) = channel();
        controls
            .attach(move |event| {
                let _ = sender.send(event);
            })
            .ok()?;
        Some(Self { controls, events })
    }

    /// What Windows asked for since the last look.
    pub fn events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    pub fn track(&mut self, title: &str, artist: &str, album: Option<&str>, cover_url: Option<&str>, length: Option<Duration>) {
        let _ = self.controls.set_metadata(MediaMetadata {
            title: Some(title),
            artist: Some(artist),
            album,
            cover_url,
            duration: length,
        });
    }

    pub fn state(&mut self, playing: bool, position: Duration) {
        let progress = Some(MediaPosition(position));
        let _ = self.controls.set_playback(if playing { MediaPlayback::Playing { progress } } else { MediaPlayback::Paused { progress } });
    }
}
