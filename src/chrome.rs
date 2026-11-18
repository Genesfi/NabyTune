//! The window around the pages: the title bar, the dock with the transport, seek line and
//! volume, the side panel (Up next), and the small pieces they share.

use super::*;

impl NabyTune {
    /// The playing song's cover at `side` px: softly blurred while only the row's small
    /// artwork stands in (pixels blown up read as broken; a soft blur reads as loading).
    pub(crate) fn cover_art(&self, image: Option<Arc<RenderImage>>, side: f32, radius: f32) -> gpui::Div {
        artwork(image, side, radius).when(!self.cover_sharp, |d| d.blur(px((side * 0.03).clamp(3.0, 14.0))))
    }

    // ---- Title bar --------------------------------------------------------------------

    pub(crate) fn titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
        let drag = |id: &'static str| div().id(id).flex_1().min_w(px(16.0)).h_full().window_control_area(WindowControlArea::Drag);
        let tab = |id: &'static str, label: &'static str, this: Tab, current: Tab| {
            let active = this == current;
            div()
                .id(id)
                .relative()
                .h(px(32.0))
                .px(px(14.0))
                .flex()
                .items_center()
                .rounded(px(6.0))
                .cursor_pointer()
                .text_size(px(13.0))
                .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                .text_color(if active { rgb(0xffffff) } else { rgba(SECONDARY) })
                .hover(|s| s.bg(rgba(SUBTLE)).text_color(rgb(0xffffff)))
                .child(label)
                // The current page: a short accent pill under its name, as Windows 11's tabs.
                .when(active, |d| {
                    d.child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .flex()
                            .justify_center()
                            .child(div().w(px(16.0)).h(px(3.0)).rounded_full().bg(rgb(accent()))),
                    )
                })
        };
        // No Back button: the pages are tabs, and Home is always one click away.
        div()
            .flex()
            .items_center()
            .h(px(52.0))
            .gap_1()
            .pl(px(18.0))
            .child(
                div()
                    .id("brand")
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .pr_5()
                    .window_control_area(WindowControlArea::Drag)
                    .child(brand_mark(18.0))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .text_size(px(15.0))
                            .child(div().font_family(DISPLAY).font_weight(FontWeight::SEMIBOLD).child("Naby"))
                            .child(div().font_family("Georgia").italic().text_size(px(16.0)).text_color(rgba(0xffffffb3)).child("Tune")),
                    ),
            )
            .child(tab("tab-home", t("Home"), Tab::Home, self.tab).on_click(cx.listener(|this, _, _, cx| {
                this.tab = Tab::Home;
                cx.notify();
            })))
            .child(tab("tab-library", t("Library"), Tab::Library, self.tab).on_click(cx.listener(|this, _, _, cx| {
                this.tab = Tab::Library;
                this.library_motion.clear();
                cx.notify();
            })))
            .child(tab("tab-stats", t("Stats"), Tab::Stats, self.tab).on_click(cx.listener(|this, _, _, cx| this.open_stats(cx))))
            .when(!self.query.is_empty(), |d| {
                d.child(tab("tab-search", t("Search"), Tab::Search, self.tab).on_click(cx.listener(|this, _, _, cx| {
                    this.tab = Tab::Search;
                    cx.notify();
                })))
            })
            .child(drag("drag-left"))
            .child(self.search.clone())
            .child(drag("drag-right"))
            .child(
                // Settings sits with the window's own buttons, and looks like one of them.
                div()
                    .id("open-settings")
                    .self_start()
                    .w(px(46.0))
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .font_family(ICONS)
                    .text_size(px(13.0))
                    .when(self.tab == Tab::Settings, |d| d.bg(rgba(SUBTLE_STRONG)))
                    .hover(|s| s.bg(rgba(SUBTLE_STRONG)))
                    .child("\u{E713}")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab = if this.tab == Tab::Settings { Tab::Home } else { Tab::Settings };
                        this.settings_motion.clear();
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .self_start()
                    .child(caption("min", "\u{E921}", WindowControlArea::Min, false))
                    .child(caption("max", "\u{E922}", WindowControlArea::Max, false))
                    .child(caption("close", "\u{E8BB}", WindowControlArea::Close, true)),
            )
    }

    pub(crate) fn up_next_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(i) = self.index else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.0))
                .text_color(rgba(SECONDARY))
                .child(t("Queue is empty"))
                .into_any_element();
        };
        let mut list = div().id("up-next").flex_1().min_h_0().overflow_y_scroll().px_2().pb_3().flex().flex_col();
        let queue = self.queue.clone();
        for (j, track) in queue.iter().enumerate() {
            let here = j == i;
            let played = j < i;
            list = list.child(
                div()
                    .id(("next", j))
                    .h(px(52.0))
                    .flex_none()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .when(played, |d| d.opacity(0.65).hover(|s| s.opacity(1.0).bg(rgba(SUBTLE))))
                    .when(!played, |d| d.hover(|s| s.bg(rgba(SUBTLE))))
                    .when(here, |d| d.bg(rgba(SUBTLE_STRONG)))
                    .child(artwork(self.art.get(&track.video_id).map(|a| a.small.clone()), 36.0, 5.0))
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
                            .child(div().text_size(px(12.0)).text_color(rgba(SECONDARY)).truncate().child(track.artist.clone())),
                    )
                    // On hover: × takes it out of the queue (not the one playing).
                    .when(!here, |d| {
                        d.child(
                            div()
                                .id(("next-remove", j))
                                .size(px(26.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(6.0))
                                .font_family(ICONS)
                                .text_size(px(11.0))
                                .text_color(rgba(SECONDARY))
                                .opacity(0.0)
                                .group_hover(format!("next-{j}"), |s| s.opacity(1.0))
                                .hover(|s| s.bg(rgba(SUBTLE_STRONG)).text_color(rgb(0xffffff)))
                                .child("\u{E711}")
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.remove_from_queue(j, cx);
                                })),
                        )
                    })
                    .group(format!("next-{j}"))
                    .on_mouse_down(gpui::MouseButton::Right, {
                        let t = track.clone();
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            this.open_track_menu(t.clone(), event.position, Some(j));
                            cx.notify();
                        })
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.index = Some(j);
                        this.playing = true;
                        this.start(cx);
                    })),
            );
        }
        if self.autoplay_fetching.is_some() {
            list = list.child(
                div()
                    .px_3()
                    .py_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(12.0))
                    .text_color(rgba(TERTIARY))
                    .child(div().size(px(6.0)).rounded_full().bg(rgb(accent())))
                    .child(t("Finding similar vibes…")),
            );
        } else if !self.autoplay && self.repeat == Repeat::Off && self.queue.len().saturating_sub(i + 1) == 0 {
            list = list.child(
                div()
                    .id("up-next-enable-autoplay")
                    .mx_2()
                    .mt_3()
                    .p_3()
                    .rounded(px(10.0))
                    .bg(rgba(SUBTLE))
                    .border_1()
                    .border_color(rgba(STROKE))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(SUBTLE_STRONG)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_autoplay(cx)))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xffffff))
                            .child(t("Play similar vibes automatically")),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgba(SECONDARY))
                            .child(t("Turn on Autoplay to keep matching songs lined up next")),
                    ),
            );
        }

        let autoplay_on = self.autoplay;
        let loop_locked = self.repeat != Repeat::Off;
        let autoplay_active = autoplay_on && !loop_locked;
        let autoplay_toggle = div()
            .id("up-next-autoplay")
            .h(px(26.0))
            .px(px(10.0))
            .rounded_full()
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(6.0))
            .border_1()
            .border_color(if autoplay_active { rgb(accent()) } else { rgba(STROKE) })
            .bg(if autoplay_active { rgba(0xffffff18) } else { rgba(SUBTLE) })
            .when(autoplay_on && loop_locked, |d| d.opacity(0.6))
            .hover(|s| s.bg(rgba(SUBTLE_STRONG)))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_autoplay(cx)))
            .child(
                div()
                    .size(px(6.0))
                    .rounded_full()
                    .bg(if autoplay_active { rgb(accent()) } else { rgba(TERTIARY) }),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .font_weight(if autoplay_active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                    .text_color(if autoplay_active { rgb(0xffffff) } else { rgba(SECONDARY) })
                    .child(if autoplay_on && loop_locked { "Autoplay (Loop)" } else { t("Autoplay") }),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_4()
                    .pt_4()
                    .pb_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(16.0)).font_weight(FontWeight::SEMIBOLD).child(t("Up Next")))
                    .child(autoplay_toggle),
            )
            .children(self.radio_header(cx))
            .child(list)
            .into_any_element()
    }

    /// The side panel on glass, sliding in from the right edge of the page layer.
    pub(crate) fn side_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        // The full-screen player has the lyrics while it's up.
        if self.full_screen {
            return None;
        }
        let body = match self.panel {
            Panel::None => return None,
            Panel::Lyrics => {
                let toggle = self.look_button(LookAnchor::Panel, cx);
                let column = self.lyric_column(&PANEL_LYRICS, window, cx);
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_none()
                            .px_4()
                            .pt_4()
                            .pb_2()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_size(px(16.0)).font_weight(FontWeight::SEMIBOLD).child(t("Lyrics")))
                            .child(toggle),
                    )
                    .child(div().flex_1().min_h_0().child(column))
                    .into_any_element()
            }
            Panel::UpNext => self.up_next_panel(cx),
        };
        let slide = self.panel_motion.frame(
            Motion::new(Target::default().x(0.0).opacity(1.0))
                .initial(Target::default().x(28.0).opacity(0.0))
                .transition(Transition::Spring(self.spring)),
            window,
            cx,
        );
        Some(
            div()
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .w(px(PANEL))
                .occlude()
                .child(
                    slide.apply(
                        div()
                            .size_full()
                            .bg(rgba(GLASS))
                            .backdrop_blur(px(30.0))
                            .border_l_1()
                            .border_color(rgba(STROKE))
                            .child(body),
                    ),
                )
                .into_any_element(),
        )
    }

    // ---- Transport, seek line, dock ---------------------------------------------------

    /// Shuffle, previous, play, next, repeat. The dock's and the full-screen player's are the
    /// same row at two sizes; `stage` picks the latter's element ids and gesture state.
    pub(crate) fn transport(&self, stage: bool, cx: &mut Context<Self>) -> gpui::Div {
        let (play_id, prev_id, next_id, shuffle_id, repeat_id) = if stage {
            ("s-play", "s-prev", "s-next", "s-shuffle", "s-repeat")
        } else {
            ("play", "prev", "next", "shuffle", "repeat")
        };
        let gestures: [&GestureState; 3] = if stage {
            [&self.stage_gestures[0], &self.stage_gestures[1], &self.stage_gestures[2]]
        } else {
            [&self.play_gesture, &self.prev_gesture, &self.next_gesture]
        };
        let scale = if stage { 1.3 } else { 1.0 };
        let press = |state: &GestureState, id: &'static str, child: gpui::Stateful<gpui::Div>, rest: f32, hover: f32, down: f32| {
            state.bind(
                id,
                child,
                Motion::new(Target::default().width(rest * scale).height(rest * scale)).transition(Transition::Spring(self.spring)),
                GestureTargets::default()
                    .while_hover(Target::default().width(hover * scale).height(hover * scale))
                    .while_press(Target::default().width(down * scale).height(down * scale)),
            )
        };
        let round = |id: &'static str| div().id(id).rounded_full().flex().items_center().justify_center();
        let play_icon = if self.playing && self.index.is_some() {
            div()
                .flex()
                .gap(px(4.0 * scale))
                .child(div().w(px(4.0 * scale)).h(px(15.0 * scale)).rounded(px(1.5)).bg(rgb(on_accent())))
                .child(div().w(px(4.0 * scale)).h(px(15.0 * scale)).rounded(px(1.5)).bg(rgb(on_accent())))
        } else {
            icon("\u{F5B0}", 18.0 * scale).pl(px(2.0))
        };
        let play = press(
            gestures[0],
            if stage { "s-play-motion" } else { "play-motion" },
            round(play_id).bg(rgb(accent())).text_color(rgb(on_accent())).child(play_icon).on_click(cx.listener(|this, _, _, cx| this.toggle(cx))),
            44.0,
            48.0,
            40.0,
        );
        let previous = press(
            gestures[1],
            if stage { "s-prev-motion" } else { "prev-motion" },
            round(prev_id).hover(|s| s.bg(rgba(SUBTLE_STRONG))).child(icon("\u{E892}", 16.0 * scale)).on_click(cx.listener(|this, _, _, cx| this.skip(-1, cx))),
            36.0,
            40.0,
            32.0,
        );
        let next = press(
            gestures[2],
            if stage { "s-next-motion" } else { "next-motion" },
            round(next_id).hover(|s| s.bg(rgba(SUBTLE_STRONG))).child(icon("\u{E893}", 16.0 * scale)).on_click(cx.listener(|this, _, _, cx| this.skip(1, cx))),
            36.0,
            40.0,
            32.0,
        );
        let slot = |w: f32, child: AnyElement| div().size(px(w * scale)).flex().items_center().justify_center().child(child);
        let repeat_glyph = if self.repeat == Repeat::One { "\u{E8ED}" } else { "\u{E8EE}" };
        div()
            .flex()
            .items_center()
            .gap(px(8.0 * scale))
            .child(action(shuffle_id, "\u{E8B1}", self.shuffle).on_click(cx.listener(|this, _, _, cx| {
                this.shuffle = !this.shuffle;
                cx.notify();
            })))
            .child(slot(40.0, previous.into_any_element()))
            .child(slot(50.0, play.into_any_element()))
            .child(slot(40.0, next.into_any_element()))
            .child(action(repeat_id, repeat_glyph, self.repeat != Repeat::Off).on_click(cx.listener(|this, _, _, cx| {
                this.repeat = match this.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
                if this.repeat != Repeat::Off {
                    this.autoplay_fetching = None;
                }
                this.queue_changed(cx);
            })))
            // Full screen: the volume too, under the buttons (the dock is out of reach), as one
            // button; its slider shows once it's clicked.

    }

    /// Elapsed, the line (click to seek), remaining. `cell` keeps the line's bounds for the click.
    pub(crate) fn seek_line(&self, id: &'static str, cell: Rc<Cell<(f32, f32)>>, width: f32, cx: &mut Context<Self>) -> gpui::Div {
        let length = self.length;
        let now = self.now().min(length);
        let progress = if length.is_zero() { 0.0 } else { (now.as_secs_f32() / length.as_secs_f32()).clamp(0.0, 1.0) };
        let (elapsed, remaining) = if self.loading || length.is_zero() {
            ("-:--".to_string(), "-:--".to_string())
        } else {
            (clock(now), format!("-{}", clock(length.saturating_sub(now))))
        };
        let measure = cell.clone();
        div()
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(11.0))
            .text_color(rgba(TERTIARY))
            .child(div().w(px(40.0)).flex().justify_end().child(elapsed))
            .child(
                div()
                    .on_children_prepainted(move |bounds, _, _| {
                        if let Some(b) = bounds.first() {
                            measure.set((f32::from(b.origin.x), f32::from(b.size.width)));
                        }
                    })
                    .child(
                        div()
                            .id(id)
                            .w(px(width))
                            .h(px(16.0))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                    let (left, w) = cell.get();
                                    this.seek((f32::from(event.position.x) - left) / w.max(1.0));
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .h(px(4.0))
                                    .rounded_full()
                                    .bg(rgba(SUBTLE_STRONG))
                                    .child(div().h_full().rounded_full().bg(rgb(accent())).w(px(width * progress))),
                            ),
                    ),
            )
            .child(div().w(px(40.0)).child(remaining))
    }

    /// The dock's volume: click or drag along it; the mouse wheel nudges it by 5%.
    pub(crate) fn volume_slider(&self, stage: bool, cx: &mut Context<Self>) -> gpui::Div {
        // The full-screen player's own: the dock's is drawn beneath it, at another place.
        let bar = if stage { self.stage_volume_bar.clone() } else { self.volume_bar.clone() };
        let (measure, cell, cell_drag) = (bar.clone(), bar.clone(), bar);
        let level = if self.muted { 0.0 } else { self.volume };
        div()
            .on_children_prepainted(move |bounds, _, _| {
                if let Some(b) = bounds.first() {
                    measure.set((f32::from(b.origin.x), f32::from(b.size.width)));
                }
            })
            .child(
                div()
                    .id(if stage { "stage-volume-slider" } else { "volume-slider" })
                    .w(px(VOLUME))
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            let (left, w) = cell.get();
                            this.set_volume((f32::from(event.position.x) - left) / w.max(1.0));
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                        if event.pressed_button == Some(gpui::MouseButton::Left) {
                            let (left, w) = cell_drag.get();
                            this.set_volume((f32::from(event.position.x) - left) / w.max(1.0));
                            cx.notify();
                        }
                    }))
                    .on_scroll_wheel(cx.listener(|this, event: &gpui::ScrollWheelEvent, _, cx| {
                        let dy = f32::from(event.delta.pixel_delta(px(20.0)).y);
                        if dy != 0.0 {
                            this.set_volume(this.volume + if dy > 0.0 { 0.05 } else { -0.05 });
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(4.0))
                            .rounded_full()
                            .bg(rgba(SUBTLE_STRONG))
                            .child(div().h_full().rounded_full().bg(rgb(0xffffff)).w(px(VOLUME * level)))
                            .child(
                                div()
                                    .absolute()
                                    .top(px(-4.0))
                                    .left(px((VOLUME * level - 6.0).clamp(-2.0, VOLUME - 10.0)))
                                    .size(px(12.0))
                                    .rounded_full()
                                    .bg(rgb(0xffffff))
                                    .shadow_md(),
                            ),
                    ),
            )
    }

    /// The playing song's heart: lit when liked; a click likes it or takes it back.
    pub(crate) fn heart_button(&self, id: &'static str, size_px: f32, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let liked = self.current().is_some_and(|t| self.is_liked(&t.video_id));
        div()
            .id(id)
            .flex_none()
            .size(px(size_px + 12.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .font_family(ICONS)
            .text_size(px(size_px))
            .text_color(if liked { rgb(accent()) } else { rgba(SECONDARY) })
            .hover(|s| s.bg(rgba(SUBTLE_STRONG)).text_color(rgb(0xffffff)))
            .child(if liked { track_menu::HEART_FILLED } else { track_menu::HEART })
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.toggle_like_current(cx);
            }))
    }

    /// A song's artist, as a link to their page.
    pub(crate) fn artist_link(&self, id: &'static str, track: &Track, size_px: f32, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let t = track.clone();
        div()
            .id(id)
            .min_w_0()
            .truncate()
            .cursor_pointer()
            .text_size(px(size_px))
            .text_color(rgba(SECONDARY))
            .hover(|s| s.text_color(rgb(0xffffff)).underline())
            .child(track.artist.clone())
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.set_full_screen(false);
                this.open_artist_of(&t, cx);
            }))
    }

    pub(crate) fn dock(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status: Option<(SharedString, u32)> = match (&self.error, &self.opening, self.loading) {
            (Some(e), _, _) => Some((e.clone().into(), 0xff8a80ff)),
            (None, Some(title), _) => Some((tf("Opening “{}”…", &[title]).into(), SECONDARY)),
            (None, None, true) => Some((t("Loading…").into(), SECONDARY)),
            _ => None,
        };
        let cover_frame = self.cover_motion.frame(
            Motion::new(Target::default().y(0.0).opacity(1.0))
                .initial(Target::default().y(10.0).opacity(0.0))
                .transition(Transition::Spring(self.spring)),
            window,
            cx,
        );
        let now_playing = match self.current().cloned() {
            Some(track) => div()
                .id("now-playing")
                .on_mouse_down(gpui::MouseButton::Right, {
                    let t = track.clone();
                    cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                        this.open_track_menu(t.clone(), event.position, None);
                        cx.notify();
                    })
                })
                .flex()
                .items_center()
                .gap_3()
                .min_w_0()
                // The artwork opens the full-screen player, as in the web dock.
                .child(
                    div()
                        .id("dock-cover")
                        .cursor_pointer()
                        .child(cover_frame.apply(self.cover_art(self.cover.clone(), 56.0, 10.0).shadow_md()))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_full_screen(true);
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .min_w_0()
                                .child(div().text_size(px(14.0)).font_weight(FontWeight::SEMIBOLD).truncate().child(track.title.clone()))
                                .child(self.heart_button("dock-heart", 13.0, cx)),
                        )
                        .child(match &status {
                            Some((text, color)) => div().text_size(px(12.0)).text_color(rgba(*color)).truncate().child(text.clone()),
                            None => div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .min_w_0()
                                .child(self.artist_link("dock-artist", &track, 12.0, cx))
                                // Lossless from an addon: say so, as the old players' badge did.
                                .when_some(self.lossless_from.as_ref().map(|_| self.quality.clone()), |d, quality| {
                                    d.child(
                                        div()
                                            .flex_none()
                                            .px(px(6.0))
                                            .py(px(1.0))
                                            .rounded(px(4.0))
                                            .border_1()
                                            .border_color(rgba(accent_wash(0x99)))
                                            .text_size(px(10.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(accent()))
                                            .child(quality),
                                    )
                                }),
                        }),
                ),
            None => div()
                .id("now-playing")
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .size(px(56.0))
                        .rounded(px(10.0))
                        .bg(rgba(SUBTLE_STRONG))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(brand_mark(22.0)),
                )
                .child(div().text_size(px(13.0)).text_color(rgba(TERTIARY)).child(t("Pick a song to start"))),
        };

        let transport = self.transport(false, cx);
        let seek = self.seek_line("seek", self.seek_bar.clone(), SEEK, cx);
        div().p_2().child(
            div()
                .h(px(84.0))
                .px_4()
                .flex()
                .items_center()
                .gap_6()
                .rounded(px(14.0))
                .bg(rgba(DOCK))
                .backdrop_blur(px(28.0))
                .border_1()
                .border_color(rgba(STROKE))
                .shadow_lg()
                .child(div().flex_1().min_w_0().child(now_playing))
                .child(div().flex().flex_col().items_center().gap(px(2.0)).child(transport).child(seek))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_1()
                        .child(action("dock-add", "\u{E710}", false).on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                            if let Some(track) = this.current().cloned() {
                                this.open_playlist_menu(track, event.position());
                            }
                            cx.notify();
                        })))
                        .child(action("lyrics", "\u{E8BD}", self.panel == Panel::Lyrics).on_click(cx.listener(|this, _, _, cx| {
                            this.open_panel(Panel::Lyrics);
                            cx.notify();
                        })))
                        .child(action("queue", "\u{E8FD}", self.panel == Panel::UpNext).on_click(cx.listener(|this, _, _, cx| {
                            this.open_panel(Panel::UpNext);
                            cx.notify();
                        })))
                        .child(action("volume", volume_glyph(self.muted, self.volume), false).on_click(cx.listener(|this, _, _, cx| {
                            this.muted = !this.muted;
                            this.audio.mute(this.muted);
                            cx.notify();
                        })))
                        // Room either side, so the knob at full volume doesn't touch its neighbours.
                        .child(div().pl_1().pr_3().child(self.volume_slider(false, cx)))
                        .child(action("mini", "\u{E8A7}", false).on_click(cx.listener(|this, _, _, cx| {
                            this.open_mini_player(cx);
                        })))
                        .child(action("expand", "\u{E740}", false).on_click(cx.listener(|this, _, _, cx| {
                            if this.index.is_some() {
                                this.set_full_screen(true);
                                cx.notify();
                            }
                        }))),
                ),
        )
    }
}

