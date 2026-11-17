//! What plays: starting a song, loading it, the queue, the transport's actions,
//! volume and seeking, media keys, crossfades and what comes next.

use super::*;

impl NabyTune {
    pub(crate) fn tracks_of(&self, source: Source) -> Vec<Track> {
        match (source, &self.feed) {
            (Source::Seeds, _) => self.home.tracks.clone(),
            (Source::Results, _) => self.results.tracks.clone(),
            (Source::Shelf(n), Feed::Ready(shelves)) => shelves.get(n).map(|s| s.songs.clone()).unwrap_or_default(),
            (Source::Recommended, _) => self.recommended.clone(),
            (Source::Recent, _) => self.library.history.iter().map(Track::from).collect(),
            (Source::Playlist(n), _) => self.library.playlists.get(n).map(|p| p.tracks.iter().map(Track::from).collect()).unwrap_or_default(),
            (Source::Artist, _) => self.artist_view.as_ref().map(|v| v.songs.clone()).unwrap_or_default(),
            (Source::Collection, _) => self.collection_view.as_ref().map(|v| v.tracks.clone()).unwrap_or_default(),
            (Source::Local, _) => self.local_tracks.iter().map(local::track_of).collect(),
            _ => Vec::new(),
        }
    }

    // ---- Playback -------------------------------------------------------------

    pub(crate) fn play_from(&mut self, list: Vec<Track>, index: usize, cx: &mut Context<Self>) {
        // Another list of songs: whatever radio ran is over.
        self.radio = None;
        self.queue = list;
        self.index = Some(index);
        self.playing = true;
        self.start(cx);
    }

    /// Keeps a song's big cover for when it plays again, the oldest dropped past a few.
    pub(crate) fn keep_big_cover(&mut self, video_id: String, big: BigCover) {
        self.big_covers.retain(|(id, _)| *id != video_id);
        self.big_covers.push((video_id, big));
        let excess = self.big_covers.len().saturating_sub(BIG_COVERS);
        self.big_covers.drain(..excess);
    }

