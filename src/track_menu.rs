//! A song's right-click menu: play it next or at the end of the queue, heart it or turn it
//! down, go to its artist or album, or add it to a playlist. Hearts and dislikes live here
//! too: kept in the library, and sent to YouTube Music when signed in.

use std::time::{Duration, Instant};

use gpui::{deferred, div, prelude::*, px, rgb, rgba, AnyElement, Context, FontWeight, Pixels, SharedString, Window};

use crate::appearance::accent;
use crate::i18n::{t, tf};
use crate::youtube::Rating;
use crate::{library, NabyTune, PlaylistMenu, Track, GLASS, ICONS, SECONDARY, STROKE, SUBTLE, TERTIARY};

pub(crate) struct TrackMenu {
    pub track: Track,
    pub at: gpui::Point<Pixels>,
    /// Where the song is in the queue, when the menu is opened from Up Next.
    pub queued: Option<usize>,
}

pub(crate) const HEART: &str = "\u{EB51}";
pub(crate) const HEART_FILLED: &str = "\u{EB52}";

impl NabyTune {
    pub(crate) fn open_track_menu(&mut self, track: Track, at: gpui::Point<Pixels>, queued: Option<usize>) {
        self.playlist_menu = None;
        if self.track_menu_closed.elapsed() > Duration::from_millis(250) {
            self.track_menu = Some(TrackMenu { track, at, queued });
        }
    }

    fn close_track_menu(&mut self) {
        self.track_menu = None;
        self.track_menu_closed = Instant::now();
    }

    /// Puts `track` right after the playing song (or plays it, when nothing does).
    pub(crate) fn play_next(&mut self, track: Track, cx: &mut Context<Self>) {
        match self.index {
            Some(i) => {
                let title = track.title.to_string();
                // Moved, if it's queued already (not the currently playing slot).
                if let Some(at) = self.queue.iter().position(|t| t.video_id == track.video_id).filter(|at| *at != i) {
                    self.queue.remove(at);
                    if at < i {
                        self.index = Some(i - 1);
                    }
                }
                let i = self.index.unwrap_or(0);
                self.fetch_art(&track, cx);
                self.queue.insert(i + 1, track);
                self.toast = Some((tf("Playing next: {}", &[&title]).into(), Instant::now()));
                self.queue_changed_with_next(Some(i + 1), cx);
            }
            None => self.play_from(vec![track], 0, cx),
        }
    }

    /// Puts `track` at the very bottom of the queue (or plays it, when nothing does).
    pub(crate) fn add_to_queue(&mut self, track: Track, cx: &mut Context<Self>) {
        let Some(cur) = self.index else {
            self.play_from(vec![track], 0, cx);
            return;
        };
        let title = track.title.to_string();
        // If it's already sitting elsewhere in the queue (e.g. already played earlier), move it to the bottom.
        if let Some(at) = self.queue.iter().position(|t| t.video_id == track.video_id).filter(|at| *at != cur) {
            self.queue.remove(at);
            if at < cur {
                self.index = Some(cur - 1);
            }
        }
        self.fetch_art(&track, cx);
        self.queue.push(track);
        self.toast = Some((tf("Added to queue: {}", &[&title]).into(), Instant::now()));
        self.queue_changed(cx);
    }

    /// Takes song `at` out of the queue (not the playing one).
    pub(crate) fn remove_from_queue(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(i) = self.index else { return };
        if at == i || at >= self.queue.len() {
            return;
        }
        self.queue.remove(at);
        if at < i {
            self.index = Some(i - 1);
        }
        self.queue_changed(cx);
    }

    /// What comes next may have changed: shuffle's pick and the song fetched ahead follow.
    pub(crate) fn queue_changed(&mut self, cx: &mut Context<Self>) {
        self.queue_changed_with_next(None, cx);
    }

