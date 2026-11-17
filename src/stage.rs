//! The full-screen player: its layouts and backgrounds, the rail at the right edge, its
//! volume pop-out, and opening and closing it.

use super::*;

impl NabyTune {
    /// Opens the full-screen player, or starts it sliding away (it's gone once it has).
    pub(crate) fn set_full_screen(&mut self, open: bool) {
        if !open {
            if self.full_screen && self.stage_closing.is_none() {
                self.stage_closing = Some(Instant::now());
                self.stage_close_drawn = false;
                self.layout_menu = false;
                self.stage_volume_open = false;
                if self.look_menu == Some(LookAnchor::Stage) {
                    self.look_menu = None;
                }
            }
            return;
        }
        if !self.full_screen || self.stage_closing.is_some() {
            self.stage_motion = MotionState::default();
            self.stage_cover_motion = MotionState::default();
        }
        self.stage_closing = None;
        if !self.full_screen {
            self.full_screen = true;
            // One lyric column at a time: its scroll and measurements are shared.
            self.lyric_motion.scroll = MotionState::default();
        }
    }

    /// The full-screen player is up and staying (not on its way out).
    pub(crate) fn stage_open(&self) -> bool {
        self.full_screen && self.stage_closing.is_none()
    }

    /// The full-screen player's controls, in one quiet column at the right edge, halfway
    /// down: how it looks (layout, lyric style, romaji), then where else the song can go (the
    /// mini player, lyrics on the desktop). Just symbols, lit when on; each names itself on
    /// hover, and the menus open beside it.
    pub(crate) fn stage_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        #[derive(Clone, Copy, PartialEq)]
        enum State {
            Idle,
            Open,
            On,
        }
        type Press = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>;
        let button = |id: &'static str, hint: &'static str, glyph: AnyElement, state: State, pending: bool, press: Press| {
            div()
                .relative()
                .group(id)
                .child(
                    div()
                        .id(id)
                        .size(px(34.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .cursor_pointer()
                        .map(|d| match state {
                            State::Idle => d.text_color(rgba(0xffffff8c)).hover(|s| s.bg(rgba(0xffffff14)).text_color(rgb(0xffffff))),
                            State::Open => d.bg(rgba(0xffffff1f)).text_color(rgb(0xffffff)),
                            State::On => d.bg(rgba(accent_wash(0x26))).text_color(rgb(accent())).hover(|s| s.bg(rgba(accent_wash(0x3d)))),
                        })
                        .when(pending, |d| d.opacity(0.5))
                        .child(glyph)
                        .on_click(press),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(7.0))
                        .right(px(44.0))
                        .px(px(8.0))
                        .py(px(3.0))
                        .rounded(px(6.0))
                        .bg(rgba(GLASS))
                        .text_size(px(11.0))
                        .font_weight(FontWeight::MEDIUM)
                        .whitespace_nowrap()
                        .text_color(rgb(0xffffff))
                        .opacity(0.0)
                        .when(state != State::Open, |d| d.group_hover(id, |s| s.opacity(1.0)))
                        .child(t(hint)),
                )
        };
        let state = |open: bool| if open { State::Open } else { State::Idle };
        let toggled = |on: bool| if on { State::On } else { State::Idle };
        let glyph = |code: &'static str, size: f32| div().font_family(ICONS).text_size(px(size)).child(code).into_any_element();
        // Between groups: a dot, not a rule.
        let dot = || div().my(px(3.0)).size(px(3.0)).rounded_full().bg(rgba(0xffffff33));

        let layout = button(
            "rail-layout",
            "Layout & background",
            glyph("\u{F0E2}", 14.0),
            state(self.layout_menu),
            false,
            Box::new(cx.listener(|this, _, _, cx| {
                // The mouse-down that closed the menu is this click's own: leave it closed.
                if this.layout_menu_closed.elapsed() > Duration::from_millis(300) {
                    this.layout_menu = !this.layout_menu;
                }
                cx.notify();
            })),
        );
        let look_open = self.look_menu == Some(LookAnchor::Stage);
        let look = button(
            "rail-look",
            "Lyric style",
            glyph("\u{E8D2}", 14.0),
            state(look_open),
            false,
            Box::new(cx.listener(|this, _, _, cx| {
                if this.look_menu_closed.elapsed() > Duration::from_millis(300) {
                    this.look_menu = if this.look_menu == Some(LookAnchor::Stage) { None } else { Some(LookAnchor::Stage) };
                }
                cx.notify();
            })),
        );
        let mut rail = div().flex().flex_col().items_center().gap(px(6.0)).child(self.stage_volume(cx)).child(dot()).child(layout).child(look);

        if matches!(self.lyrics, Lyrics::Ready(_)) {
            let romaji = button(
                "rail-romaji",
                match self.show_romaji {
                    true => "Hide romaji",
                    false => "Romaji",
                },
                div().font_family(DISPLAY).text_size(px(15.0)).font_weight(FontWeight::SEMIBOLD).child("あ").into_any_element(),
                toggled(self.show_romaji),
                self.extras.romaji_pending,
                Box::new(cx.listener(|this, _, _, cx| {
                    // On but with nothing to show (it failed): a click tries again rather than switching off.
                    let failed = this.show_romaji && this.extras.romaji.is_none() && !this.extras.romaji_pending;
                    if !failed {
                        this.show_romaji = !this.show_romaji;
                        settings::set("lyrics_romaji", if this.show_romaji { "1" } else { "0" });
                    }
                    this.fetch_extras(cx);
                    cx.notify();
                })),
            );
            rail = rail.child(romaji);
        }
        let mini = button(
            "rail-mini",
            "Mini player",
            glyph("\u{E8A7}", 14.0),
            toggled(self.mini.is_some()),
            false,
            Box::new(cx.listener(|this, _, _, cx| this.open_mini_player(cx))),
        );
        let desk = button(
            "rail-desk",
            match self.desk_lyrics.is_some() {
                true => "Hide lyrics on the desktop",
                false => "Lyrics on the desktop",
            },
            glyph("\u{E7F4}", 14.0),
            toggled(self.desk_lyrics.is_some()),
            false,
            Box::new(cx.listener(|this, _, _, cx| this.toggle_desktop_lyrics(cx))),
        );
        rail = rail.child(dot()).child(mini).child(desk);

        // A menu sits left of the rail, centred on the window's height.
        let beside = |card: AnyElement| deferred(div().absolute().top_0().bottom_0().right(px(52.0)).flex().items_center().child(card)).with_priority(1);
        div()
            .absolute()
            .top(px(52.0))
            .bottom_0()
            .right(px(RAIL_EDGE))
            .flex()
            .flex_col()
            .justify_center()
            .child(
                div().relative().child(rail).when_some(self.extras.note.clone(), |d, note| {
                    d.child(
                        div()
                            .absolute()
                            .top(relative(1.0))
                            .right_0()
                            .pt_2()
                            .w(px(130.0))
                            .text_right()
                            .text_size(px(10.0))
                            .line_height(px(13.0))
                            .text_color(rgba(SECONDARY))
                            .child(note),
                    )
                }),
            )
            .when(self.layout_menu, |d| d.child(beside(self.layout_menu_card(cx).into_any_element())))
            .when(look_open, |d| d.child(beside(self.look_menu_card(cx).into_any_element())))
            .into_any_element()
    }

