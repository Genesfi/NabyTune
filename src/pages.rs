//! Pages opened from inside others: an artist's (top songs, albums, singles, artists like
//! them) and an album's or playlist's (its header and songs). The way back is the arrow at
//! their top, the mouse's back button, or any tab.

use gpui::{div, prelude::*, px, rgb, rgba, AnyElement, Context, FontWeight, SharedString};

use crate::appearance::{accent, on_accent};
use crate::i18n::{songs, t, tf};
use crate::youtube::{self, ArtistPage, Card, Header};
use crate::{artwork, card_key, NabyTune, Source, Tab, Track, CARD_FETCH, CARD_KEEP, DISPLAY, ICONS, SECONDARY, STROKE, SUBTLE_STRONG, TERTIARY};

/// An artist's page, as it loads.
pub(crate) struct ArtistView {
    pub name: SharedString,
    pub page: Option<ArtistPage>,
    pub songs: Vec<Track>,
    pub error: Option<String>,
}

/// An album or playlist, as it loads.
pub(crate) struct CollectionView {
    pub card: Card,
    pub header: Option<Header>,
    pub tracks: Vec<Track>,
    pub error: Option<String>,
}

impl NabyTune {
    /// Goes to page `tab`, remembering where from (for the way back).
    pub(crate) fn go_to(&mut self, tab: Tab) {
        if self.tab != tab || matches!(tab, Tab::Artist | Tab::Collection) {
            self.back_stack.push(self.tab);
            self.back_stack.truncate(20);
        }
        self.tab = tab;
    }

    /// Back to the page before (Home when there's none).
    pub(crate) fn go_back(&mut self) {
        let previous = self.back_stack.pop().unwrap_or(Tab::Home);
        self.tab = previous;
    }