    fn queue_changed_with_next(&mut self, pinned_next: Option<usize>, cx: &mut Context<Self>) {
        if self.shuffle {
            self.shuffle_next = pinned_next.or_else(|| self.pick_shuffle());
        }
        let next = self.next_index().and_then(|n| self.queue.get(n)).map(|t| t.video_id.clone());
        if self.prefetched.as_ref().is_some_and(|(id, _)| Some(id) != next.as_ref()) {
            self.prefetched = None;
        }
        self.prefetch_next(cx);
        self.prefetch_next_cover(cx);
        self.ensure_autoplay(cx);
        self.save_session();
        cx.notify();
    }

    pub(crate) fn is_liked(&self, video_id: &str) -> bool {
        self.library.is_liked(video_id)
    }

    /// Hearts `track` (or takes the heart back): here, and on YouTube Music when signed in.
    pub(crate) fn set_liked(&mut self, track: &Track, liked: bool, cx: &mut Context<Self>) {
        self.library.set_liked(library::Saved::from(track), liked);
        self.library.save();
        self.rate_online(track.video_id.clone(), if liked { Rating::Like } else { Rating::Indifferent }, cx);
        cx.notify();
    }

    pub(crate) fn toggle_like_current(&mut self, cx: &mut Context<Self>) {
        if let Some(track) = self.current().cloned() {
            let liked = !self.is_liked(&track.video_id);
            self.set_liked(&track, liked, cx);
        }
    }

    /// Turns `track` down: it's skipped if it plays, and kept out of autoplay and
    /// recommendations from now on.
    pub(crate) fn dislike(&mut self, track: &Track, cx: &mut Context<Self>) {
        self.library.set_disliked(&track.video_id, true);
        self.library.save();
        self.recommended.retain(|t| t.video_id != track.video_id);
        self.rate_online(track.video_id.clone(), Rating::Dislike, cx);
        if self.current().is_some_and(|c| c.video_id == track.video_id) {
            self.skip(1, cx);
        } else if let Some(at) = self.queue.iter().position(|t| t.video_id == track.video_id) {
            self.remove_from_queue(at, cx);
        }
        cx.notify();
    }