    /// The full-screen player's volume, at the head of the rail: a speaker like the rail's other
    /// symbols (the wheel over it nudges the volume); a click slides the slider out to its
    /// left, with the level and a mute switch.
    pub(crate) fn stage_volume(&self, cx: &mut Context<Self>) -> gpui::Div {
        let open = self.stage_volume_open;
        let wheel = |this: &mut Self, event: &gpui::ScrollWheelEvent, cx: &mut Context<Self>| {
            let dy = f32::from(event.delta.pixel_delta(px(20.0)).y);
            if dy != 0.0 {
                this.set_volume(this.volume + if dy > 0.0 { 0.05 } else { -0.05 });
                cx.notify();
            }
        };
        let button = div()
            .id("s-volume")
            .size(px(34.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .font_family(ICONS)
            .text_size(px(14.0))
            .map(|d| match open {
                true => d.bg(rgba(0xffffff1f)).text_color(rgb(0xffffff)),
                false => d.text_color(rgba(0xffffff8c)).hover(|s| s.bg(rgba(0xffffff14)).text_color(rgb(0xffffff))),
            })
            .child(volume_glyph(self.muted, self.volume))
            .on_click(cx.listener(|this, _, _, cx| {
                // The mouse-down that closed it is this click's own: leave it closed.
                if this.stage_volume_closed.elapsed() > Duration::from_millis(300) {
                    this.stage_volume_open = !this.stage_volume_open;
                }
                cx.notify();
            }))
            .on_scroll_wheel(cx.listener(move |this, event: &gpui::ScrollWheelEvent, _, cx| wheel(this, event, cx)));
        let level = if self.muted { 0 } else { (self.volume * 100.0).round() as u32 };
        let pop = open.then(|| {
            deferred(
                div().absolute().right(px(44.0)).top(px(-5.0)).child(
                        div()
                            .id("stage-volume-pop")
                            .h(px(44.0))
                            .pl(px(6.0))
                            .pr(px(14.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .rounded_full()
                            .bg(rgba(GLASS))
                            .backdrop_blur(px(24.0))
                            .border_1()
                            .border_color(rgba(STROKE))
                            .shadow_lg()
                            .occlude()
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.stage_volume_open = false;
                                this.stage_volume_closed = Instant::now();
                                cx.notify();
                            }))
                            .on_scroll_wheel(cx.listener(move |this, event: &gpui::ScrollWheelEvent, _, cx| wheel(this, event, cx)))
                            .child(action("s-mute", volume_glyph(self.muted, self.volume), self.muted).on_click(cx.listener(|this, _, _, cx| {
                                this.muted = !this.muted;
                                this.audio.mute(this.muted);
                                cx.notify();
                            })))
                            .child(self.volume_slider(true, cx))
                            .child(div().w(px(28.0)).text_size(px(12.0)).text_color(rgba(SECONDARY)).child(format!("{level}")))
                            // Slides out of the rail as it fades in.
                            .with_animation("stage-volume-in", gpui::Animation::new(Duration::from_millis(220)).with_easing(gpui::ease_out_quint()), |d, k| {
                                d.opacity(k).mr(px(-10.0 * (1.0 - k)))
                            }),
                ),
            )
            .with_priority(1)
        });
        div().relative().child(button).children(pop)
    }

    // ---- Full-screen player ------------------------------------------------------------

    /// The full-screen player's background, as chosen (the full-bleed layout always shows
    /// the cover itself).
    pub(crate) fn stage_backdrop(&self, layout: StageLayout) -> AnyElement {
        let fill = |image: Arc<RenderImage>| div().absolute().inset_0().child(img(image).object_fit(ObjectFit::Cover).size_full());
        let mode = if layout == StageLayout::Bleed { StageBackdrop::Cover } else { self.stage_backdrop };
        let mut back = div().absolute().inset_0().bg(rgb(0x0b0b0b));
        match mode {
            StageBackdrop::Ambient => {
                if let Some(previous) = self.previous_ambient.clone() {
                    back = back.child(fill(previous));
                }
                if let Some(current) = self.ambient.clone() {
                    back = back.child(fill(current));
                }
                back = back.child(div().absolute().inset_0().bg(rgba(0x0000_0026)));
            }
            StageBackdrop::SoftCover => {
                if let Some(art) = &self.stage_art {
                    back = back.child(fill(art.soft.clone()));
                }
                back = back.child(div().absolute().inset_0().bg(rgba(0x0000_0033)));
            }
            StageBackdrop::Cover => {
                if let Some(cover) = self.cover_large.clone() {
                    back = back.child(fill(cover).when(!self.cover_sharp, |d| d.blur(px(28.0))));
                }
                // Darker towards the bottom and the left, where the text sits.
                back = back
                    .child(div().absolute().inset_0().bg(rgba(0x0000_0059)))
                    .child(div().absolute().inset_0().bg(gpui::linear_gradient(
                        180.0,
                        gpui::linear_color_stop(gpui::hsla(0.0, 0.0, 0.0, 0.0), 0.35),
                        gpui::linear_color_stop(gpui::hsla(0.0, 0.0, 0.0, 0.85), 1.0),
                    )))
                    .child(div().absolute().inset_0().bg(gpui::linear_gradient(
                        90.0,
                        gpui::linear_color_stop(gpui::hsla(0.0, 0.0, 0.0, 0.55), 0.0),
                        gpui::linear_color_stop(gpui::hsla(0.0, 0.0, 0.0, 0.0), 0.6),
                    )));
            }
            StageBackdrop::Gradient => {
                let [a, b] = self.stage_art.as_ref().map_or([0x202020, 0x0b0b0b], |art| art.colors);
                back = back.bg(gpui::linear_gradient(135.0, gpui::linear_color_stop(rgb(a), 0.0), gpui::linear_color_stop(rgb(b), 1.0)));
            }
            StageBackdrop::Dark => {}
        }
        back.into_any_element()
    }

    /// Cover, title and artist, seek line and transport, stacked to `width`.
    pub(crate) fn stage_info(&mut self, track: &Track, cover: Option<gpui_ce_motion::Frame>, width: f32, centred: bool, cx: &mut Context<Self>) -> gpui::Div {
        let transport = self.transport(true, cx);
        let seek = self.seek_line("stage-seek", self.stage_seek.clone(), (width - 96.0).max(160.0), cx);
        let title_size = (width * 0.07).clamp(22.0, 32.0);
        let mut column = div().w(px(width)).flex_none().flex().flex_col().gap_4().when(centred, |d| d.items_center());
        if let Some(frame) = cover {
            column = column.child(frame.apply(self.cover_art(self.cover_large.clone(), width, (width * 0.035).max(10.0)).shadow_lg()));
        }
        column
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(centred, |d| d.items_center().text_center())
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_3()
                            .when(centred, |d| d.justify_center())
                            .child(div().min_w_0().font_family(DISPLAY).text_size(px(title_size)).font_weight(FontWeight::BOLD).truncate().child(track.title.clone()))
                            .child(self.heart_button("stage-heart", (title_size * 0.55).clamp(14.0, 18.0), cx)),
                    )
                    .child(div().max_w_full().flex().when(centred, |d| d.justify_center()).child(self.artist_link("stage-artist", track, title_size * 0.6, cx).text_color(rgba(0xffffffb3)))),
            )
            .child(seek)
            .child(div().w_full().flex().justify_center().child(transport))
    }

    /// Renders either the spinning vinyl disc (when `vinyl_mode` is on) or the standard rounded cover.
    pub(crate) fn stage_cover(&mut self, size: f32, radius: f32, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        if self.vinyl_mode {
            div().child(self.vinyl_disc(size, window, cx))
        } else {
            self.cover_art(self.cover_large.clone(), size, radius).shadow_lg()
        }
    }

    /// A rotating vinyl record with concentric grooves, circular album label,
    /// and a frosted metallic glass center spindle hole.
    pub(crate) fn vinyl_disc(&mut self, size: f32, window: &mut Window, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        if self.playing && !self.loading {
            window.request_animation_frame();
        }

        // Gentle beat pulsing inside a fixed-size wrapper so text/layout never jitters
        let scope = self.audio.scope();
        let beat = if self.playing { scope.beat() } else { 0.0 };
        let level = if self.playing { scope.level() } else { 0.0 };
        let pulse_scale = 1.0 + (beat * 0.015 + level * 0.008);
        let actual_size = size * pulse_scale;

        // Smooth continuous visual angle via high-precision delta-time clock (native 60/75/120/144 FPS)
        let (prev_angle, last_instant) = self.vinyl_state.get();
        let now = std::time::Instant::now();
        let angle = if self.vinyl_rotate && self.playing && !self.loading {
            let dt = (now - last_instant).as_secs_f32().clamp(0.0, 0.05);
            let speed = self.vinyl_speed.max(0.5);
            let next_angle = (prev_angle + (dt / speed) * 360.0) % 360.0;
            self.vinyl_state.set((next_angle, now));
            next_angle
        } else {
            self.vinyl_state.set((prev_angle, now));
            prev_angle
        };

        // Real-time per-frame fixed-point bilinear rotation at exact floating-point angle (~0.15 ms, ~0.5 MB RAM)
        let rotated_frame = if self.vinyl_rotate {
            if let Some(base) = self.vinyl_base.clone() {
                let needs_new = match &self.vinyl_frame {
                    Some((last_angle, _)) => (angle - *last_angle).abs() >= 0.02,
                    None => true,
                };
                if needs_new {
                    let new_img = ambient::vinyl_frame_at(&base, angle);
                    if let Some(old_prev) = self.vinyl_prev_frame.take() {
                        cx.drop_image(old_prev, Some(window));
                    }
                    if let Some((_, cur)) = self.vinyl_frame.replace((angle, new_img.clone())) {
                        self.vinyl_prev_frame = Some(cur);
                    }
                    Some(new_img)
                } else {
                    self.vinyl_frame.as_ref().map(|(_, img)| img.clone())
                }
            } else {
                None
            }
        } else {
            None
        };

        // Thin sleek vinyl rim matching user reference (cover artwork fills ~96.5% of disc diameter)
        let label_size = actual_size * 0.965;
        let spindle_size = actual_size * 0.17;
        let hole_size = spindle_size * 0.28;

        let disc = div()
            .relative()
            .size(px(actual_size))
            .flex_none()
            .rounded_full()
            .bg(rgb(0x0e0e12))
            .shadow_2xl()
            .border_2()
            .border_color(rgba(0x22222bff))
            // Thin concentric micro-groove rim
            .child(
                div()
                    .absolute()
                    .inset(px(actual_size * 0.006))
                    .rounded_full()
                    .border_1()
                    .border_color(rgba(0xffffff18)),
            )
            // Circular album art label
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(label_size))
                            .rounded_full()
                            .overflow_hidden()
                            .border_1()
                            .border_color(rgba(0x0000_0066))
                            .shadow_md()
                            .map(|d| {
                                if let Some(frame_img) = rotated_frame {
                                    return d.child(
                                        img(frame_img)
                                            .object_fit(ObjectFit::Cover)
                                            .size_full()
                                            .rounded_full(),
                                    );
                                }
                                match self.cover_large.clone() {
                                    Some(image) => d.child(
                                        img(image)
                                            .object_fit(ObjectFit::Cover)
                                            .size_full()
                                            .rounded_full()
                                            .when(!self.cover_sharp, |s| s.blur(px((size * 0.03).clamp(3.0, 14.0)))),
                                    ),
                                    None => d.bg(rgba(SUBTLE_STRONG)).child(
                                        div()
                                            .size_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .font_family(ICONS)
                                            .text_size(px(label_size * 0.3))
                                            .text_color(rgba(SECONDARY))
                                            .child("\u{E8D6}"),
                                    ),
                                }
                            }),
                    ),
            )
            // Specular sheen light reflections (dual opposing cones)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .bg(gpui::linear_gradient(
                        angle,
                        gpui::linear_color_stop(rgba(0xffffff22), 0.0),
                        gpui::linear_color_stop(rgba(0xffffff00), 0.38),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .bg(gpui::linear_gradient(
                        (angle + 180.0) % 360.0,
                        gpui::linear_color_stop(rgba(0xffffff22), 0.0),
                        gpui::linear_color_stop(rgba(0xffffff00), 0.38),
                    )),
            )
            // Frosted metallic glass spindle center (matching YT Music Vinyl & FocusGrow)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(spindle_size))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgba(0xe8ebf2f0))
                            .backdrop_blur(px(16.0))
                            .border_2()
                            .border_color(rgba(0xffffffdd))
                            .shadow_lg()
                            .child(
                                div()
                                    .size(px(hole_size))
                                    .rounded_full()
                                    .bg(rgb(0x0a0a0d))
                                    .border_1()
                                    .border_color(rgba(0x33333388))
                                    .shadow_md(),
                            ),
                    ),
            );

        div()
            .id("vinyl-disc")
            .size(px(size))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle(cx);
            }))
            .child(disc)
    }

    /// Up Next mini-preview (next 1-2 tracks) for fullscreen stage view.
    pub(crate) fn stage_up_next(&self, width: f32, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let cur = self.index?;
        let upcoming: Vec<(usize, Track)> = self
            .queue
            .iter()
            .enumerate()
            .skip(cur + 1)
            .take(2)
            .map(|(i, t)| (i, t.clone()))
            .collect();

        if upcoming.is_empty() {
            return None;
        }

        let mut container = div()
            .w(px(width))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded_xl()
            .bg(rgba(0x0000_0033))
            .border_1()
            .border_color(rgba(0xffffff14))
            .backdrop_blur(px(20.0))
            .shadow_md();

        let header = div()
            .px_1()
            .pb_1()
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(10.0))
            .font_weight(FontWeight::BOLD)
            .text_color(rgba(0xffffff88))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().font_family(ICONS).text_size(px(12.0)).child("\u{E8FD}"))
                    .child(t("Up Next").to_uppercase()),
            )
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(rgba(0xffffff55))
                    .child(format!("{}/{}", cur + 1, self.queue.len())),
            );

        container = container.child(header);

        for (idx, track) in upcoming {
            let art = self.art.get(&track.video_id).map(|a| a.small.clone());
            let item = div()
                .id(("stage-next", idx))
                .h(px(40.0))
                .w_full()
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded_lg()
                .cursor_pointer()
                .hover(|s| s.bg(rgba(0xffffff1a)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.index = Some(idx);
                    this.playing = true;
                    this.start(cx);
                }))
                .child(
                    div()
                        .size(px(28.0))
                        .flex_none()
                        .rounded_md()
                        .overflow_hidden()
                        .bg(rgba(SUBTLE_STRONG))
                        .map(|d| match art {
                            Some(image) => d.child(img(image).size_full().object_fit(ObjectFit::Cover)),
                            None => d.flex().items_center().justify_center().font_family(ICONS).text_size(px(12.0)).text_color(rgba(SECONDARY)).child("\u{E8D6}"),
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .text_color(rgb(0xffffff))
                                .child(track.title.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(rgba(0xffffff99))
                                .truncate()
                                .child(track.artist.clone()),
                        ),
                );

            container = container.child(item);
        }

        Some(container)
    }

    /// The whole window for the song, in the chosen layout over the chosen background.
    /// Every size follows the window, so a maximised window fills out rather than leaving
    /// a small player in one corner.
    pub(crate) fn stage(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.full_screen {
            return None;
        }
        // On its way out: down and away, quicker at the end, the page showing through.
        const CLOSE: Duration = Duration::from_millis(320);
        if self.stage_closing.is_some() && !self.stage_close_drawn {
            self.stage_close_drawn = true;
            self.stage_closing = Some(Instant::now());
        }
        let leaving = match self.stage_closing {
            Some(at) if at.elapsed() >= CLOSE => {
                self.stage_closing = None;
                self.full_screen = false;
                self.lyric_motion.scroll = MotionState::default();
                return None;
            }
            Some(at) => {
                window.request_animation_frame();
                let k = at.elapsed().as_secs_f32() / CLOSE.as_secs_f32();
                // Ease in: it lets go at once, then gathers speed.
                Some(k * k)
            }
            None => None,
        };
        let Some(track) = self.current().cloned() else {
            self.full_screen = false;
            self.stage_closing = None;
            return None;
        };
        let enter = self.stage_motion.frame(
            Motion::new(Target::default().y(0.0).opacity(1.0))
                .initial(Target::default().y(28.0).opacity(0.0))
                .transition(Transition::Spring(self.spring)),
            window,
            cx,
        );
        let cover_frame = self.stage_cover_motion.frame(
            Motion::new(Target::default().y(0.0).opacity(1.0))
                .initial(Target::default().y(18.0).opacity(0.0))
                .transition(Transition::Spring(self.spring)),
            window,
            cx,
        );
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        let area = (vh - 52.0 - 40.0).max(240.0);
        let rail = self.stage_rail(cx);
        let layout = self.stage_layout;
        let backdrop = self.stage_backdrop(layout);

        let caption = |id: &'static str, glyph: &'static str, area: WindowControlArea, close: bool| {
            div()
                .id(id)
                .w(px(46.0))
                .h(px(32.0))
                .flex()
                .items_center()
                .justify_center()
                .font_family(ICONS)
                .text_size(px(10.0))
                .window_control_area(area)
                .hover(move |s| if close { s.bg(rgb(0xc42b1c)) } else { s.bg(rgba(SUBTLE_STRONG)) })
                .child(glyph)
        };
        // The name, quietly, over the middle: a plain "Naby" and a serif italic "Tune", half-lit,
        // part of the strip the window is dragged by. Words only: no mark.
        let wordmark = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(52.0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("stage-wordmark")
                    .h(px(32.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .opacity(0.62)
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .text_size(px(15.0))
                            .child(div().font_family(DISPLAY).font_weight(FontWeight::SEMIBOLD).child("Naby"))
                            .child(div().font_family("Georgia").italic().text_size(px(16.0)).text_color(rgba(0xffffffd9)).child("Tune")),
                    ),
            );
        let top = div()
            .relative()
            .flex()
            .items_center()
            .h(px(52.0))
            .pl_3()
            .child(wordmark)
            .child(action("stage-close", "\u{E70D}", false).on_click(cx.listener(|this, _, _, cx| {
                this.set_full_screen(false);
                cx.notify();
            })))
            .child(div().id("stage-drag").flex_1().h_full().window_control_area(WindowControlArea::Drag))
            .child(
                div()
                    .flex()
                    .self_start()
                    .child(caption("s-min", "\u{E921}", WindowControlArea::Min, false))
                    .child(caption("s-max", "\u{E922}", WindowControlArea::Max, false))
                    .child(caption("s-close", "\u{E8BB}", WindowControlArea::Close, true)),
            );

        let lyric_style = |text: f32, anchor: f32| LyricStyle {
            text,
            line_height: text * 1.3,
            gap: text * 0.62,
            pad: 8.0,
            anchor,
            blur_step: 1.3,
            blur_max: 4.5,
        };
        let subtitle = LyricStyle { text: 19.0, line_height: 26.0, gap: 0.0, pad: 0.0, anchor: 0.5, blur_step: 0.0, blur_max: 0.0 };

        let body: AnyElement = match layout {
            // Cover (or spinning vinyl) and controls beside the lyrics, the pair centred as one group.
            StageLayout::Side => {
                let size = (area * 0.62).min(vw * 0.3).clamp(240.0, 640.0);
                let gap = (vw * 0.05).clamp(40.0, 110.0);
                let room = ((vw - 64.0 - 64.0 - RAIL_ROOM).min(1500.0) - size - gap).max(200.0);
                let column = room.min(size * 1.45).min(860.0);
                let cover = cover_frame.apply(self.stage_cover(size, (size * 0.035).max(10.0), window, cx));
                let info = self.stage_info(&track, None, size, self.vinyl_mode, cx);
                let lyrics = self.lyric_column(&lyric_style((vh * 0.04).min(column * 0.058).clamp(20.0, 48.0), 0.42), window, cx);
                let left_col = div()
                    .w(px(size))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .when(self.vinyl_mode, |d| d.items_center())
                    .gap_4()
                    .child(cover)
                    .child(info);
                div()
                    .size_full()
                    .flex()
                    .justify_center()
                    .pl(px(64.0))
                    .pr(px(64.0 + RAIL_ROOM - 40.0))
                    .pb(px(40.0))
                    .child(
                        div()
                            .h_full()
                            .flex()
                            .items_center()
                            .gap(px(gap))
                            .child(left_col)
                            .child(div().w(px(column)).flex_none().h_full().py(px(24.0)).child(lyrics)),
                    )
                    .into_any_element()
            }
            // Side by side + Up Next preview card below the controls (supports both Cover and Vinyl mode).
            StageLayout::UpNext => {
                let size = (area * 0.60).min(vw * 0.32).clamp(240.0, 560.0);
                let gap = (vw * 0.05).clamp(40.0, 110.0);
                let room = ((vw - 64.0 - 64.0 - RAIL_ROOM).min(1500.0) - size - gap).max(200.0);
                let column = room.min(size * 1.45).min(860.0);
                let cover = cover_frame.apply(self.stage_cover(size, (size * 0.035).max(10.0), window, cx));
                let info = self.stage_info(&track, None, size, true, cx);
                let lyrics = self.lyric_column(&lyric_style((vh * 0.04).min(column * 0.058).clamp(20.0, 48.0), 0.42), window, cx);
                let up_next = self.stage_up_next(size, cx);
                let mut left_col = div()
                    .w(px(size))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child(cover)
                    .child(info);
                if let Some(next_card) = up_next {
                    left_col = left_col.child(next_card);
                }
                div()
                    .size_full()
                    .flex()
                    .justify_center()
                    .pl(px(64.0))
                    .pr(px(64.0 + RAIL_ROOM - 40.0))
                    .pb(px(40.0))
                    .child(
                        div()
                            .h_full()
                            .flex()
                            .items_center()
                            .gap(px(gap))
                            .child(left_col)
                            .child(div().w(px(column)).flex_none().h_full().py(px(24.0)).child(lyrics)),
                    )
                    .into_any_element()
            }
            // Everything on one axis: the cover (or vinyl), the title, the sung line as a subtitle, controls.
            StageLayout::Center => {
                let size = (area * 0.5).min(vw * 0.4).clamp(220.0, 600.0);
                let cover = cover_frame.apply(self.stage_cover(size, (size * 0.035).max(10.0), window, cx));
                // The subtitle reads from across the room: a size up from the card's.
                let line = self.subtitle_line(&LyricStyle { text: 24.0, line_height: 32.0, ..subtitle }, window, cx);
                let info = self.stage_info(&track, None, size.max(360.0), true, cx);
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_4()
                    .pb(px(40.0))
                    .child(cover)
                    .child(div().w(px((vw * 0.7).min(1000.0))).h(px(116.0)).child(line))
                    .child(info)
                    .into_any_element()
            }
            // The cover is the room: a big title bottom-left (with floating vinyl when Vinyl mode is on), the lyrics up the right.
            StageLayout::Bleed => {
                let title = (vh * 0.065).clamp(34.0, 76.0);
                let seek = self.seek_line("stage-seek", self.stage_seek.clone(), (vw * 0.28).clamp(260.0, 520.0), cx);
                let transport = self.transport(true, cx);
                let lyrics_width = vw * 0.42 - RAIL_ROOM * 0.5;
                let lyrics = self.lyric_column(&lyric_style((vh * 0.032).min(lyrics_width * 0.06).clamp(20.0, 40.0), 0.45), window, cx);
                let bleed_vinyl = self.vinyl_mode.then(|| {
                    let vsize = (area * 0.42).min(vw * 0.24).clamp(180.0, 340.0);
                    cover_frame.apply(self.vinyl_disc(vsize, window, cx))
                });
                div()
                    .size_full()
                    .flex()
                    .gap(px(64.0))
                    .pl(px(64.0))
                    .pr(px(RAIL_ROOM + 12.0))
                    .pb(px(48.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .justify_end()
                            .items_center()
                            .gap_3()
                            .children(bleed_vinyl.map(|v| div().pb_3().child(v)))
                            .child(
                                div()
                                    .text_center()
                                    .font_family(DISPLAY)
                                    .text_size(px(title))
                                    .line_height(px(title * 1.08))
                                    .font_weight(FontWeight::BLACK)
                                    .child(track.title.clone()),
                            )
                            .child(div().text_center().text_size(px(title * 0.36)).text_color(rgba(0xffffffcc)).child(track.artist.clone()))
                            .child(div().pt_3().child(seek))
                            .child(transport),
                    )
                    .child(div().w(px(lyrics_width)).flex_none().h_full().py(px(24.0)).child(lyrics))
                    .into_any_element()
            }
            // The words are the show: lyrics large across the middle, the song small below.
            StageLayout::Lyrics => {
                let lyrics_width = (vw * 0.74).min(1150.0).min(vw - 2.0 * RAIL_ROOM - 24.0);
                let lyrics = self.lyric_column(&lyric_style((vh * 0.054).min(lyrics_width * 0.06).clamp(24.0, 66.0), 0.45), window, cx);
                let seek = self.seek_line("stage-seek", self.stage_seek.clone(), (vw * 0.26).clamp(240.0, 480.0), cx);
                let transport = self.transport(true, cx);
                let mini_cover: AnyElement = if self.vinyl_mode {
                    self.vinyl_disc(56.0, window, cx).into_any_element()
                } else {
                    self.cover_art(self.cover.clone(), 56.0, 10.0).shadow_md().into_any_element()
                };
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .justify_center()
                            .child(div().w(px(lyrics_width)).h_full().child(lyrics)),
                    )
                    .child(
                        div()
                            .h(px(108.0))
                            .px(px(48.0))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(mini_cover)
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .child(div().text_size(px(15.0)).font_weight(FontWeight::SEMIBOLD).truncate().child(track.title.clone()))
                                            .child(div().text_size(px(13.0)).text_color(rgba(SECONDARY)).truncate().child(track.artist.clone())),
                                    ),
                            )
                            .child(div().flex().flex_col().items_center().gap_1().child(transport).child(seek))
                            .child(div().flex_1()),
                    )
                    .into_any_element()
            }
            // A glass card floating in the middle: cover (or vinyl), the sung line, controls.
            StageLayout::Card => {
                let size = (area * 0.5).min(vw * 0.3).clamp(300.0, 420.0);
                let cover = cover_frame.apply(self.stage_cover(size, 14.0, window, cx));
                let line = self.subtitle_line(&LyricStyle { text: 16.0, ..subtitle }, window, cx);
                let info = self.stage_info(&track, None, size, true, cx);
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .pb(px(40.0))
                    .child(
                        div()
                            .p(px(24.0))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_3()
                            .rounded(px(28.0))
                            .bg(rgba(0x1414_1466))
                            .backdrop_blur(px(40.0))
                            .border_1()
                            .border_color(rgba(0xffffff1a))
                            .shadow_lg()
                            .child(cover)
                            .child(div().w(px(size)).h(px(76.0)).child(line))
                            .child(info),
                    )
                    .into_any_element()
            }
        };

        let drop = leaving.map_or(0.0, |k| k * vh * 0.75);
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(drop))
                .h(px(vh))
                .when_some(leaving, |d, k| d.opacity(1.0 - k * 0.85))
                // Over the whole window: clicks stop here instead of reaching the page and dock beneath.
                .occlude()
                .child(
                    enter.apply(
                        div()
                            .size_full()
                            .flex()
                            .flex_col()
                            .child(backdrop)
                            .children(self.visualizer(window))
                            .child(top)
                            .child(div().flex_1().min_h_0().child(body))
                            .child(rail),
                    ),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn layout_menu_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let header = |text: &'static str| {
            div().px(px(10.0)).pt(px(6.0)).pb(px(4.0)).text_size(px(11.0)).font_weight(FontWeight::SEMIBOLD).text_color(rgba(TERTIARY)).child(text)
        };
        let item = |id: (&'static str, usize), name: &'static str, selected: bool| {
            div()
                .id(id)
                .h(px(32.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .justify_between()
                .rounded(px(8.0))
                .cursor_pointer()
                .text_size(px(13.0))
                .text_color(if selected { rgb(0xffffff) } else { rgba(0xffffffcc) })
                .when(selected, |d| d.bg(rgba(SUBTLE)).font_weight(FontWeight::SEMIBOLD))
                .hover(|s| s.bg(rgba(SUBTLE)))
                .child(name)
                .child(div().size(px(6.0)).rounded_full().when(selected, |d| d.bg(rgb(accent()))))
        };
        let mut card = div()
            .id("layout-menu")
            .w(px(220.0))
            .max_h(px(640.0))
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
                this.layout_menu = false;
                this.layout_menu_closed = Instant::now();
                cx.notify();
            }))
            .child(header(t("LAYOUT")));
        for (n, layout) in StageLayout::ALL.into_iter().enumerate() {
            card = card.child(item(("layout", n), layout.name(), layout == self.stage_layout).on_click(cx.listener(move |this, _, _, cx| {
                this.stage_layout = layout;
                settings::set("stage_layout", layout.key());
                // A new column measures itself afresh.
                this.lyric_motion.scroll = MotionState::default();
                this.stage_cover_motion = MotionState::default();
                cx.notify();
            })));
        }
        let fixed = self.stage_layout == StageLayout::Bleed;
        card = card.child(div().my(px(4.0)).mx(px(6.0)).h(px(1.0)).bg(rgba(STROKE))).child(header(t("BACKGROUND")));
        let mut backgrounds = div().flex().flex_col().gap(px(1.0)).when(fixed, |d| d.opacity(0.4));
        for (n, mode) in StageBackdrop::ALL.into_iter().enumerate() {
            backgrounds = backgrounds.child(item(("backdrop", n), mode.name(), mode == self.stage_backdrop).on_click(cx.listener(move |this, _, _, cx| {
                this.stage_backdrop = mode;
                settings::set("stage_backdrop", mode.key());
                cx.notify();
            })));
        }
        let mut card = card.child(backgrounds).when(fixed, |d| {
            d.child(div().px(px(10.0)).py(px(4.0)).text_size(px(11.0)).text_color(rgba(TERTIARY)).child(t("Full cover always uses the cover")))
        });
        card = card.child(div().my(px(4.0)).mx(px(6.0)).h(px(1.0)).bg(rgba(STROKE))).child(header(t("MOVES WITH THE MUSIC")));
        for (n, visual) in visualizer::Visual::ALL.into_iter().enumerate() {
            card = card.child(item(("visual", n), visual.name(), visual == self.visual).on_click(cx.listener(move |this, _, _, cx| {
                this.visual = visual;
                settings::set("visualizer", visual.key());
                cx.notify();
            })));
        }
        card = card
            .child(div().my(px(4.0)).mx(px(6.0)).h(px(1.0)).bg(rgba(STROKE)))
            .child(header(t("VINYL")))
            .child(item(("layout-vinyl-mode", 0), t("Vinyl disc"), self.vinyl_mode).on_click(cx.listener(|this, _, _, cx| {
                this.vinyl_mode = !this.vinyl_mode;
                settings::set("vinyl_mode", if this.vinyl_mode { "1" } else { "0" });
                cx.notify();
            })));
        if self.vinyl_mode {
            card = card
                .child(item(("layout-vinyl-spin", 0), t("Rotate record"), self.vinyl_rotate).on_click(cx.listener(|this, _, _, cx| {
                    this.vinyl_rotate = !this.vinyl_rotate;
                    settings::set("vinyl_rotate", if this.vinyl_rotate { "1" } else { "0" });
                    cx.notify();
                })))
                .child(header(t("ROTATION SPEED")))
                .child(self.vinyl_speed_slider(208.0, cx));
        }
        card
    }

    /// Interactive continuous slider for vinyl record spin speed (from fast 1.8s 33⅓ RPM to very slow 45s).
    pub(crate) fn vinyl_speed_slider(&self, width: f32, cx: &mut Context<Self>) -> gpui::Div {
        let bar = self.vinyl_speed_bar.clone();
        let (measure, cell, cell_drag) = (bar.clone(), bar.clone(), bar);
        let min_spd = 1.8f32;
        let max_spd = 45.0f32;
        let current = self.vinyl_speed.clamp(min_spd, max_spd);
        let progress = ((current - min_spd) / (max_spd - min_spd)).clamp(0.0, 1.0);
        let slider_w = (width - 12.0).max(80.0);

        let desc = if current <= 2.5 {
            "33⅓ RPM"
        } else if current <= 5.5 {
            "Cepat"
        } else if current <= 12.0 {
            "Sedang"
        } else if current <= 24.0 {
            "Lambat"
        } else {
            "Sangat Lambat"
        };

        let presets: [(f32, &'static str); 4] = [
            (1.8, "33⅓"),
            (8.0, "Sedang"),
            (20.0, "Lambat"),
            (35.0, "Santai"),
        ];

        let mut preset_row = div().flex().items_center().gap_1().w_full().pt_1();
        for (spd, lbl) in presets {
            let active = (self.vinyl_speed - spd).abs() < 1.0;
            preset_row = preset_row.child(
                div()
                    .id(("vspeed-preset", (spd * 10.0) as usize))
                    .flex_1()
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .text_size(px(10.5))
                    .font_weight(if active { FontWeight::BOLD } else { FontWeight::NORMAL })
                    .text_color(if active { rgb(0xffffff) } else { rgba(0xffffffaa) })
                    .bg(if active { rgba(0xffffff28) } else { rgba(SUBTLE) })
                    .hover(|s| s.bg(rgba(SUBTLE_STRONG)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.vinyl_speed = spd;
                        settings::set("vinyl_speed", &spd.to_string());
                        cx.notify();
                    }))
                    .child(lbl),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(width))
            .px(px(6.0))
            .py(px(4.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(11.5))
                    .child(div().font_weight(FontWeight::MEDIUM).text_color(rgb(0xffffff)).child(format!("{:.1} s", current)))
                    .child(div().text_size(px(10.5)).text_color(rgba(SECONDARY)).child(desc)),
            )
            .child(
                div()
                    .on_children_prepainted(move |bounds, _, _| {
                        if let Some(b) = bounds.first() {
                            measure.set((f32::from(b.origin.x), f32::from(b.size.width)));
                        }
                    })
                    .child(
                        div()
                            .id("vinyl-speed-bar")
                            .w(px(slider_w))
                            .h(px(20.0))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                    let (left, w) = cell.get();
                                    let frac = ((f32::from(event.position.x) - left) / w.max(1.0)).clamp(0.0, 1.0);
                                    let spd = ((min_spd + frac * (max_spd - min_spd)) * 2.0).round() / 2.0;
                                    let spd = spd.clamp(min_spd, max_spd);
                                    this.vinyl_speed = spd;
                                    settings::set("vinyl_speed", &spd.to_string());
                                    cx.notify();
                                }),
                            )
                            .on_mouse_move(cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                                if event.pressed_button == Some(gpui::MouseButton::Left) {
                                    let (left, w) = cell_drag.get();
                                    let frac = ((f32::from(event.position.x) - left) / w.max(1.0)).clamp(0.0, 1.0);
                                    let spd = ((min_spd + frac * (max_spd - min_spd)) * 2.0).round() / 2.0;
                                    let spd = spd.clamp(min_spd, max_spd);
                                    this.vinyl_speed = spd;
                                    settings::set("vinyl_speed", &spd.to_string());
                                    cx.notify();
                                }
                            }))
                            .on_scroll_wheel(cx.listener(move |this, event: &gpui::ScrollWheelEvent, _, cx| {
                                let dy = f32::from(event.delta.pixel_delta(px(20.0)).y);
                                if dy != 0.0 {
                                    let step = if dy > 0.0 { 1.0 } else { -1.0 };
                                    let spd = (this.vinyl_speed + step).clamp(min_spd, max_spd);
                                    this.vinyl_speed = spd;
                                    settings::set("vinyl_speed", &spd.to_string());
                                    cx.notify();
                                }
                            }))
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .h(px(5.0))
                                    .rounded_full()
                                    .bg(rgba(SUBTLE_STRONG))
                                    .child(div().h_full().rounded_full().bg(rgb(accent())).w(px(slider_w * progress)))
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(-4.5))
                                            .left(px((slider_w * progress - 7.0).clamp(-2.0, slider_w - 12.0)))
                                            .size(px(14.0))
                                            .rounded_full()
                                            .bg(rgb(0xffffff))
                                            .shadow_md(),
                                    ),
                            ),
                    ),
            )
            .child(preset_row)
    }
}
