//! Finding music: search, Home's shelves, artwork and its cache, the account and addons,
//! playlists, and the rows and cards pages are built from.

use super::*;

impl NabyTune {
    // ---- Search ---------------------------------------------------------------

    pub(crate) fn run_search(&mut self, query: String, cx: &mut Context<Self>) {
        self.tab = Tab::Search;
        self.searching = true;
        self.search_error = None;
        self.query = query.clone().into();
        cx.notify();
        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { youtube.search(&query) }).await;
            let _ = this.update(cx, |app, cx| {
                app.searching = false;
                match result {
                    Ok(items) => {
                        // Fresh motion: the new rows rise in, one after another. The
                        // listener's own files that match come first.
                        let mut found = app.local_matches(&app.query, 8);
                        found.extend(items.into_iter().map(Track::from));
                        app.results = Listing::new(found);
                        for track in app.results.tracks.clone() {
                            app.fetch_art(&track, cx);
                        }
                    }
                    Err(e) => app.search_error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The row-sized artwork for `track`, fetched once in the background.
    pub(crate) fn fetch_art(&mut self, track: &Track, cx: &mut Context<Self>) {
        if local::is_local(&track.video_id) {
            self.fetch_local_art(track, cx);
        } else if let Some(url) = track.thumbnail.clone() {
            self.fetch_art_at(track.video_id.clone(), url, 120, 120, cx);
        }
    }

    /// Keeps artwork under `key`, letting the longest-kept go past `ART_BUDGET` (never Home's
    /// or the playing song's).
    pub(crate) fn keep_art(&mut self, key: String, art: Art) {
        self.art_bytes += ambient::bytes_of(&art.small);
        match self.art.insert(key.clone(), art) {
            Some(old) => self.art_bytes = self.art_bytes.saturating_sub(ambient::bytes_of(&old.small)),
            None => self.art_order.push_back(key),
        }
        if self.art_bytes <= ART_BUDGET {
            return;
        }
        // Never Home's (it's the page everything starts from, all its shelves drawn at once,
        // and its covers aren't fetched again) or the playing song's.
        let mut keep: HashSet<String> = HashSet::new();
        for track in &self.home.tracks {
            keep.insert(track.video_id.clone());
        }
        for track in &self.queue {
            keep.insert(track.video_id.clone());
        }
        if let Feed::Ready(shelves) = &self.feed {
            for shelf in shelves {
                keep.extend(shelf.cards.iter().map(card_key));
                keep.extend(shelf.songs.iter().map(|t| t.video_id.clone()));
            }
        }
        keep.extend(self.current().map(|t| t.video_id.clone()));
        let mut kept = 0;
        while self.art_bytes > ART_BUDGET && kept < self.art_order.len() {
            let Some(oldest) = self.art_order.pop_front() else { break };
            if keep.contains(&oldest) {
                self.art_order.push_back(oldest);
                kept += 1;
                continue;
            }
            if let Some(gone) = self.art.remove(&oldest) {
                self.art_bytes = self.art_bytes.saturating_sub(ambient::bytes_of(&gone.small));
            }
        }
    }

    /// Artwork under `key`: fetched at `fetch` px, kept at `keep` px for display.
    pub(crate) fn fetch_art_at(&mut self, key: String, url: String, fetch: u32, keep: u32, cx: &mut Context<Self>) {
        if self.art.contains_key(&key) || !self.art_pending.insert(key.clone()) {
            return;
        }
        let youtube = self.youtube.clone();
        let id = key;
        cx.spawn(async move |this, cx| {
            let art = cx
                .background_executor()
                .spawn(async move {
                    let source = youtube.image(&youtube::sized(&url, fetch))?;
                    anyhow::Ok(Art { small: ambient::cover(&source, keep) })
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.art_pending.remove(&id);
                if let Ok(art) = art {
                    app.keep_art(id, art);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    // ---- Home ------------------------------------------------------------------

    /// YouTube Music's Home shelves, then their artwork in the background.
    pub(crate) fn load_feed(&mut self, cx: &mut Context<Self>) {
        self.feed = Feed::Loading;
        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { youtube.home() }).await;
            let _ = this.update(cx, |app, cx| {
                match result {
                    Ok(shelves) => {
                        let shelves: Vec<FeedShelf> = shelves
                            .into_iter()
                            .map(|shelf| {
                                let mut songs = Vec::new();
                                let mut cards = Vec::new();
                                for item in shelf.items {
                                    match item {
                                        youtube::ShelfItem::Song(s) => songs.push(Track::from(s)),
                                        youtube::ShelfItem::Card(c) => cards.push(c),
                                    }
                                }
                                FeedShelf { title: shelf.title.into(), songs, cards }
                            })
                            .collect();
                        for shelf in &shelves {
                            for track in shelf.songs.iter().take(12) {
                                app.fetch_art(&track.clone(), cx);
                            }
                            for card in &shelf.cards {
                                if let Some(url) = card.thumbnail.clone() {
                                    app.fetch_art_at(card_key(card), url, CARD_FETCH, CARD_KEEP, cx);
                                }
                            }
                        }
                        app.feed_motion = shelves.iter().map(|_| MotionState::default()).collect();
                        app.feed = Feed::Ready(shelves);
                    }
                    Err(e) => app.feed = Feed::Failed(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A card plays: its one song, or its playlist or album from the top.
    pub(crate) fn open_card(&mut self, card: youtube::Card, cx: &mut Context<Self>) {
        if let (None, Some(video_id)) = (&card.browse_id, &card.video_id) {
            let track = Track::from(youtube::SearchItem {
                video_id: video_id.clone(),
                title: card.title.clone(),
                artist: youtube::clean_artist(&card.subtitle),
                album: String::new(),
                duration: String::new(),
                thumbnail: card.thumbnail.clone(),
            });
            self.play_from(vec![track], 0, cx);
            return;
        }
        if youtube::is_artist(&card) {
            let id = card.browse_id.clone().unwrap_or_default();
            self.open_artist(id, card.title.clone().into(), cx);
            return;
        }
        // Albums and playlists open as a page, to play from or look through.
        if card.browse_id.is_some() {
            self.open_collection(card, cx);
            return;
        }
        let Some(browse_id) = card.browse_id.clone() else { return };
        self.opening = Some(card.title.clone().into());
        self.error = None;
        cx.notify();
        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { youtube.tracks(&browse_id, &card) }).await;
            let _ = this.update(cx, |app, cx| {
                app.opening = None;
                match result {
                    Ok(items) => {
                        let tracks: Vec<Track> = items.into_iter().map(Track::from).collect();
                        for track in tracks.iter().take(30) {
                            app.fetch_art(&track.clone(), cx);
                        }
                        app.play_from(tracks, 0, cx);
                    }
                    Err(e) => app.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads every added addon's manifest (in the background), in order.
    pub(crate) fn connect_addons(&mut self, cx: &mut Context<Self>) {
        let urls = self.addon_urls.clone();
        if urls.is_empty() {
            self.addons.clear();
            return;
        }
        let http = self.youtube.client();
        cx.spawn(async move |this, cx| {
            let answers = cx.background_executor().spawn(async move { urls.iter().map(|u| (u.clone(), addon::connect(&http, u))).collect::<Vec<_>>() }).await;
            let _ = this.update(cx, |app, cx| {
                app.addons.clear();
                app.addon_errors.clear();
                for (url, answer) in answers {
                    match answer {
                        Ok(a) => app.addons.push(a),
                        Err(e) => {
                            app.addon_errors.insert(url, format!("{e:#}"));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Adds the addon at `raw`, once its manifest answers; the address is kept.
    pub(crate) fn add_addon(&mut self, raw: String, cx: &mut Context<Self>) {
        let base = addon::normalize_base(&raw);
        if base.is_empty() || self.adding_addon || self.addon_urls.contains(&base) {
            return;
        }
        self.adding_addon = true;
        self.addon_errors.remove("");
        cx.notify();
        let http = self.youtube.client();
        cx.spawn(async move |this, cx| {
            let answer = cx.background_executor().spawn(async move { addon::connect(&http, &base) }).await;
            let _ = this.update(cx, |app, cx| {
                app.adding_addon = false;
                match answer {
                    Ok(a) => {
                        app.addon_urls.push(a.base.clone());
                        app.addons.push(a);
                        app.save_addons();
                        app.addon_field.update(cx, |f, cx| f.clear_text(cx));
                    }
                    // Kept under "" for the add row to show.
                    Err(e) => {
                        app.addon_errors.insert(String::new(), format!("{e:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn remove_addon(&mut self, base: &str) {
        self.addon_urls.retain(|u| u != base);
        self.addons.retain(|a| a.base != base);
        self.addon_errors.remove(base);
        self.save_addons();
    }

    pub(crate) fn save_addons(&self) {
        settings::remove_prefixed("addon.");
        for (i, url) in self.addon_urls.iter().enumerate() {
            settings::set(&format!("addon.{i}"), url);
        }
    }

    /// Who is signed in, and their playlists.
    pub(crate) fn load_account(&mut self, cx: &mut Context<Self>) {
        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let found = cx.background_executor().spawn(async move { (youtube.account_info(), youtube.account_playlists()) }).await;
            let _ = this.update(cx, |app, cx| {
                match found {
                    (Ok(info), playlists) => {
                        if let Some(photo) = info.photo.clone() {
                            app.fetch_art_at("account".into(), photo, 96, 64, cx);
                        }
                        app.account = Some(info);
                        app.account_error = None;
                        app.load_account_likes(cx);
                        app.account_playlists = playlists.unwrap_or_default();
                        for card in app.account_playlists.clone() {
                            if let Some(url) = card.thumbnail.clone() {
                                app.fetch_art_at(card_key(&card), url, CARD_FETCH, CARD_KEEP, cx);
                            }
                        }
                    }
                    (Err(e), _) => app.account_error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens Google's sign-in window; once signed in, everything reloads as the account.
    pub(crate) fn sign_in(&mut self, cx: &mut Context<Self>) {
        if self.signing_in {
            return;
        }
        self.signing_in = true;
        self.account_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { account::sign_in() }).await;
            let _ = this.update(cx, |app, cx| {
                app.signing_in = false;
                match result {
                    Ok(Some(cookie)) => {
                        if let Err(e) = account::save(&cookie) {
                            app.account_error = Some(format!("{e:#}"));
                        }
                        app.youtube.set_account(Some(cookie));
                        app.load_account(cx);
                        app.load_feed(cx);
                    }
                    Ok(None) => {}
                    Err(e) => app.account_error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn sign_out(&mut self, cx: &mut Context<Self>) {
        cx.background_executor().spawn(async { account::sign_out() }).detach();
        self.youtube.set_account(None);
        self.account = None;
        self.account_playlists.clear();
        self.account_error = None;
        self.load_feed(cx);
        cx.notify();
    }

    /// Adds `track` to one of the account's playlists on YouTube Music; the menu closes.
    pub(crate) fn add_to_account_playlist(&mut self, card: youtube::Card, track: &Track, cx: &mut Context<Self>) {
        self.playlist_menu = None;
        self.playlist_menu_closed = Instant::now();
        let Some(id) = card.browse_id.clone() else { return };
        let (youtube, video_id) = (self.youtube.clone(), track.video_id.clone());
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { youtube.add_to_playlist(&id, &video_id) }).await;
            let _ = this.update(cx, |app, cx| {
                if let Err(e) = result {
                    app.error = Some(tf("Couldn't add to “{}”: {}", &[&card.title, &format!("{e:#}")]));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Adds `track` to playlist `i` (or a new one when `i` is None); the menu closes.
    pub(crate) fn add_to_playlist(&mut self, i: Option<usize>, track: &Track) {
        let i = i.unwrap_or_else(|| self.library.new_playlist());
        self.library.add(i, library::Saved::from(track));
        self.library.save();
        self.playlist_menu = None;
        self.playlist_menu_closed = Instant::now();
    }

    /// The menu of playlists to add a song to, at the point it was opened from.
    pub(crate) fn playlist_menu_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.playlist_menu.as_ref()?;
        let item = |id: (&'static str, usize), glyph: &'static str, label: SharedString, hint: SharedString| {
            div()
                .id(id)
                .h(px(36.0))
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
                .child(div().text_size(px(11.0)).text_color(rgba(TERTIARY)).child(hint))
        };
        let mut card = div()
            .id("playlist-menu")
            .w(px(260.0))
            .max_h(px(440.0))
            .overflow_y_scroll()
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
                this.playlist_menu = None;
                this.playlist_menu_closed = Instant::now();
                cx.notify();
            }))
            .child(div().px(px(10.0)).pt(px(6.0)).pb(px(4.0)).text_size(px(11.0)).font_weight(FontWeight::SEMIBOLD).text_color(rgba(TERTIARY)).child(t("ADD TO PLAYLIST")));
        let track = menu.track.clone();
        for (n, playlist) in self.library.playlists.iter().enumerate() {
            let has = playlist.tracks.iter().any(|s| s.video_id == track.video_id);
            let t = track.clone();
            card = card.child(
                item(("playlist-pick", n), if has { "\u{E73E}" } else { "\u{E8FD}" }, playlist.name.clone().into(), format!("{}", playlist.tracks.len()).into())
                    .when(has, |d| d.opacity(0.55))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.add_to_playlist(Some(n), &t);
                        cx.notify();
                    })),
            );
        }
        let t = track.clone();
        card = card.child(item(("playlist-new", 0), "\u{E710}", i18n::t("New playlist").into(), SharedString::default()).on_click(cx.listener(move |this, _, _, cx| {
            this.add_to_playlist(None, &t);
            cx.notify();
        })));
        // Signed in: the account's own playlists on YouTube Music ("Liked music" isn't one
        // to add to).
        let online: Vec<(usize, youtube::Card)> = self.account_playlists.iter().cloned().enumerate().filter(|(_, c)| c.browse_id.as_deref() != Some("VLLM")).collect();
        if !online.is_empty() {
            card = card
                .child(div().my(px(4.0)).mx(px(6.0)).h(px(1.0)).bg(rgba(STROKE)))
                .child(div().px(px(10.0)).pt(px(4.0)).pb(px(4.0)).text_size(px(11.0)).font_weight(FontWeight::SEMIBOLD).text_color(rgba(TERTIARY)).child(i18n::t("ON YOUTUBE MUSIC")));
            for (n, playlist) in online {
                let t = track.clone();
                let title: SharedString = playlist.title.clone().into();
                card = card.child(item(("account-pick", n), "\u{E753}", title, SharedString::default()).on_click(cx.listener(move |this, _, _, cx| {
                    this.add_to_account_playlist(playlist.clone(), &t, cx);
                    cx.notify();
                })));
            }
        }
        Some(card.into_any_element())
    }

    /// Opens the "add to playlist" menu for `track` at `at` (or closes it, on a second click).
    pub(crate) fn open_playlist_menu(&mut self, track: Track, at: gpui::Point<Pixels>) {
        if self.playlist_menu_closed.elapsed() > Duration::from_millis(300) {
            self.playlist_menu = Some(PlaylistMenu { track, at });
        }
    }

    // ---- Pages ------------------------------------------------------------------------

    /// One track as a row: number (or the playing mark), artwork, title and detail, length.
    pub(crate) fn row(&self, source: Source, i: usize, track: &Track, numbered: bool, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let here = self.current().is_some_and(|c| c.video_id == track.video_id);
        div()
            .id(gpui::ElementId::NamedInteger(format!("row-{}", source.key()).into(), i as u64))
            .h(px(56.0))
            .flex_none()
            .px_3()
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(6.0))
            .cursor_pointer()
            .hover(|s| s.bg(rgba(SUBTLE)))
            .when(here, |d| d.bg(rgba(SUBTLE_STRONG)))
            .when(numbered, |d| {
                d.child(if here {
                    icon(if self.playing { "\u{E767}" } else { "\u{E769}" }, 13.0).w(px(22.0)).text_color(rgb(accent()))
                } else {
                    div().w(px(22.0)).text_size(px(13.0)).text_color(rgba(TERTIARY)).child((i + 1).to_string())
                })
            })
            .child(artwork(self.art.get(&track.video_id).map(|a| a.small.clone()), 40.0, 6.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .when(here, |d| d.text_color(rgb(accent())))
                            .child(track.title.clone()),
                    )
                    .child(div().text_size(px(12.0)).text_color(rgba(SECONDARY)).truncate().child(track.detail.clone())),
            )
            .when(!track.duration.is_empty(), |d| d.child(div().text_size(px(12.0)).text_color(rgba(TERTIARY)).child(track.duration.clone())))
            .child({
                // On hover: quick Play Next, Add to Queue, and + (add to playlist) / × (remove from playlist).
                let group: SharedString = format!("row-{}-{i}", source.key()).into();
                let in_playlist = match source {
                    Source::Playlist(p) => Some(p),
                    _ => None,
                };
                let (t_next, t_queue, t_pl) = (track.clone(), track.clone(), track.clone());
                let row_btn = |id: gpui::ElementId, glyph: &'static str| {
                    div()
                        .id(id)
                        .size(px(26.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.0))
                        .font_family(ICONS)
                        .text_size(px(12.0))
                        .text_color(rgba(SECONDARY))
                        .hover(|s| s.bg(rgba(SUBTLE_STRONG)).text_color(rgb(0xffffff)))
                        .child(glyph)
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .opacity(0.0)
                    .group_hover(group, |s| s.opacity(1.0))
                    .child(
                        row_btn(gpui::ElementId::NamedInteger(format!("row-next-{}", source.key()).into(), i as u64), "\u{E7A7}").on_click(cx.listener(
                            move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.play_next(t_next.clone(), cx);
                            },
                        )),
                    )
                    .child(
                        row_btn(gpui::ElementId::NamedInteger(format!("row-queue-{}", source.key()).into(), i as u64), "\u{E8FD}").on_click(cx.listener(
                            move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.add_to_queue(t_queue.clone(), cx);
                            },
                        )),
                    )
                    .child(
                        row_btn(
                            gpui::ElementId::NamedInteger(format!("row-act-{}", source.key()).into(), i as u64),
                            if in_playlist.is_some() { "\u{E711}" } else { "\u{E710}" },
                        )
                        .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                            cx.stop_propagation();
                            match in_playlist {
                                Some(p) => {
                                    if let Some(list) = this.library.playlists.get_mut(p) {
                                        list.tracks.retain(|s| s.video_id != t_pl.video_id);
                                        this.library.save();
                                    }
                                }
                                None => this.open_playlist_menu(t_pl.clone(), event.position()),
                            }
                            cx.notify();
                        })),
                    )
            })
            .group(format!("row-{}-{i}", source.key()))
            .on_mouse_down(gpui::MouseButton::Right, {
                let t = track.clone();
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    this.open_track_menu(t.clone(), event.position, None);
                    cx.notify();
                })
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                let tracks = this.tracks_of(source);
                this.play_from(tracks, i, cx);
            }))
    }

    /// A 168 px card: square artwork with a title and a line under it.
    pub(crate) fn card(id: gpui::ElementId, art: Option<Arc<RenderImage>>, title: SharedString, subtitle: SharedString, here: bool) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .w(px(168.0))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .cursor_pointer()
            .child(artwork(art, 168.0, 10.0).shadow_lg().when(here, |d| d.border_2().border_color(rgb(accent()))))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).truncate().child(title))
                    .child(div().text_size(px(12.0)).text_color(rgba(SECONDARY)).truncate().child(subtitle)),
            )
    }

    pub(crate) fn rise(&self, i: usize) -> Motion {
        Motion::new(Target::default().y(0.0).opacity(1.0))
            .initial(Target::default().y(12.0).opacity(0.0))
            .delay(Duration::from_millis((i as u64 * 24).min(480)))
            .transition(Transition::Spring(self.spring))
    }

    pub(crate) fn heading(text: impl Into<SharedString>, size_px: f32) -> gpui::Div {
        div().font_family(DISPLAY).text_size(px(size_px)).font_weight(FontWeight::SEMIBOLD).child(text.into())
    }

    pub(crate) fn home_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let current = self.current().map(|c| c.video_id.clone());
        let art = |this: &Self, key: &str| this.art.get(key).map(|a| a.small.clone());

        // "Played lately": what played last here, as big cards (YouTube Music's own Home has a
        // "Listen again" of its own further down).
        let seeds = self.home.tracks.clone();
        // Sideways when it's wider than the page (up to 8 songs).
        let mut again = Vec::new();
        for (i, track) in seeds.iter().enumerate() {
            let rise = self.rise(i);
            let Some(frame) = self.home.motion.get_mut(i).map(|state| state.frame(rise, window, cx)) else { continue };
            let group: SharedString = format!("seed-card-{i}").into();
            let (t_next, t_queue, t_menu, t_rc) = (track.clone(), track.clone(), track.clone(), track.clone());
            let card_btn = |id: (&'static str, usize), glyph: &'static str| {
                div()
                    .id(id)
                    .size(px(26.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .font_family(ICONS)
                    .text_size(px(12.0))
                    .text_color(rgb(0xffffff))
                    .hover(|s| s.bg(rgba(0xffffff33)))
                    .child(glyph)
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            };
            let overlay = div()
                .absolute()
                .top(px(130.0))
                .right(px(6.0))
                .px(px(4.0))
                .py(px(3.0))
                .rounded_full()
                .bg(rgba(0x1414_14cc))
                .backdrop_blur(px(16.0))
                .border_1()
                .border_color(rgba(0xffffff22))
                .shadow_md()
                .flex()
                .items_center()
                .gap(px(2.0))
                .opacity(0.0)
                .group_hover(group.clone(), |s| s.opacity(1.0))
                .child(card_btn(("seed-next", i), "\u{E7A7}").on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.play_next(t_next.clone(), cx);
                })))
                .child(card_btn(("seed-queue", i), "\u{E8FD}").on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.add_to_queue(t_queue.clone(), cx);
                })))
                .child(card_btn(("seed-more", i), "\u{E712}").on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                    cx.stop_propagation();
                    this.open_track_menu(t_menu.clone(), event.position(), None);
                    cx.notify();
                })));
            let card = Self::card(("seed", i).into(), art(self, &track.video_id), track.title.clone(), track.artist.clone(), current.as_deref() == Some(track.video_id.as_str()))
                .relative()
                .group(group)
                .child(overlay)
                .on_mouse_down(gpui::MouseButton::Right, cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    this.open_track_menu(t_rc.clone(), event.position, None);
                    cx.notify();
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.shelf_dragged {
                        return;
                    }
                    let list = this.tracks_of(Source::Seeds);
                    this.play_from(list, i, cx);
                }));
            again.push(frame.apply(card).into_any_element());
        }
        let again = (!again.is_empty()).then(|| self.shelf("listen-again".into(), Self::heading(t("Played lately"), 20.0), again, window, cx));

        let reload_btn = action("home-reload", "\u{E72C}", false).on_click(cx.listener(|this, _, _, cx| {
            let recent: Vec<Track> = this.library.history.iter().take(8).map(Track::from).collect();
            for track in &recent {
                this.fetch_art(track, cx);
            }
            this.home = Listing::new(recent);
            this.load_feed(cx);
            cx.notify();
        }));

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .pr(px(8.0))
            .child(Self::heading(t("Home"), 30.0))
            .child(reload_btn);

        let mut page = div()
            .id("home")
            .size_full()
            .overflow_y_scroll()
            .px(px(32.0))
            .pt(px(28.0))
            .pb(px(40.0))
            .flex()
            .flex_col()
            .gap_6()
            .child(header)
            // Nothing played yet (a fresh install): no empty shelf.
            .children(again);

        // Songs like the ones played lately.
        if !self.recommended.is_empty() {
            let mut grid = div().flex().flex_wrap();
            for (i, track) in self.recommended.clone().iter().enumerate() {
                grid = grid.child(div().w(relative(1.0 / 3.0)).pr_2().child(self.row(Source::Recommended, i, track, false, cx)));
            }
            page = page.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(Self::heading(t("Recommended for you"), 20.0))
                    .child(div().text_size(px(12.0)).text_color(rgba(TERTIARY)).mt(px(-8.0)).child(t("From what you've played lately")))
                    .child(div().mx(px(-12.0)).child(grid)),
            );
        }

        // YouTube Music's own shelves: songs as a three-column grid, playlists and albums as a
        // row of cards that scrolls sideways.
        let shelves: Vec<(SharedString, Vec<Track>, Vec<youtube::Card>)> = match &self.feed {
            Feed::Ready(shelves) => shelves.iter().map(|s| (s.title.clone(), s.songs.clone(), s.cards.clone())).collect(),
            Feed::Loading => {
                return page
                    .child(div().text_size(px(13.0)).text_color(rgba(TERTIARY)).child(t("Loading Home from YouTube Music…")))
                    .into_any_element()
            }
            Feed::Failed(e) => {
                return page
                    .child(div().text_size(px(13.0)).text_color(rgba(TERTIARY)).child(tf("Couldn't load YouTube Music Home: {}", &[e])))
                    .into_any_element()
            }
        };
        for (n, (title, songs, cards)) in shelves.into_iter().enumerate() {
            let rise = self.rise(n + 2);
            let Some(frame) = self.feed_motion.get_mut(n).map(|state| state.frame(rise, window, cx)) else { continue };
            let mut shelf = div().flex().flex_col().gap_3();
            // The title on top, unless the shelf is only cards: then it sits by their ‹ ›.
            if cards.is_empty() || !songs.is_empty() {
                shelf = shelf.child(Self::heading(title.clone(), 20.0));
            }
            if !songs.is_empty() {
                let mut grid = div().flex().flex_wrap();
                for (i, track) in songs.iter().take(12).enumerate() {
                    grid = grid.child(div().w(relative(1.0 / 3.0)).pr_2().child(self.row(Source::Shelf(n), i, track, false, cx)));
                }
                shelf = shelf.child(div().mx(px(-12.0)).child(grid));
            }
            if !cards.is_empty() {
                // Named by its first card too, so a new Home starts each row at its start.
                let key: SharedString = format!("home-{n}-{}", card_key(&cards[0])).into();
                let mut row = Vec::new();
                for (i, card) in cards.into_iter().enumerate() {
                    let opening = self.opening.as_ref().is_some_and(|t| t.as_ref() == card.title);
                    let element = Self::card(
                        gpui::ElementId::NamedInteger(format!("card-{n}").into(), i as u64),
                        art(self, &card_key(&card)),
                        card.title.clone().into(),
                        card.subtitle.clone().into(),
                        false,
                    )
                    .when(opening, |d| d.opacity(0.6))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.shelf_dragged {
                            this.open_card(card.clone(), cx);
                        }
                    }));
                    row.push(element.into_any_element());
                }
                // Songs and cards both: the title is over the songs, the cards under them.
                let heading = if songs.is_empty() { Some(Self::heading(title.clone(), 20.0)) } else { None };
                shelf = shelf.child(self.shelf(key, div().children(heading), row, window, cx));
            }
            page = page.child(frame.apply(shelf));
        }
        page.into_any_element()
    }

    pub(crate) fn search_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let subtitle: SharedString = match (&self.search_error, self.searching) {
            (Some(e), _) => e.clone().into(),
            (None, true) => t("Searching YouTube Music…").into(),
            (None, false) => songs(self.results.tracks.len()).into(),
        };
        let tracks = self.results.tracks.clone();
        let mut rows = Vec::new();
        for (i, track) in tracks.iter().enumerate() {
            let rise = self.rise(i);
            let frame = match self.results.motion.get_mut(i) {
                Some(state) => state.frame(rise, window, cx),
                None => continue,
            };
            rows.push(frame.apply(self.row(Source::Results, i, track, true, cx)).into_any_element());
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(32.0))
                    .pt(px(28.0))
                    .pb_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(Self::heading(tf("Results for “{}”", &[&self.query]), 30.0))
                    .child(div().text_size(px(13.0)).text_color(rgba(SECONDARY)).child(subtitle)),
            )
            .child(div().id("results").flex_1().min_h_0().overflow_y_scroll().px(px(20.0)).pb(px(40.0)).flex().flex_col().children(rows))
            .into_any_element()
    }
}