    fn rate_online(&mut self, video_id: String, rating: Rating, cx: &mut Context<Self>) {
        // A file on this PC is liked here only: YouTube Music doesn't know it.
        if self.account.is_none() || crate::local::is_local(&video_id) {
            return;
        }
        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { youtube.rate(&video_id, rating) }).await;
            if let Err(e) = result {
                let _ = this.update(cx, |app, cx| {
                    app.error = Some(tf("YouTube Music didn't take the rating: {}", &[&format!("{e:#}")]));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// The account's liked songs, as hearts here (signed in).
    pub(crate) fn load_account_likes(&mut self, cx: &mut Context<Self>) {
        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let found = cx.background_executor().spawn(async move { youtube.liked_songs() }).await;
            let _ = this.update(cx, |app, cx| {
                let Ok(found) = found else { return };
                // Oldest first, so the newest ends up on top.
                for item in found.into_iter().rev() {
                    if !app.library.is_liked(&item.video_id) && !app.library.is_disliked(&item.video_id) {
                        app.library.set_liked(library::Saved::from(&Track::from(item)), true);
                    }
                }
                app.library.save();
                cx.notify();
            });
        })
        .detach();
    }

    /// The menu, by the mouse, kept inside the window.
    pub(crate) fn track_menu_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.track_menu.as_ref()?;
        let track = menu.track.clone();
        let playing = self.current().is_some_and(|c| c.video_id == track.video_id);
        let liked = self.is_liked(&track.video_id);
        let item = |id: &'static str, glyph: &'static str, label: SharedString| {
            div()
                .id(id)
                .h(px(34.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .rounded(px(8.0))
                .cursor_pointer()
                .text_size(px(13.0))
                .hover(|s| s.bg(rgba(SUBTLE)))
                .child(div().w(px(16.0)).font_family(ICONS).text_size(px(13.0)).text_color(rgba(SECONDARY)).child(glyph))
                .child(div().flex_1().truncate().child(label))
        };
        let separator = || div().my(px(4.0)).mx(px(6.0)).h(px(1.0)).bg(rgba(STROKE));
        let mut card = div()
            .id("track-menu")
            .w(px(240.0))
            .p(px(6.0))
            .flex()
            .flex_col()
            .gap(px(1.0))
            .rounded(px(12.0))
            .bg(rgba(GLASS))
            .backdrop_blur(px(24.0))
            .border_1()
            .border_color(rgba(STROKE))
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.close_track_menu();
                cx.notify();
            }))
            .child(
                div()
                    .px(px(10.0))
                    .pt(px(6.0))
                    .pb(px(6.0))
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).truncate().child(track.title.clone()))
                    .child(div().text_size(px(11.0)).text_color(rgba(TERTIARY)).truncate().child(track.artist.clone())),
            )
            .child(separator());
        // Its height, for where it fits: items, separators, the song at the top.
        let mut rows = 3.0f32;
        let mut separators = 4.0f32;
        if !playing {
            let (a, b) = (track.clone(), track.clone());
            card = card
                .child(item("tm-next", "\u{E7A7}", t("Play next").into()).on_click(cx.listener(move |this, _, _, cx| {
                    this.close_track_menu();
                    this.play_next(a.clone(), cx);
                })))
                .child(item("tm-queue", "\u{E8FD}", t("Add to queue").into()).on_click(cx.listener(move |this, _, _, cx| {
                    this.close_track_menu();
                    this.add_to_queue(b.clone(), cx);
                })));
            rows += 2.0;
        } else {
            separators -= 1.0;
        }
        if let Some(at) = menu.queued.filter(|_| !playing) {
            card = card.child(item("tm-unqueue", "\u{E711}", t("Remove from queue").into()).on_click(cx.listener(move |this, _, _, cx| {
                this.close_track_menu();
                this.remove_from_queue(at, cx);
            })));
            rows += 1.0;
        }
        let radio_seed = track.clone();
        card = card.child(item("tm-radio", "\u{EC05}", t("Start radio").into()).on_click(cx.listener(move |this, _, _, cx| {
            this.close_track_menu();
            this.start_radio(radio_seed.clone(), cx);
        })));
        rows += 1.0;
        let (a, b) = (track.clone(), track.clone());
        card = card
            .child(separator())
            .child(
                item("tm-like", if liked { HEART_FILLED } else { HEART }, if liked { t("Remove from liked").into() } else { t("Like").into() })
                    .when(liked, |d| d.text_color(rgb(accent())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.close_track_menu();
                        this.set_liked(&a, !liked, cx);
                    })),
            )
            .child(item("tm-dislike", "\u{E8E0}", t("Not for me").into()).on_click(cx.listener(move |this, _, _, cx| {
                this.close_track_menu();
                this.dislike(&b, cx);
            })))
            .child(separator());
        rows += 2.0;
        let (a, b, c) = (track.clone(), track.clone(), track.clone());
        let at = menu.at;
        card = card
            .child(item("tm-artist", "\u{E77B}", t("Go to artist").into()).on_click(cx.listener(move |this, _, _, cx| {
                this.close_track_menu();
                this.full_screen = false;
                this.open_artist_of(&a, cx);
            })))
            .child(item("tm-album", "\u{E93C}", t("Go to album").into()).on_click(cx.listener(move |this, _, _, cx| {
                this.close_track_menu();
                this.full_screen = false;
                this.open_album_of(&b, cx);
            })))
            .child(separator())
            .child(item("tm-playlist", "\u{E710}", t("Add to playlist…").into()).on_click(cx.listener(move |this, _, _, cx| {
                this.close_track_menu();
                this.playlist_menu = Some(PlaylistMenu { track: c.clone(), at });
                cx.notify();
            })));
        rows += 3.0;
        let height = 48.0 + (rows - 3.0) * 35.0 + separators * 9.0 + 14.0;
        let viewport = window.viewport_size();
        let x = f32::from(at.x).clamp(8.0, f32::from(viewport.width) - 248.0);
        let y = if f32::from(at.y) + height > f32::from(viewport.height) - 8.0 { f32::from(at.y) - height } else { f32::from(at.y) };
        Some(deferred(div().absolute().left(px(x)).top(px(y.max(8.0))).child(card)).with_priority(3).into_any_element())
    }
}