pub(crate) fn clock(t: Duration) -> String {
    let s = t.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

pub(crate) fn icon(glyph: &'static str, size_px: f32) -> gpui::Div {
    div().font_family(ICONS).text_size(px(size_px)).flex().items_center().justify_center().child(glyph)
}

/// Seconds in a "3:33" or "1:02:03" length, if it is one.
pub(crate) fn clock_seconds(text: &str) -> Option<f64> {
    let mut total = 0.0;
    for part in text.split(':') {
        total = total * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    (total > 0.0).then_some(total)
}

/// The speaker icon for a volume: muted, then one to three waves.
pub(crate) fn volume_glyph(muted: bool, volume: f32) -> &'static str {
    match volume {
        _ if muted || volume <= 0.0 => "\u{E74F}",
        v if v < 0.34 => "\u{E993}",
        v if v < 0.67 => "\u{E994}",
        _ => "\u{E995}",
    }
}

/// NabyTune's mark, as on its icon (assets/make_icon.py): a record's ring with one line of
/// sound through it, in the accent. `height` is the ring's outer size.
pub(crate) fn brand_mark(height: f32) -> gpui::Div {
    let colour = rgb(accent());
    let mark = gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let (x, y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let side = f32::from(bounds.size.width).min(f32::from(bounds.size.height));
            let (cx, cy) = (x + side / 2.0, y + side / 2.0);
            let ring = (side * 0.118).max(1.0);
            let r = (side - ring) / 2.0;
            let mut circle = gpui::PathBuilder::stroke(px(ring));
            circle.move_to(gpui::point(px(cx - r), px(cy)));
            circle.arc_to(gpui::point(px(r), px(r)), px(0.0), false, true, gpui::point(px(cx + r), px(cy)));
            circle.arc_to(gpui::point(px(r), px(r)), px(0.0), false, true, gpui::point(px(cx - r), px(cy)));
            circle.close();
            if let Ok(path) = circle.build() {
                window.paint_path(path, colour);
            }
            // One period of a sine across the middle: up, then down, with round ends.
            let (half, amp, line) = (side * 0.245, side * 0.13, (side * 0.105).max(1.0));
            let at = |t: f32| gpui::point(px(cx - half + t * 2.0 * half), px(cy - (t * std::f32::consts::TAU).sin() * amp));
            let mut wave = gpui::PathBuilder::stroke(px(line));
            wave.move_to(at(0.0));
            for i in 1..=32 {
                wave.line_to(at(i as f32 / 32.0));
            }
            if let Ok(path) = wave.build() {
                window.paint_path(path, colour);
            }
            for end in [at(0.0), at(1.0)] {
                let cap = gpui::Bounds::new(gpui::point(end.x - px(line / 2.0), end.y - px(line / 2.0)), gpui::size(px(line), px(line)));
                window.paint_quad(gpui::fill(cap, colour).corner_radii(px(line / 2.0)));
            }
        },
    );
    div().size(px(height)).flex_none().child(mark.size_full())
}