    /// Opens the artist page `browse_id` ("UC…").
    pub(crate) fn open_artist(&mut self, browse_id: String, name: SharedString, cx: &mut Context<Self>) {
        self.artist_view = Some(ArtistView { name, page: None, songs: Vec::new(), error: None });
        self.go_to(Tab::Artist);
        cx.notify();
        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { youtube.artist(&browse_id) }).await;
            let _ = this.update(cx, |app, cx| {
                let Some(view) = app.artist_view.as_mut() else { return };
                match result {
                    Ok(page) => {
                        view.songs = page.songs.iter().cloned().map(Track::from).collect();
                        view.name = page.name.clone().into();
                        let photo = page.thumbnail.clone();
                        let cards: Vec<Card> = page.shelves.iter().flat_map(|(_, c)| c.iter().cloned()).collect();
                        let songs = view.songs.clone();
                        view.page = Some(page);
                        if let Some(url) = photo {
                            app.fetch_art_at("artist-photo".into(), youtube::sized(&url, 400), 400, 400, cx);
                        }
                        for track in &songs {
                            app.fetch_art(track, cx);
                        }
                        for card in cards {
                            if let Some(url) = card.thumbnail.clone() {
                                app.fetch_art_at(card_key(&card), url, CARD_FETCH, CARD_KEEP, cx);
                            }
                        }
                    }
                    Err(e) => view.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens the page of `track`'s artist: the one its row links, else the best match by name.
    pub(crate) fn open_artist_of(&mut self, track: &Track, cx: &mut Context<Self>) {
        let name = track.artist.clone();
        // "A, B & C": the first named.
        let first: String = name.split([',', '&']).next().unwrap_or("").trim().to_string();
        self.artist_view = Some(ArtistView { name: first.clone().into(), page: None, songs: Vec::new(), error: None });
        self.go_to(Tab::Artist);
        cx.notify();
        let (youtube, video_id) = (self.youtube.clone(), track.video_id.clone());
        cx.spawn(async move |this, cx| {
            let id = cx
                .background_executor()
                .spawn(async move {
                    let linked = youtube.song_links(&video_id).ok().and_then(|(artist, _)| artist);
                    match linked {
                        Some(id) => Ok(id),
                        None => youtube.find_artist(&first)?.ok_or_else(|| anyhow::anyhow!(tf("YouTube Music has no artist page for {}", &[&first]))),
                    }
                })
                .await;
            let _ = this.update(cx, |app, cx| match id {
                Ok(id) => {
                    // Still wanted: the listener hasn't gone elsewhere meanwhile.
                    if app.tab == Tab::Artist {
                        app.back_stack.pop();
                        let name = app.artist_view.as_ref().map(|v| v.name.clone()).unwrap_or_default();
                        app.open_artist(id, name, cx);
                    }
                }
                Err(e) => {
                    if let Some(view) = app.artist_view.as_mut() {
                        view.error = Some(format!("{e:#}"));
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Opens the album `track` is from: the one its row links, else the best match by name.
    pub(crate) fn open_album_of(&mut self, track: &Track, cx: &mut Context<Self>) {
        // "Artist • Album" (search rows): the album named, when there is one.
        let album = track.detail.split(" • ").nth(1).map(str::to_string);
        let placeholder = Card { title: album.clone().unwrap_or_else(|| track.title.to_string()), subtitle: track.artist.to_string(), thumbnail: track.thumbnail.clone(), browse_id: None, video_id: None };
        self.collection_view = Some(CollectionView { card: placeholder, header: None, tracks: Vec::new(), error: None });
        self.go_to(Tab::Collection);
        cx.notify();
        let (youtube, video_id, artist) = (self.youtube.clone(), track.video_id.clone(), track.artist.to_string());
        cx.spawn(async move |this, cx| {
            let id = cx
                .background_executor()
                .spawn(async move {
                    if let Ok((_, Some(id))) = youtube.song_links(&video_id) {
                        return Ok(id);
                    }
                    let name = album.ok_or_else(|| anyhow::anyhow!(t("This song's album isn't known")))?;
                    youtube.find_album(&name, &artist)?.ok_or_else(|| anyhow::anyhow!(tf("YouTube Music has no page for “{}”", &[&name])))
                })
                .await;
            let _ = this.update(cx, |app, cx| match id {
                Ok(id) => {
                    if app.tab == Tab::Collection {
                        app.back_stack.pop();
                        let mut card = app.collection_view.as_ref().map(|v| v.card.clone()).unwrap_or_else(|| Card { title: String::new(), subtitle: String::new(), thumbnail: None, browse_id: None, video_id: None });
                        card.browse_id = Some(id);
                        app.open_collection(card, cx);
                    }
                }
                Err(e) => {
                    if let Some(view) = app.collection_view.as_mut() {
                        view.error = Some(format!("{e:#}"));
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The songs given a heart, as a page to play from.
    pub(crate) fn open_liked(&mut self, cx: &mut Context<Self>) {
        let tracks: Vec<Track> = self.library.liked.iter().map(Track::from).collect();
        for track in tracks.iter().take(60) {
            self.fetch_art(track, cx);
        }
        let card = Card { title: "Liked songs".into(), subtitle: String::new(), thumbnail: None, browse_id: Some("local:liked".into()), video_id: None };
        let header = Header { title: "Liked songs".into(), subtitle: "PLAYLIST".into(), byline: "Your hearts, newest first".into(), thumbnail: None };
        self.collection_view = Some(CollectionView { card, header: Some(header), tracks, error: None });
        self.go_to(Tab::Collection);
        cx.notify();
    }

    /// Opens an album's or playlist's page.
    pub(crate) fn open_collection(&mut self, card: Card, cx: &mut Context<Self>) {
        let Some(browse_id) = card.browse_id.clone() else { return };
        self.collection_view = Some(CollectionView { card: card.clone(), header: None, tracks: Vec::new(), error: None });
        self.go_to(Tab::Collection);
        cx.notify();
        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { youtube.collection(&browse_id, &card) }).await;
            let _ = this.update(cx, |app, cx| {
                let Some(view) = app.collection_view.as_mut() else { return };
                match result {
                    Ok((header, items)) => {
                        view.tracks = items.into_iter().map(Track::from).collect();
                        let cover = header.thumbnail.clone();
                        let key = card_key(&view.card);
                        view.header = Some(header);
                        let tracks = view.tracks.clone();
                        if let Some(url) = cover {
                            app.fetch_art_at(key, youtube::sized(&url, 400), 400, CARD_KEEP, cx);
                        }
                        for track in tracks.iter().take(60) {
                            app.fetch_art(track, cx);
                        }
                    }
                    Err(e) => view.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The arrow back, at the top of the pages opened from others.
    fn back_button(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        div()
            .id("page-back")
            .size(px(32.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .font_family(ICONS)
            .text_size(px(12.0))
            .text_color(rgba(SECONDARY))
            .bg(rgba(SUBTLE_STRONG))
            .hover(|s| s.bg(rgba(0xffffff26)).text_color(rgb(0xffffff)))
            .child("\u{E72B}")
            .on_click(cx.listener(|this, _, _, cx| {
                this.go_back();
                cx.notify();
            }))
    }

    /// Play and Shuffle, for a page's songs.
    fn play_buttons(&self, id: &'static str, source: Source, cx: &mut Context<Self>) -> gpui::Div {
        let button = |glyph: &'static str, label: &'static str, primary: bool| {
            div()
                .id((id, if primary { 0usize } else { 1 }))
                .h(px(38.0))
                .px(px(18.0))
                .flex()
                .items_center()
                .gap_2()
                .rounded_full()
                .cursor_pointer()
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .map(|d| {
                    if primary {
                        d.bg(rgb(accent())).text_color(rgb(on_accent())).hover(|s| s.opacity(0.9))
                    } else {
                        d.bg(rgba(SUBTLE_STRONG)).border_1().border_color(rgba(STROKE)).hover(|s| s.bg(rgba(0xffffff26)))
                    }
                })
                .child(div().font_family(ICONS).text_size(px(12.0)).child(glyph))
                .child(label)
        };
        div()
            .flex()
            .gap_2()
            .child(button("\u{E768}", t("Play"), true).on_click(cx.listener(move |this, _, _, cx| {
                let tracks = this.tracks_of(source);
                if !tracks.is_empty() {
                    this.shuffle = false;
                    this.play_from(tracks, 0, cx);
                }
            })))
            .child(button("\u{E8B1}", t("Shuffle"), false).on_click(cx.listener(move |this, _, _, cx| {
                let tracks = this.tracks_of(source);
                if !tracks.is_empty() {
                    this.shuffle = true;
                    let start = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize)) % tracks.len();
                    this.play_from(tracks, start, cx);
                }
            })))
    }

    /// A page's frame: the back arrow over its content, scrolling.
    fn sub_page(&self, id: &'static str, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .size_full()
            .overflow_y_scroll()
            .px(px(32.0))
            .pt(px(20.0))
            .pb(px(40.0))
            .flex()
            .flex_col()
            .gap_6()
            .child(div().flex().child(self.back_button(cx)))
    }

    /// Copies text to the Windows clipboard and shows a brief confirmation toast.
    pub(crate) fn copy_text(&mut self, text: String, label: SharedString, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.toast = Some((label, std::time::Instant::now()));
        cx.notify();
    }

    /// Toggles YouTube Music subscription (Follow/Subscribe) for the currently viewed artist.
    pub(crate) fn toggle_artist_subscribe(&mut self, channel_id: String, name: String, cx: &mut Context<Self>) {
        if self.account.is_none() {
            self.toast = Some((t("Sign in to YouTube Music to subscribe").into(), std::time::Instant::now()));
            cx.notify();
            return;
        }
        let Some(view) = self.artist_view.as_mut() else { return };
        let Some(page) = view.page.as_mut() else { return };
        let next = !page.subscribed;
        page.subscribed = next;
        let msg = if next {
            tf("Subscribed to {}", &[&name])
        } else {
            tf("Unsubscribed from {}", &[&name])
        };
        self.toast = Some((msg.into(), std::time::Instant::now()));
        cx.notify();

        let youtube = self.youtube.clone();
        let ch = channel_id.clone();
        cx.spawn(async move |this, cx| {
            let res = cx.background_executor().spawn(async move { youtube.subscribe(&ch, next) }).await;
            if let Err(e) = res {
                let _ = this.update(cx, |app, cx| {
                    if let Some(view) = app.artist_view.as_mut() {
                        if let Some(page) = view.page.as_mut() {
                            if page.channel_id.as_deref() == Some(channel_id.as_str()) {
                                page.subscribed = !next;
                            }
                        }
                    }
                    app.toast = Some((tf("Couldn't update subscription: {}", &[&format!("{e:#}")]).into(), std::time::Instant::now()));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn artist_page(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> AnyElement {
        let mut page = self.sub_page("artist-page", cx);
        let Some(view) = self.artist_view.as_ref() else { return page.into_any_element() };
        let photo = self.art.get("artist-photo").map(|a| a.small.clone()).filter(|_| view.page.is_some());
        let subtitle: SharedString = match (&view.error, &view.page) {
            (Some(e), _) => e.clone().into(),
            (None, None) => t("Loading…").into(),
            (None, Some(p)) => p.subtitle.clone().into(),
        };
        let artist_name_copy = view.name.to_string();
        let radio = view.page.as_ref().and_then(|p| p.radio.clone());
        let channel_id = view.page.as_ref().and_then(|p| p.channel_id.clone());
        let subscribed = view.page.as_ref().is_some_and(|p| p.subscribed);
        let name_for_btns = view.name.clone();
        let has_songs = !view.songs.is_empty();

        page = page.child(
            div()
                .flex()
                .items_center()
                .gap(px(24.0))
                .child(artwork(photo, 160.0, 80.0).shadow_lg())
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().text_size(px(12.0)).font_weight(FontWeight::SEMIBOLD).text_color(rgba(TERTIARY)).child(t("ARTIST")))
                        .child(
                            div()
                                .id("artist-name-copy")
                                .flex()
                                .items_center()
                                .gap_2()
                                .cursor_pointer()
                                .group("artist-title")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.copy_text(artist_name_copy.clone(), tf("Copied: {}", &[&artist_name_copy]).into(), cx);
                                }))
                                .child(div().font_family(DISPLAY).text_size(px(40.0)).font_weight(FontWeight::BOLD).child(view.name.clone()))
                                .child(
                                    div()
                                        .font_family(ICONS)
                                        .text_size(px(14.0))
                                        .text_color(rgba(SECONDARY))
                                        .opacity(0.0)
                                        .group_hover("artist-title", |s| s.opacity(0.9))
                                        .child("\u{E8C8}"),
                                ),
                        )
                        .child(div().text_size(px(13.0)).text_color(rgba(SECONDARY)).child(subtitle))
                        .when(has_songs || channel_id.is_some(), |d| {
                            let name_r = name_for_btns.clone();
                            let name_s = name_for_btns.to_string();
                            let ch_sub = channel_id.clone();
                            let ch_link = channel_id.clone();
                            d.child(
                                div()
                                    .pt_2()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap_2()
                                    .when(has_songs, |d| d.child(self.play_buttons("artist-play", Source::Artist, cx)))
                                    .when_some(radio, |d, playlist| {
                                        d.child(
                                            div()
                                                .id("artist-radio")
                                                .h(px(38.0))
                                                .px(px(18.0))
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .rounded_full()
                                                .cursor_pointer()
                                                .text_size(px(14.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .bg(rgba(SUBTLE_STRONG))
                                                .border_1()
                                                .border_color(rgba(STROKE))
                                                .hover(|s| s.bg(rgba(0xffffff26)))
                                                .child(div().font_family(ICONS).text_size(px(12.0)).child("\u{EC05}"))
                                                .child(t("Radio"))
                                                .on_click(cx.listener(move |this, _, _, cx| this.start_artist_radio(playlist.clone(), name_r.clone(), cx))),
                                        )
                                    })
                                    .when_some(ch_sub, |d, ch| {
                                        d.child(
                                            div()
                                                .id("artist-subscribe")
                                                .h(px(38.0))
                                                .px(px(18.0))
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .rounded_full()
                                                .cursor_pointer()
                                                .text_size(px(14.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .bg(if subscribed { rgba(0xffffff1a) } else { rgba(SUBTLE_STRONG) })
                                                .border_1()
                                                .border_color(if subscribed { rgb(accent()) } else { rgba(STROKE) })
                                                .text_color(if subscribed { rgb(0xffffff) } else { rgba(0xffffffdd) })
                                                .hover(|s| s.bg(rgba(0xffffff26)))
                                                .child(
                                                    div()
                                                        .font_family(ICONS)
                                                        .text_size(px(12.0))
                                                        .when(subscribed, |d| d.text_color(rgb(accent())))
                                                        .child(if subscribed { "\u{E73E}" } else { "\u{E710}" }),
                                                )
                                                .child(if subscribed { t("Subscribed") } else { t("Subscribe") })
                                                .on_click(cx.listener(move |this, _, _, cx| this.toggle_artist_subscribe(ch.clone(), name_s.clone(), cx))),
                                        )
                                    })
                                    .when_some(ch_link, |d, ch| {
                                        d.child(
                                            div()
                                                .id("artist-copy-link")
                                                .h(px(38.0))
                                                .px(px(16.0))
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .rounded_full()
                                                .cursor_pointer()
                                                .text_size(px(14.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .bg(rgba(SUBTLE_STRONG))
                                                .border_1()
                                                .border_color(rgba(STROKE))
                                                .hover(|s| s.bg(rgba(0xffffff26)))
                                                .child(div().font_family(ICONS).text_size(px(12.0)).child("\u{E71B}"))
                                                .child(t("Share"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    let url = format!("https://music.youtube.com/channel/{ch}");
                                                    this.copy_text(url, t("Copied artist link").into(), cx);
                                                })),
                                        )
                                    }),
                            )
                        }),
                ),
        );
        let Some(artist) = view.page.clone() else { return page.into_any_element() };
        let songs = view.songs.clone();
        if !songs.is_empty() {
            let mut list = div().flex().flex_col().mx(px(-12.0));
            for (i, track) in songs.iter().enumerate() {
                list = list.child(self.row(Source::Artist, i, track, true, cx));
            }
            page = page.child(div().flex().flex_col().gap_3().child(Self::heading(t("Top songs"), 20.0)).child(list));
        }
        for (n, (title, cards)) in artist.shelves.iter().enumerate() {
            // Named by its first card too, so another artist's row starts at its start.
            let key: SharedString = format!("artist-{n}-{}", cards.first().map(card_key).unwrap_or_default()).into();
            let mut row = Vec::new();
            for (i, card) in cards.iter().cloned().enumerate() {
                let id = gpui::ElementId::NamedInteger(format!("artist-card-{n}").into(), i as u64);
                let art = self.art.get(&card_key(&card)).map(|a| a.small.clone());
                let (title, subtitle): (SharedString, SharedString) = (card.title.clone().into(), card.subtitle.clone().into());
                // Artists in a circle, as YouTube Music shows them.
                let element = if youtube::is_artist(&card) { person_card(id, art, title, subtitle) } else { Self::card(id, art, title, subtitle, false) };
                let element = element.on_click(cx.listener(move |this, _, _, cx| {
                    if !this.shelf_dragged {
                        this.open_card(card.clone(), cx);
                    }
                }));
                row.push(element.into_any_element());
            }
            page = page.child(self.shelf(key, Self::heading(title.clone(), 20.0), row, window, cx));
        }
        page.into_any_element()
    }

    pub(crate) fn collection_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut page = self.sub_page("collection-page", cx);
        let Some(view) = self.collection_view.as_ref() else { return page.into_any_element() };
        let cover = self.art.get(&card_key(&view.card)).map(|a| a.small.clone());
        let (title, kind, byline): (SharedString, SharedString, SharedString) = match &view.header {
            Some(h) => (h.title.clone().into(), h.subtitle.clone().into(), h.byline.clone().into()),
            None => (view.card.title.clone().into(), view.card.subtitle.clone().into(), SharedString::default()),
        };
        // Liked songs is NabyTune's own page, so its header is in the UI's language.
        let liked = view.card.browse_id.as_deref() == Some("local:liked");
        let (title, kind, byline) = if liked { (t("Liked songs").into(), t("PLAYLIST").into(), t("Your hearts, newest first").into()) } else { (title, kind, byline) };
        let status: Option<SharedString> = match (&view.error, view.header.is_some()) {
            (Some(e), _) => Some(e.clone().into()),
            (None, false) => Some(t("Loading…").into()),
            _ => None,
        };
        let count = view.tracks.len();
        let length: f64 = view.tracks.iter().filter_map(|t| crate::clock_seconds(&t.duration)).sum();
        let summary: SharedString = match (count, length) {
            (0, _) => SharedString::default(),
            (1, _) => songs(1).into(),
            (n, l) if l >= 60.0 => tf("{} songs • {} min", &[&n, &((l / 60.0).round() as u64)]).into(),
            (n, _) => songs(n).into(),
        };
        page = page.child(
            div()
                .flex()
                .items_end()
                .gap(px(24.0))
                .child(if view.card.browse_id.as_deref() == Some("local:liked") {
                    // Liked songs: the heart, on the accent.
                    div()
                        .size(px(200.0))
                        .flex_none()
                        .rounded(px(12.0))
                        .shadow_lg()
                        .bg(gpui::linear_gradient(135.0, gpui::linear_color_stop(rgb(accent()), 0.0), gpui::linear_color_stop(rgba(crate::appearance::accent_wash(0x40)), 1.0)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .font_family(ICONS)
                        .text_size(px(56.0))
                        .text_color(rgb(on_accent()))
                        .child(crate::track_menu::HEART_FILLED)
                } else {
                    artwork(cover, 200.0, 12.0).shadow_lg()
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().text_size(px(12.0)).font_weight(FontWeight::SEMIBOLD).text_color(rgba(TERTIARY)).child(kind))
                        .child(div().font_family(DISPLAY).text_size(px(34.0)).line_height(px(40.0)).font_weight(FontWeight::BOLD).child(title))
                        .when(!byline.is_empty(), |d| d.child(div().text_size(px(14.0)).font_weight(FontWeight::SEMIBOLD).child(byline)))
                        .child(div().text_size(px(13.0)).text_color(rgba(SECONDARY)).child(status.unwrap_or(summary)))
                        .when(count > 0, |d| d.child(div().pt_2().child(self.play_buttons("collection-play", Source::Collection, cx)))),
                ),
        );
        let tracks = view.tracks.clone();
        if !tracks.is_empty() {
            let mut list = div().flex().flex_col().mx(px(-12.0));
            for (i, track) in tracks.iter().enumerate() {
                list = list.child(self.row(Source::Collection, i, track, true, cx));
            }
            page = page.child(list);
        }
        page.into_any_element()
    }
}

/// An artist's card: their photo in a circle, the name under it.
fn person_card(id: gpui::ElementId, art: Option<std::sync::Arc<gpui::RenderImage>>, title: SharedString, subtitle: SharedString) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w(px(168.0))
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .child(artwork(art, 152.0, 76.0).shadow_lg())
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .child(div().text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).truncate().child(title))
                .child(div().text_size(px(12.0)).text_color(rgba(SECONDARY)).truncate().child(subtitle)),
        )
}