    /// Fetches the next song's big cover while this one plays, so skipping shows it sharp.
    pub(crate) fn prefetch_next_cover(&mut self, cx: &mut Context<Self>) {
        let Some(next) = self.index.and_then(|i| self.queue.get(i + 1)).cloned() else { return };
        if next.local.is_some() || self.big_covers.iter().any(|(id, _)| *id == next.video_id) {
            return;
        }
        let Some(url) = next.thumbnail.clone() else { return };
        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let big = cx.background_executor().spawn(async move { youtube.cover_image(&url).ok().map(|img| BigCover::from(&img)) }).await;
            let _ = this.update(cx, |app, _| {
                if let Some(big) = big {
                    app.keep_big_cover(next.video_id, big);
                }
            });
        })
        .detach();
    }

    /// "Recommended for you": songs like the two played last, none of them played lately.
    pub(crate) fn recommend(&mut self, cx: &mut Context<Self>) {
        let seeds: Vec<String> = self.library.history.iter().take(2).map(|s| s.video_id.clone()).collect();
        if seeds.is_empty() {
            return;
        }
        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let lists = cx.background_executor().spawn(async move { seeds.iter().map(|id| youtube.radio(id).unwrap_or_default()).collect::<Vec<_>>() }).await;
            let _ = this.update(cx, |app, cx| {
                let played: HashSet<String> = app.library.history.iter().take(30).map(|s| s.video_id.clone()).collect();
                // Taken in turns from each song's radio, so both have a say.
                let mut picked: Vec<Track> = Vec::new();
                let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
                for i in 0..longest {
                    for list in &lists {
                        if let Some(item) = list.get(i) {
                            if !played.contains(&item.video_id) && !app.library.is_disliked(&item.video_id) && !picked.iter().any(|t| t.video_id == item.video_id) {
                                picked.push(Track::from(item.clone()));
                            }
                        }
                    }
                }
                picked.truncate(12);
                for track in &picked {
                    app.fetch_art(track, cx);
                }
                app.recommended = picked;
                cx.notify();
            });
        })
        .detach();
    }

    /// Toggles smart vibe-matched Autoplay on or off, immediately topping up Up Next when turned on.
    pub(crate) fn toggle_autoplay(&mut self, cx: &mut Context<Self>) {
        self.autoplay = !self.autoplay;
        settings::set("autoplay", if self.autoplay { "1" } else { "0" });
        if self.autoplay {
            self.toast = Some((t("Autoplay on · similar vibes").into(), Instant::now()));
            self.ensure_autoplay(cx);
        } else {
            self.autoplay_fetching = None;
            self.toast = Some((t("Autoplay off").into(), Instant::now()));
        }
        cx.notify();
    }

    /// Keeps Up Next stocked with vibe-matched recommendations whenever Autoplay is on
    /// and 2 or fewer upcoming songs remain.
    pub(crate) fn ensure_autoplay(&mut self, cx: &mut Context<Self>) {
        if !self.autoplay || self.radio.is_some() || self.repeat == Repeat::One {
            return;
        }
        let Some(i) = self.index else { return };
        let remaining = self.queue.len().saturating_sub(i + 1);
        if remaining > 2 {
            return;
        }
        let Some(seed) = self.queue.last().or_else(|| self.queue.get(i)).cloned() else { return };
        if self.autoplay_fetching.as_deref() == Some(seed.video_id.as_str()) {
            return;
        }
        self.extend_with_radio(seed, cx);
    }

    /// Queues vibe-matched songs similar to `seed` after the last song in the queue,
    /// balancing acoustic/mood vibe closeness with artist variety and personal taste.
    pub(crate) fn extend_with_radio(&mut self, seed: Track, cx: &mut Context<Self>) {
        let norm_artist = |s: &str| -> String {
            s.split(['•', ',', '&', '×', '/'])
                .next()
                .unwrap_or(s)
                .trim()
                .to_lowercase()
        };

        let seed_id = seed.video_id.clone();
        let seed_title = seed.title.to_string();
        let seed_artist_raw = seed.artist.to_string();
        let seed_artist = norm_artist(&seed_artist_raw);
        let is_local = local_files::path_of(&seed_id).is_some();

        let mut taste_artists: HashSet<String> = HashSet::new();
        for s in self.library.liked.iter().take(60) {
            let a = norm_artist(&s.artist);
            if !a.is_empty() {
                taste_artists.insert(a);
            }
        }
        for s in self.library.history.iter().take(30) {
            let a = norm_artist(&s.artist);
            if !a.is_empty() {
                taste_artists.insert(a);
            }
        }
        let liked_ids: HashSet<String> = self.library.liked.iter().map(|s| s.video_id.clone()).collect();
        let rec_ids: HashSet<String> = self.recommended.iter().map(|t| t.video_id.clone()).collect();

        self.autoplay_fetching = Some(seed_id.clone());
        cx.notify();

        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    let yt_id = if is_local {
                        let query = format!("{seed_title} {seed_artist_raw}");
                        youtube
                            .search(&query)
                            .ok()
                            .and_then(|list| list.into_iter().next())
                            .map(|item| item.video_id)
                            .unwrap_or_default()
                    } else {
                        seed_id.clone()
                    };
                    if yt_id.is_empty() {
                        return Vec::new();
                    }
                    youtube.radio(&yt_id).unwrap_or_default()
                })
                .await;

            let _ = this.update(cx, |app, cx| {
                app.autoplay_fetching = None;
                if !app.autoplay || app.radio.is_some() {
                    cx.notify();
                    return;
                }

                // Collect unique, non-disliked vibe candidates with base vibe + taste affinity scores.
                let mut pool: Vec<(f32, String, Track)> = Vec::new();
                for (pos, item) in found.into_iter().enumerate() {
                    if app.queue.iter().any(|t| t.video_id == item.video_id)
                        || app.library.is_disliked(&item.video_id)
                        || pool.iter().any(|(_, _, t)| t.video_id == item.video_id)
                    {
                        continue;
                    }
                    let track = Track::from(item);
                    let artist_key = norm_artist(&track.artist);
                    // Preserve YouTube Music's acoustic/mood vibe order as primary signal.
                    let mut score = 100.0 - (pos as f32) * 1.8;
                    if !artist_key.is_empty() && artist_key != seed_artist && taste_artists.contains(&artist_key) {
                        score += 8.0;
                    }
                    if rec_ids.contains(&track.video_id) {
                        score += 6.0;
                    }
                    if liked_ids.contains(&track.video_id) {
                        score += 5.0;
                    }
                    pool.push((score, artist_key, track));
                }

                // Fallback to personalized Home recommendations if radio came back empty.
                if pool.is_empty() {
                    for (pos, track) in app.recommended.clone().into_iter().enumerate() {
                        if app.queue.iter().any(|t| t.video_id == track.video_id)
                            || app.library.is_disliked(&track.video_id)
                            || pool.iter().any(|(_, _, t)| t.video_id == track.video_id)
                        {
                            continue;
                        }
                        let artist_key = norm_artist(&track.artist);
                        pool.push((80.0 - (pos as f32) * 2.0, artist_key, track));
                    }
                }

                // Greedy vibe-preserving diversity selection:
                // Prioritizes close vibes from varied artists, while still allowing the same artist
                // spaced out naturally when its vibe matches strongly.
                let mut last_artist = seed_artist.clone();
                let mut artist_counts: HashMap<String, usize> = HashMap::new();
                if !seed_artist.is_empty() {
                    artist_counts.insert(seed_artist, 1);
                }

                let mut added = 0usize;
                while added < 15 && !pool.is_empty() {
                    let mut best_idx = 0usize;
                    let mut best_eff = f32::NEG_INFINITY;
                    for (idx, (base_score, artist_key, _)) in pool.iter().enumerate() {
                        let mut eff = *base_score;
                        if !artist_key.is_empty() {
                            if *artist_key == last_artist {
                                eff -= 45.0;
                            }
                            let count = artist_counts.get(artist_key).copied().unwrap_or(0);
                            eff -= (count as f32) * 18.0;
                        }
                        if eff > best_eff {
                            best_eff = eff;
                            best_idx = idx;
                        }
                    }
                    let (_, artist_key, track) = pool.remove(best_idx);
                    if !artist_key.is_empty() {
                        *artist_counts.entry(artist_key.clone()).or_insert(0) += 1;
                        last_artist = artist_key;
                    }
                    app.fetch_art(&track, cx);
                    app.queue.push(track);
                    added += 1;
                }

                if added > 0 {
                    if app.shuffle && app.shuffle_next.is_none() {
                        app.shuffle_next = app.pick_shuffle();
                    }
                    app.prefetch_next(cx);
                    app.prefetch_next_cover(cx);
                    app.save_session();
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Shows the current track at once with what is at hand, then loads its stream and big cover.
    pub(crate) fn start(&mut self, cx: &mut Context<Self>) {
        let Some(track) = self.index.and_then(|i| self.queue.get(i)).cloned() else { return };
        self.previous_ambient = self.ambient.take();
        self.cover_motion = MotionState::default();
        self.stage_cover_motion = MotionState::default();
        self.ambient_motion = MotionState::default();
        // The row's thumbnail is plenty to work the backdrop out from.
        let source = track.local.clone().or_else(|| self.art.get(&track.video_id).and_then(|a| ambient::rgba_of(&a.small)).map(Arc::new));
        let cached = self.big_covers.iter().find(|(id, _)| *id == track.video_id).map(|(_, b)| b.clone());
        self.cover_sharp = track.local.is_some() || cached.is_some();
        (self.cover, self.cover_large) = match (&track.local, self.art.get(&track.video_id)) {
            (Some(local), _) => (Some(ambient::cover(local, 224)), Some(ambient::cover(local, STAGE_ART))),
            // Until the big one arrives, the row's artwork stands in for both.
            (None, Some(art)) => (Some(art.small.clone()), Some(art.small.clone())),
            _ => (None, None),
        };
        // A thumbnail is plenty for a backdrop blurred this far.
        self.ambient = source.as_ref().map(|s| ambient::ambient(s, 0.85));
        self.stage_art = source.as_ref().map(|s| StageArt { soft: ambient::soft(s), colors: ambient::palette(s) });
        // "Cover" accent: the song's colour, from the artwork at hand (the big cover refines it).
        if let Some(s) = &source {
            appearance::set_cover_colour(ambient::accent_from(s));
        }
        // Its big cover, made ready before (the next song's is fetched ahead): sharp at once.
        if let Some(big) = &cached {
            self.cover = Some(big.cover.clone());
            self.cover_large = Some(big.cover_large.clone());
            self.ambient = Some(big.glow.clone());
            self.stage_art = Some(big.art.clone());
            self.vinyl_base = Some(big.vinyl_base.clone());
            appearance::set_cover_colour(big.colour);
        } else {
            self.vinyl_base = source.as_ref().map(|s| ambient::vinyl_base(s, 256));
        }
        if let Some((a, _)) = &mut self.vinyl_frame {
            *a = -999.0;
        }

        self.generation += 1;
        let generation = self.generation;
        self.loading = true;
        self.error = None;
        self.length = Duration::ZERO;
        self.lyrics = Lyrics::Loading;
        self.lyric_options.clear();
        self.lyric_motion = LyricMotion::new(0);
        self.extras = LyricExtras::default();
        let fade = std::mem::take(&mut self.fade_next);
        // A crossfade keeps the last song going under this one; otherwise it stops now.
        if !fade {
            self.audio.pause();
        }
        let request = self.load_request(&track, cached.is_none());
        // Fetched ahead while the last song played: it starts at once.
        if let Some((_, loaded)) = self.prefetched.take().filter(|(id, _)| *id == track.video_id) {
            self.loaded(generation, &track, Ok(loaded), fade, cx);
            cx.notify();
            return;
        }
        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { fetch_loaded(&youtube, &request) }).await;
            let _ = this.update(cx, |app, cx| {
                if app.generation != generation {
                    return;
                }
                app.loaded(generation, &track, result, fade, cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// What fetching `track` takes, with the settings as they are.
    pub(crate) fn load_request(&self, track: &Track, big_wanted: bool) -> LoadRequest {
        LoadRequest {
            video_id: track.video_id.clone(),
            title: track.title.to_string(),
            artist: track.artist.to_string(),
            seconds: clock_seconds(&track.duration),
            big: (track.local.is_none() && big_wanted).then(|| track.thumbnail.clone()).flatten(),
            // Lossless first, from the addons in order, when they're on.
            addons: if self.lossless { self.addons.clone() } else { Vec::new() },
            tier: self.tier,
        }
    }

    /// The song after this one, as it will come when this one ends (shuffle's pick is made
    /// ahead, so it can be fetched ahead too).
    pub(crate) fn next_index(&self) -> Option<usize> {
        let i = self.index?;
        let n = self.queue.len();
        match self.repeat {
            Repeat::One => Some(i),
            _ if self.shuffle && n > 1 => self.shuffle_next,
            Repeat::All if n > 0 => Some((i + 1) % n),
            _ => (i + 1 < n).then_some(i + 1),
        }
    }

    /// Fetches the next song's stream (and cover) while this one plays: skipping to it, or
    /// crossfading into it, starts at once.
    pub(crate) fn prefetch_next(&mut self, cx: &mut Context<Self>) {
        let Some(next) = self.next_index().filter(|n| Some(*n) != self.index).and_then(|n| self.queue.get(n)).cloned() else { return };
        if self.prefetched.as_ref().is_some_and(|(id, _)| *id == next.video_id) || self.prefetching.as_deref() == Some(next.video_id.as_str()) {
            return;
        }
        let cached = self.big_covers.iter().any(|(id, _)| *id == next.video_id);
        let request = self.load_request(&next, !cached);
        self.prefetching = Some(next.video_id.clone());
        let youtube = self.youtube.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { fetch_loaded(&youtube, &request) }).await;
            let _ = this.update(cx, |app, _| {
                if app.prefetching.as_deref() == Some(next.video_id.as_str()) {
                    app.prefetching = None;
                }
                if let Ok(mut loaded) = result {
                    if let Some(big) = loaded.big.take() {
                        app.keep_big_cover(next.video_id.clone(), big);
                    }
                    app.prefetched = Some((next.video_id, loaded));
                }
            });
        })
        .detach();
    }

    /// A track's stream (or why there's none) is here: play it.
    pub(crate) fn loaded(&mut self, generation: u64, track: &Track, result: anyhow::Result<Loaded>, fade: bool, cx: &mut Context<Self>) {
        let lyric_id = track.video_id.clone();
        let saved = library::Saved::from(track);
        let (title, artist) = (track.title.to_string(), youtube::clean_artist(&track.artist));
        let cover_url = track.thumbnail.as_ref().map(|url| youtube::sized(url, 544));
        self.loading = false;
        match result {
            Ok(loaded) => {
                self.failed_in_row = 0;
                if loaded.cover.is_some() {
                    self.cover = loaded.cover;
                    self.cover_large = loaded.cover_large;
                }
                if let Some(big) = loaded.big {
                    appearance::set_cover_colour(big.colour);
                    self.vinyl_base = Some(big.vinyl_base.clone());
                    if let Some((a, _)) = &mut self.vinyl_frame {
                        *a = -999.0;
                    }
                    if self.ambient.is_none() {
                        self.ambient = Some(big.glow.clone());
                        self.ambient_motion = MotionState::default();
                    }
                    // The sharper backgrounds, now they're here.
                    self.stage_art = Some(big.art.clone());
                    self.cover_sharp = true;
                    self.keep_big_cover(lyric_id.clone(), big);
                }
                // The next song's pick under shuffle, made now so it can be fetched ahead.
                self.shuffle_next = self.pick_shuffle();
                self.prefetch_next_cover(cx);
                self.length = loaded.length;
                self.track_gain = loaded.volume;
                self.quality = loaded.quality.clone();
                self.lossless_from = loaded.lossless_from.clone();
                self.library.played(saved);
                self.library.save();
                // Update "Played lately" in real time as songs play:
                let recent: Vec<Track> = self.library.history.iter().take(8).map(Track::from).collect();
                for r in &recent {
                    self.fetch_art(r, cx);
                }
                self.home = Listing::new(recent);
                // Keep Up Next stocked with vibe-matched recommendations when Autoplay is on.
                self.ensure_autoplay(cx);
                let fade = if fade { Duration::from_secs(self.crossfade as u64) } else { Duration::ZERO };
                self.audio.load(generation, loaded.bytes, self.output_volume(), self.playing, fade);
                // Coming back to where the last session left off.
                if let Some(at) = self.resume_at.take() {
                    self.audio.seek(at.min(loaded.length));
                }
                if let Some(media) = &mut self.media {
                    let album = track.detail.split_once(" • ").map(|(_, a)| a.trim()).filter(|a| !a.is_empty());
                    media.track(&title, &artist, album, cover_url.as_deref(), Some(loaded.length));
                    media.state(self.playing, Duration::ZERO);
                }
                if let Some(bridge) = &self.bridge {
                    let album = track.detail.split_once(" • ").map(|(_, a)| a.trim()).filter(|a| !a.is_empty());
                    bridge.broadcast_track(
                        &title,
                        &artist,
                        album,
                        cover_url.as_deref(),
                        loaded.length,
                        &lyric_id,
                        self.playing,
                    );
                }
                self.on_track_changed(cx);
                self.fetch_lyrics(generation, lyric_id, title, artist, loaded.length, cx);
            }
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                self.lyrics = Lyrics::Idle;
                // A song that won't play doesn't stop the music: the next one starts after a
                // moment (time enough to read why). A run of them stops it, as it would loop.
                self.failed_in_row += 1;
                let has_next = self.next_index().is_some();
                if self.playing && has_next && self.failed_in_row <= 5 {
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(2500)).await;
                        let _ = this.update(cx, |app, cx| {
                            if app.generation == generation && app.error.is_some() {
                                app.skip(1, cx);
                            }
                        });
                    })
                    .detach();
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn now(&self) -> Duration {
        let status = self.audio.status();
        if status.generation == self.generation && !self.loading { status.position } else { Duration::ZERO }
    }

    pub(crate) fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.index.is_none() {
            let list = if self.tab == Tab::Search && !self.results.tracks.is_empty() { &self.results } else { &self.home };
            if !list.tracks.is_empty() {
                self.play_from(list.tracks.clone(), 0, cx);
            }
            return;
        }
        self.playing = !self.playing;
        if self.playing { self.audio.play() } else { self.audio.pause() }
        self.report_media(self.now());
        cx.notify();
    }

    /// What the audio engine plays at: the listener's volume (on a squared curve, as ears
    /// hear loudness), times the track's normalising gain when normalisation is on.
    pub(crate) fn output_volume(&self) -> f32 {
        (if self.normalize { self.track_gain } else { 1.0 }) * self.volume * self.volume
    }

    /// Sets the listener's volume (0 to 1) and unmutes, as moving a volume slider does.
    pub(crate) fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if self.muted && self.volume > 0.0 {
            self.muted = false;
            self.audio.mute(false);
        }
        self.audio.set_volume(self.output_volume());
        settings::set("volume", &format!("{:.2}", self.volume));
    }

    pub(crate) fn seek(&mut self, fraction: f32) {
        self.seek_to(self.length.mul_f32(fraction.clamp(0.0, 1.0)));
    }

    pub(crate) fn seek_to(&mut self, to: Duration) {
        if !self.loading && self.length > Duration::ZERO {
            let to = to.min(self.length);
            self.audio.seek(to);
            self.report_media(to);
        }
    }

    /// Tells Windows' media controls where playback stands.
    pub(crate) fn report_media(&mut self, position: Duration) {
        let playing = self.playing;
        if let Some(media) = &mut self.media {
            media.state(playing, position);
        }
        if let Some(bridge) = &self.bridge {
            bridge.broadcast_state(playing);
        }
        if let Some(tray) = &self.tray {
            tray.set_playing(playing);
        }
        if let Some(tb) = &self.taskbar {
            tb.set_playing(playing);
        }
    }

    /// A media key, or a button in Windows' media flyout.
    pub(crate) fn media_event(&mut self, event: media::Event, cx: &mut Context<Self>) {
        use media::{Event, SeekDirection};
        let step = |this: &Self, direction: SeekDirection, by: Duration| match direction {
            SeekDirection::Forward => this.now() + by,
            SeekDirection::Backward => this.now().saturating_sub(by),
        };
        match event {
            Event::Toggle => self.toggle(cx),
            Event::Play if !self.playing => self.toggle(cx),
            Event::Pause | Event::Stop if self.playing => self.toggle(cx),
            Event::Next => self.skip(1, cx),
            Event::Previous => self.skip(-1, cx),
            Event::Seek(direction) => self.seek_to(step(self, direction, Duration::from_secs(10))),
            Event::SeekBy(direction, by) => self.seek_to(step(self, direction, by)),
            Event::SetPosition(position) => self.seek_to(position.0),
            _ => {}
        }
        cx.notify();
    }

    /// Next or previous; with shuffle on, next is any other track in the queue.
    pub(crate) fn skip(&mut self, by: isize, cx: &mut Context<Self>) {
        let (Some(i), n) = (self.index, self.queue.len()) else { return };
        if n == 0 {
            return;
        }
        let next = if self.shuffle && by > 0 && n > 1 {
            self.shuffle_next.filter(|p| *p < n && *p != i).or_else(|| self.pick_shuffle()).unwrap_or((i + 1) % n)
        } else {
            ((i as isize + by).rem_euclid(n as isize)) as usize
        };
        self.index = Some(next);
        self.start(cx);
    }

    /// A random other song of the queue, for shuffle.
    pub(crate) fn pick_shuffle(&self) -> Option<usize> {
        let (i, n) = (self.index?, self.queue.len());
        if n < 2 {
            return None;
        }
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize);
        let pick = (seed ^ (self.generation as usize).wrapping_mul(2_654_435_761)) % (n - 1);
        Some(if pick >= i { pick + 1 } else { pick })
    }

    /// Near the end of a song with crossfade on: the next one starts, fading in over it.
    pub(crate) fn maybe_crossfade(&mut self, cx: &mut Context<Self>) {
        if self.crossfade == 0 || !self.playing || self.loading || self.length.is_zero() || self.crossfaded == self.generation {
            return;
        }
        let left = self.length.saturating_sub(self.now());
        if left > Duration::from_secs(self.crossfade as u64) || self.now().is_zero() {
            return;
        }
        // Only into a song that's here already, and never past a sleep timer or Repeat One.
        let Some(next) = self.next_index().filter(|n| Some(*n) != self.index) else { return };
        let ready = self.queue.get(next).is_some_and(|t| self.prefetched.as_ref().is_some_and(|(id, _)| *id == t.video_id));
        if !ready || self.sleep == Sleep::EndOfSong || self.repeat == Repeat::One {
            return;
        }
        self.crossfaded = self.generation;
        self.fade_next = true;
        self.index = Some(next);
        self.start(cx);
    }

    pub(crate) fn track_ended(&mut self, cx: &mut Context<Self>) {
        // Sleep timer set to the end of the song: stop here, whatever comes next.
        if self.sleep == Sleep::EndOfSong {
            self.sleep = Sleep::Off;
            self.playing = false;
            self.report_media(self.length);
            cx.notify();
            return;
        }
        match (self.repeat, self.index) {
            (Repeat::One, Some(_)) => self.start(cx),
            (Repeat::All, Some(_)) => self.skip(1, cx),
            (Repeat::Off, Some(i)) if self.shuffle || i + 1 < self.queue.len() => self.skip(1, cx),
            _ => self.playing = false,
        }
    }

    pub(crate) fn current(&self) -> Option<&Track> {
        self.index.and_then(|i| self.queue.get(i))
    }

    /// Takes a colour (or "system") as the accent, or "cover" to follow each song's cover.
    pub(crate) fn pick_accent(&mut self, key: &str) {
        let cover = key == "cover";
        appearance::set_follow_cover(cover);
        settings::set("accent_cover", if cover { "1" } else { "0" });
        if !cover {
            appearance::set_accent(key);
            settings::set("accent", key);
            self.accent_choice = key.to_string();
        }
    }

    pub(crate) fn open_panel(&mut self, panel: Panel) {
        self.panel = if self.panel == panel { Panel::None } else { panel };
        self.panel_motion = MotionState::default();
    }
}