/// A failed romanisation, in a few words: the rail is narrow. The whole error goes to the log.
pub(crate) fn short_error(what: &'static str, error: &anyhow::Error) -> SharedString {
    audio::log(format!("{what} failed: {error:#}"));
    tf("{} failed. Click to retry.", &[&t(what)]).into()
}

/// A rounded artwork square, or an empty tile while it loads.
pub(crate) fn artwork(image: Option<Arc<RenderImage>>, side: f32, radius: f32) -> gpui::Div {
    let tile = div().size(px(side)).flex_none().rounded(px(radius)).bg(rgba(SUBTLE_STRONG));
    match image {
        Some(image) => tile.child(img(image).object_fit(ObjectFit::Cover).size_full().rounded(px(radius))),
        None => tile,
    }
}

/// A 32 px square action in the dock or title bar; `on` lights it in the accent.
pub(crate) fn action(id: impl Into<gpui::ElementId>, glyph: &'static str, on: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .size(px(32.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .cursor_pointer()
        .font_family(ICONS)
        .text_size(px(15.0))
        .text_color(if on { rgb(accent()) } else { rgba(SECONDARY) })
        .hover(|s| s.bg(rgba(SUBTLE_STRONG)).text_color(rgb(0xffffff)))
        .child(glyph)
}
