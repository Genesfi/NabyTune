//! Settings, as one page in Windows 11's way: grouped cards of rows, each with a name, a
//! line on what it does, and its control on the right. Every change applies at once and is
//! kept in the settings file (the sleep timer only lasts until NabyTune closes).

use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, rgb, rgba, AnyElement, Context, FontWeight, SharedString, Window};
use gpui_ce_motion::MotionState;

use crate::appearance::{self, accent, on_accent, Material, PRESETS};
use crate::i18n::{self, plural, t, tf};
use crate::{eq, settings, Look, NabyTune, Sleep, ICONS, SECONDARY, STROKE, SUBTLE, SUBTLE_STRONG, TERTIARY};

/// Settings' two colour strips: which way along the colour wheel, and how light.
#[derive(Clone, Copy)]
enum Strip {
    Hue,
    Light,
}

/// A choice in a segmented control was picked.
type Pick<T> = fn(&mut NabyTune, T, &mut Window, &mut Context<NabyTune>);

/// The lyric sources, as Settings › Lyrics › Preferred source offers them ("" is the best).
/// Settings › Playback › Crossfade: (seconds, label).
const CROSSFADES: [(u32, &str); 6] = [(0, "Off"), (2, "2 s"), (4, "4 s"), (6, "6 s"), (8, "8 s"), (12, "12 s")];

const SOURCES: [(&str, &str); 6] =
    [("", "Best available"), ("LyricsPlus", "LyricsPlus"), ("LRCLIB", "LRCLIB"), ("KuGou", "KuGou"), ("NetEase", "NetEase"), ("YouTube Music", "YouTube Music")];

const SLEEP_CHOICES: [(Sleep, &str); 7] = [
    (Sleep::Off, "Off"),
    (Sleep::Minutes(15), "15 min"),
    (Sleep::Minutes(30), "30 min"),
    (Sleep::Minutes(45), "45 min"),
    (Sleep::Minutes(60), "1 hr"),
    (Sleep::Minutes(90), "1.5 hr"),
    (Sleep::EndOfSong, "End of song"),
];

/// Where to support NabyTune's developer.
pub(crate) const SAWERIA: &str = "https://saweria.co/billynabil";
pub(crate) const KOFI: &str = "https://ko-fi.com/billynabil";

/// Where NabyTune comes from, and the licence it's under (GPL v3, as BitChord's).
const BITCHORD: &str = "https://github.com/kushagrasinghx/BitChord";
const GPL: &str = "https://www.gnu.org/licenses/gpl-3.0.html";

/// A small button that opens `url` in the browser.
fn link_button(id: &'static str, label: &'static str, url: &'static str, cx: &mut Context<NabyTune>) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(32.0))
        .px(px(14.0))
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .rounded(px(6.0))
        .cursor_pointer()
        .text_size(px(13.0))
        .border_1()
        .border_color(rgba(STROKE))
        .bg(rgba(SUBTLE_STRONG))
        .hover(|s| s.bg(rgba(0xffffff26)))
        .child(div().font_family(ICONS).text_size(px(12.0)).child("\u{E8A7}"))
        .child(t(label))
        .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url)))
}

/// Saweria and Ko-fi, as two buttons that open the page in the browser.
pub(crate) fn support_buttons(cx: &mut Context<NabyTune>) -> gpui::Div {
    let button = |id: &'static str, label: &'static str, url: &'static str, primary: bool| {
        div()
            .id(id)
            .h(px(34.0))
            .px(px(16.0))
            .flex()
            .items_center()
            .gap_2()
            .rounded_full()
            .cursor_pointer()
            .text_size(px(13.0))
            .font_weight(FontWeight::SEMIBOLD)
            .map(|d| {
                if primary {
                    d.bg(rgb(accent())).text_color(rgb(on_accent())).hover(|s| s.opacity(0.9))
                } else {
                    d.bg(rgba(SUBTLE_STRONG)).border_1().border_color(rgba(STROKE)).hover(|s| s.bg(rgba(0xffffff26)))
                }
            })
            .child(div().font_family(ICONS).text_size(px(12.0)).child("\u{E8A7}"))
            .child(label)
            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url)))
    };
    div().flex().flex_none().gap_2().child(button("support-saweria", "Saweria", SAWERIA, true)).child(button("support-kofi", "Ko-fi", KOFI, false))
}

/// A group of settings under a heading (translated here), as one card with hairlines between its rows.
fn section(title: &'static str, rows: Vec<AnyElement>) -> gpui::Div {
    let mut card = div().flex().flex_col().rounded(px(10.0)).bg(rgba(SUBTLE)).border_1().border_color(rgba(STROKE));
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            card = card.child(div().h(px(1.0)).mx(px(16.0)).bg(rgba(STROKE)));
        }
        card = card.child(row);
    }
    div().flex().flex_col().gap_2().child(div().text_size(px(14.0)).font_weight(FontWeight::SEMIBOLD).child(t(title))).child(card)
}

/// One setting: its name (translated here) and a line about it on the left, its control on the right.
fn row(title: &'static str, hint: impl Into<SharedString>, control: impl IntoElement) -> AnyElement {
    div()
        .min_h(px(64.0))
        .px(px(16.0))
        .py(px(12.0))
        .flex()
        .items_center()
        .gap_6()
        .child(
            div()
                .flex_1()
                .min_w(px(180.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(div().text_size(px(14.0)).child(t(title)))
                .child(div().text_size(px(12.0)).text_color(rgba(SECONDARY)).child(hint.into())),
        )
        .child(control)
        .into_any_element()
}

/// An on/off switch, Windows 11's shape in NabyTune's accent.
pub(crate) fn switch(id: &'static str, on: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .w(px(40.0))
        .h(px(20.0))
        .px(px(3.0))
        .flex()
        .items_center()
        .rounded_full()
        .cursor_pointer()
        .map(|d| if on { d.justify_end().bg(rgb(accent())) } else { d.justify_start().border_1().border_color(rgba(0xffffff8c)).hover(|s| s.bg(rgba(SUBTLE_STRONG))) })
        .child(div().size(px(12.0)).rounded_full().bg(if on { rgb(on_accent()) } else { rgba(0xffffffcc) }))
}

impl NabyTune {
    pub(crate) fn settings_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let sections = vec![
            self.support_section(cx),
            self.account_section(cx),
            self.appearance_section(cx),
            self.playback_section(cx),
            self.lossless_section(cx),
            self.lyrics_section(window, cx),
            self.integrations_section(cx),
            self.shortcuts_section(),
            self.about_section(cx),
        ];
        if self.settings_motion.len() < sections.len() {
            self.settings_motion.resize_with(sections.len(), MotionState::default);
        }
        let mut column = div().w_full().max_w(px(880.0)).flex().flex_col().gap_6().child(Self::heading(t("Settings"), 30.0));
        for (i, section) in sections.into_iter().enumerate() {
            let rise = self.rise(i);
            let frame = self.settings_motion[i].frame(rise, window, cx);
            column = column.child(frame.apply(section));
        }
        // A readable column in the middle of the page, however wide the window.
        div()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .px(px(32.0))
            .pt(px(28.0))
            .pb(px(40.0))
            .flex()
            .flex_col()
            .items_center()
            .child(column)
            .into_any_element()
    }

    /// Any colour: a strip of every hue, and under it one from dark to light, each to click
    /// or drag along. Each keeps what the other set.
    fn accent_strips(&self, cx: &mut Context<Self>) -> gpui::Div {
        let current = accent();
        let custom = self.accent_choice.starts_with('#') && !PRESETS.iter().any(|(_, c)| format!("#{c:06x}") == self.accent_choice);
        let hue = appearance::hue_of(current);
        let top = if appearance::is_neutral(current) { 0.94 } else { appearance::LIGHTEST };
        let light = appearance::lightness_of(current).clamp(appearance::DARKEST, top);
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(self.strip(Strip::Hue, hue, light, custom && !appearance::is_neutral(current), cx))
            .child(self.strip(Strip::Light, hue, light, true, cx))
    }

    fn strip(&self, kind: Strip, hue: f32, light: f32, marked: bool, cx: &mut Context<Self>) -> gpui::Div {
        const WIDTH: f32 = 292.0;
        let bar = match kind {
            Strip::Hue => self.hue_bar.clone(),
            Strip::Light => self.light_bar.clone(),
        };
        let (measure, cell, cell_drag) = (bar.clone(), bar.clone(), bar);
        let grey = appearance::is_neutral(accent());
        let top = if grey { 0.94 } else { appearance::LIGHTEST };
        // Where along the strip (0 to 1) picks which colour.
        let colour_at = move |at: f32| match kind {
            Strip::Hue => appearance::hsl(at.clamp(0.0, 0.999), light),
            // A grey accent darkens and lightens through greys, from near-black to white.
            Strip::Light if grey => appearance::neutral(appearance::DARKEST + (0.94 - appearance::DARKEST) * at.clamp(0.0, 1.0)),
            Strip::Light => appearance::hsl(hue, appearance::DARKEST + (appearance::LIGHTEST - appearance::DARKEST) * at.clamp(0.0, 1.0)),
        };
        let pick = move |this: &mut NabyTune, x: f32, cell: &std::rc::Rc<std::cell::Cell<(f32, f32)>>| {
            let (left, w) = cell.get();
            let key = format!("#{:06x}", colour_at((x - left) / w.max(1.0)));
            this.pick_accent(&key);
        };
        let stops = 6;
        let mut track = div().relative().w(px(WIDTH)).h(px(14.0)).rounded_full().flex().overflow_hidden();
        for i in 0..stops {
            let (a, b) = (colour_at(i as f32 / stops as f32), colour_at((i + 1) as f32 / stops as f32));
            track = track.child(div().flex_1().h_full().bg(gpui::linear_gradient(90.0, gpui::linear_color_stop(rgb(a), 0.0), gpui::linear_color_stop(rgb(b), 1.0))));
        }
        let at = match kind {
            Strip::Hue => hue,
            Strip::Light => (light.min(top) - appearance::DARKEST) / (top - appearance::DARKEST),
        } * WIDTH;
        let marker = marked.then(|| {
            div().absolute().top(px(-3.0)).left(px((at - 10.0).clamp(-2.0, WIDTH - 18.0))).size(px(20.0)).rounded_full().border_2().border_color(rgb(0xffffff)).bg(rgb(accent())).shadow_md()
        });
        div()
            .on_children_prepainted(move |bounds, _, _| {
                if let Some(b) = bounds.first() {
                    measure.set((f32::from(b.origin.x), f32::from(b.size.width)));
                }
            })
            .child(
                div()
                    .id(match kind {
                        Strip::Hue => "hue-strip",
                        Strip::Light => "light-strip",
                    })
                    .relative()
                    .py(px(3.0))
                    .cursor_pointer()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            pick(this, f32::from(event.position.x), &cell);
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                        if event.pressed_button == Some(gpui::MouseButton::Left) {
                            pick(this, f32::from(event.position.x), &cell_drag);
                            cx.notify();
                        }
                    }))
                    .child(track)
                    .children(marker),
            )
    }

    /// A row of choices, the picked one raised.
    pub(crate) fn segmented<T: Copy + PartialEq + 'static>(&self, id: &'static str, options: Vec<(T, SharedString)>, selected: T, pick: Pick<T>, cx: &mut Context<Self>) -> gpui::Div {
        let mut group = div().flex().flex_wrap().justify_end().gap(px(2.0)).p(px(3.0)).rounded(px(8.0)).bg(rgba(0x0000_0033)).border_1().border_color(rgba(STROKE));
        for (n, (value, label)) in options.into_iter().enumerate() {
            let on = value == selected;
            group = group.child(
                div()
                    .id((id, n))
                    .h(px(28.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .text_size(px(13.0))
                    .map(|d| {
                        if on {
                            d.bg(rgba(SUBTLE_STRONG)).text_color(rgb(0xffffff)).font_weight(FontWeight::SEMIBOLD).border_b_2().border_color(rgb(accent()))
                        } else {
                            d.text_color(rgba(SECONDARY)).hover(|s| s.bg(rgba(SUBTLE)).text_color(rgb(0xffffff)))
                        }
                    })
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        pick(this, value, window, cx);
                        cx.notify();
                    })),
            );
        }
        group
    }

    /// Supporting NabyTune's developer, first on the page.
    fn support_section(&self, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .px(px(20.0))
            .py(px(18.0))
            .flex()
            .items_center()
            .gap_5()
            .rounded(px(12.0))
            .border_1()
            .border_color(rgba(appearance::accent_wash(0x55)))
            .bg(gpui::linear_gradient(
                120.0,
                gpui::linear_color_stop(rgba(appearance::accent_wash(0x2a)), 0.0),
                gpui::linear_color_stop(rgba(0xffff_ff08), 1.0),
            ))
            .child(div().font_family(ICONS).text_size(px(26.0)).text_color(rgb(accent())).child("\u{EB51}"))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(div().text_size(px(15.0)).font_weight(FontWeight::SEMIBOLD).child(t("Support NabyTune")))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgba(SECONDARY))
                            .child(t("Free, no ads, made by one developer. If it's part of your day, a coffee helps keep it going.")),
                    ),
            )
            .child(support_buttons(cx))
    }

    /// Signing in to YouTube Music: Google's own page opens in a window of its own; the
    /// password goes to Google, never through NabyTune.
    fn account_section(&self, cx: &mut Context<Self>) -> gpui::Div {
        let pill = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .h(px(32.0))
                .px(px(16.0))
                .flex()
                .items_center()
                .rounded_full()
                .cursor_pointer()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .map(|d| {
                    if primary {
                        d.bg(rgb(accent())).text_color(rgb(on_accent())).hover(|s| s.opacity(0.9))
                    } else {
                        d.bg(rgba(SUBTLE_STRONG)).border_1().border_color(rgba(STROKE)).hover(|s| s.bg(rgba(0xffffff26)))
                    }
                })
                .child(t(label))
        };
        let row_view = match (&self.account, self.signing_in, self.youtube.signed_in()) {
            (Some(account), _, _) => {
                let photo = self.art.get("account").map(|a| a.small.clone());
                let hint = if account.handle.is_empty() { t("Signed in to YouTube Music").to_string() } else { tf("{} · signed in to YouTube Music", &[&account.handle]) };
                div()
                    .min_h(px(72.0))
                    .px(px(16.0))
                    .py(px(12.0))
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(match photo {
                        Some(image) => div().size(px(44.0)).rounded_full().overflow_hidden().child(gpui::img(image).size_full()),
                        None => div().size(px(44.0)).rounded_full().bg(rgb(accent())).text_color(rgb(on_accent())).flex().items_center().justify_center().font_weight(FontWeight::BOLD).child(account.name.chars().next().unwrap_or('?').to_string()),
                    })
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(div().text_size(px(15.0)).font_weight(FontWeight::SEMIBOLD).child(account.name.clone()))
                            .child(div().text_size(px(12.0)).text_color(rgba(SECONDARY)).child(hint)),
                    )
                    .child(pill("sign-out", "Sign out", false).on_click(cx.listener(|this, _, _, cx| this.sign_out(cx))))
                    .into_any_element()
            }
            (None, true, _) => row("YouTube Music", t("Finish signing in in the window that opened…"), div().text_size(px(12.0)).text_color(rgba(TERTIARY)).child(t("Waiting"))),
            (None, false, true) => row("YouTube Music", t("Signed in; loading your account…"), div()),
            (None, false, false) => row(
                "YouTube Music",
                self.account_error.clone().unwrap_or_else(|| t("Sign in for your library, your playlists and a Home made for you").into()),
                pill("sign-in", "Sign in", true).on_click(cx.listener(|this, _, _, cx| this.sign_in(cx))),
            ),
        };
        section("Account", vec![row_view])
    }

    /// Lossless: addons added by address. YouTube itself never streams lossless, so these are
    /// the only way to it; a song an addon doesn't have plays from YouTube as before.
    fn lossless_section(&self, cx: &mut Context<Self>) -> gpui::Div {
        let tiers = vec![(crate::addon::Tier::Lossless, SharedString::from("CD · 16-bit")), (crate::addon::Tier::HiRes, SharedString::from("Hi-Res · 24-bit"))];
        let playing: SharedString = match (&self.lossless_from, self.index.is_some()) {
            (Some(name), _) => tf("Now playing {} from {}", &[&self.quality, name]).into(),
            (None, true) if !self.quality.is_empty() => tf("Now playing {} from YouTube", &[&self.quality]).into(),
            _ => t("Songs an addon has play lossless; the rest stay on YouTube").into(),
        };
        let mut rows = vec![
            row(
                "Play lossless",
                playing,
                switch("lossless", self.lossless).on_click(cx.listener(|this, _, _, cx| {
                    this.lossless = !this.lossless;
                    settings::set("lossless", if this.lossless { "1" } else { "0" });
                    cx.notify();
                })),
            ),
            row(
                "Quality",
                t("Hi-Res is larger: up to 24-bit/192 kHz, where the addon has it"),
                self.segmented(
                    "tier",
                    tiers,
                    self.tier,
                    |this, tier, _, _| {
                        this.tier = tier;
                        settings::set("lossless_tier", tier.key());
                    },
                    cx,
                ),
            ),
        ];
        // Each addon added: its name, where it lives (path hidden: it may hold a token), and ×.
        for url in self.addon_urls.clone() {
            let found = self.addons.iter().find(|a| a.base == url);
            let (title, hint): (SharedString, SharedString) = match (found, self.addon_errors.get(&url)) {
                (Some(a), _) => (
                    if a.version.is_empty() { a.name.clone().into() } else { format!("{} · v{}", a.name, a.version).into() },
                    crate::addon::redact(&url).into(),
                ),
                (None, Some(e)) => (crate::addon::redact(&url).into(), tf("Not answering: {}", &[e]).into()),
                (None, None) => (crate::addon::redact(&url).into(), t("Connecting…").into()),
            };
            let remove = url.clone();
            rows.push(
                div()
                    .min_h(px(60.0))
                    .px(px(16.0))
                    .py(px(10.0))
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(div().font_family(ICONS).text_size(px(16.0)).text_color(if found.is_some() { rgb(accent()) } else { rgba(TERTIARY) }).child("\u{E8D6}"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(div().text_size(px(14.0)).truncate().child(title))
                            .child(div().text_size(px(12.0)).text_color(rgba(SECONDARY)).truncate().child(hint)),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("remove-{url}")))
                            .size(px(30.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.0))
                            .cursor_pointer()
                            .font_family(ICONS)
                            .text_size(px(11.0))
                            .text_color(rgba(SECONDARY))
                            .hover(|s| s.bg(rgba(SUBTLE_STRONG)).text_color(rgb(0xffffff)))
                            .child("\u{E711}")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_addon(&remove);
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            );
        }
        let add_hint: SharedString = match (self.adding_addon, self.addon_errors.get("")) {
            (true, _) => t("Reading its manifest…").into(),
            (false, Some(e)) => e.clone().into(),
            _ => t("Paste an addon's address and press Enter. None is built in.").into(),
        };
        rows.push(row(
            "Add an addon",
            add_hint,
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(self.addon_field.clone())
                .child(
                    div()
                        .id("add-addon")
                        .h(px(32.0))
                        .px(px(14.0))
                        .flex()
                        .items_center()
                        .rounded(px(6.0))
                        .cursor_pointer()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .bg(rgb(accent()))
                        .text_color(rgb(on_accent()))
                        .hover(|s| s.opacity(0.9))
                        .child(t("Add"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            let url = this.addon_field.read(cx).text().to_string();
                            this.add_addon(url, cx);
                        })),
                ),
        ));
        section("Lossless", rows)
    }

    fn appearance_section(&self, cx: &mut Context<Self>) -> gpui::Div {
        // Accent: Windows' own, then the presets.
        let mut swatches = div().flex().flex_wrap().justify_end().gap(px(8.0));
        let mut choices: Vec<(String, u32, &str)> = Vec::new();
        if let Some(system) = appearance::system_accent() {
            choices.push(("system".into(), system, "Windows"));
        }
        choices.extend(PRESETS.iter().map(|(name, colour)| (format!("#{colour:06x}"), *colour, *name)));
        let follows = appearance::follows_cover();
        let current_name = t(choices.iter().find(|(key, _, _)| *key == self.accent_choice).map_or("Custom", |(_, _, name)| *name));
        for (n, (key, colour, _)) in choices.into_iter().enumerate() {
            let on = !follows && key == self.accent_choice;
            swatches = swatches.child(
                div()
                    .id(("accent", n))
                    .size(px(28.0))
                    .rounded_full()
                    .cursor_pointer()
                    .bg(rgb(colour))
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_2()
                    .border_color(if on { rgb(0xffffff) } else { rgba(0x0000_0000) })
                    .hover(|s| s.border_color(rgba(0xffffff80)))
                    .when(key == "system", |d| d.child(div().font_family(ICONS).text_size(px(11.0)).text_color(rgba(0x000000aa)).child("\u{E7F4}")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pick_accent(&key);
                        cx.notify();
                    })),
            );
        }
        let materials = Material::ALL.iter().map(|m| (*m, SharedString::from(m.name()))).collect();
        let material_hint = t(match self.material {
            Material::Mica => "Your wallpaper's colours, softly, behind the page",
            Material::MicaAlt => "Mica with more of the wallpaper's colour",
            Material::Acrylic => "Frosted glass over whatever is behind the window",
            Material::Solid => "A plain dark ground: nothing shows through",
        });
        section(
            "Appearance",
            vec![
                row(
                    "Colour from the album cover",
                    t("The accent takes the colour of the cover that's playing, and changes with each song"),
                    switch("accent-cover", follows).on_click(cx.listener(move |this, _, _, cx| {
                        let key = if follows { this.accent_choice.clone() } else { "cover".to_string() };
                        this.pick_accent(&key);
                        cx.notify();
                    })),
                ),
                row(
                    "Accent colour",
                    match follows {
                        true => SharedString::from(t("Shown until a song's cover gives its colour. Picking one here turns the cover colour off")),
                        false => tf("Buttons, highlights and the song that's playing · {}. Pick one, or any colour and brightness on the strips", &[&current_name]).into(),
                    },
                    div().flex().flex_col().items_end().gap(px(12.0)).when(follows, |d| d.opacity(0.55)).child(swatches).child(self.accent_strips(cx)),
                ),
                row(
                    "Window material",
                    material_hint,
                    self.segmented(
                        "material",
                        materials,
                        self.material,
                        |this, material, window, _| {
                            this.material = material;
                            material.apply(window);
                            settings::set("material", material.key());
                        },
                        cx,
                    ),
                ),
                row(
                    "Ambient background",
                    t("The playing song's cover, as a soft glow behind everything"),
                    switch("ambient", self.show_ambient).on_click(cx.listener(|this, _, _, cx| {
                        this.show_ambient = !this.show_ambient;
                        settings::set("ambient", if this.show_ambient { "1" } else { "0" });
                        cx.notify();
                    })),
                ),
                row(
                    "Vinyl disc",
                    t("Show album art as a vinyl record across full-screen layouts"),
                    switch("vinyl-mode", self.vinyl_mode).on_click(cx.listener(|this, _, _, cx| {
                        this.vinyl_mode = !this.vinyl_mode;
                        settings::set("vinyl_mode", if this.vinyl_mode { "1" } else { "0" });
                        cx.notify();
                    })),
                ),
                row(
                    "Vinyl rotation",
                    t("Spin the vinyl record and album art while playing"),
                    switch("vinyl-rotate", self.vinyl_rotate).on_click(cx.listener(|this, _, _, cx| {
                        this.vinyl_rotate = !this.vinyl_rotate;
                        settings::set("vinyl_rotate", if this.vinyl_rotate { "1" } else { "0" });
                        cx.notify();
                    })),
                ),
                row(
                    "Vinyl spin speed",
                    t("How fast the vinyl disc and album art rotate"),
                    self.vinyl_speed_slider(260.0, cx),
                ),
                // Each language named in itself, so either can be found from the other.
                row(
                    "Language",
                    t("Menus, buttons and messages in NabyTune"),
                    self.segmented(
                        "language",
                        vec![("en", SharedString::from("English")), ("id", SharedString::from("Bahasa Indonesia"))],
                        i18n::language(),
                        |_, code, _, _| {
                            i18n::set_language(code);
                            settings::set("language", code);
                        },
                        cx,
                    ),
                ),
            ],
        )
    }

    fn playback_section(&self, cx: &mut Context<Self>) -> gpui::Div {
        let sleep_hint: SharedString = match (self.sleep, self.sleep_at) {
            (Sleep::EndOfSong, _) => t("Pauses when this song ends").into(),
            (Sleep::Minutes(_), Some(at)) => {
                let left = at.saturating_duration_since(Instant::now()).as_secs().div_ceil(60);
                tf("Pauses in {} min", &[&left]).into()
            }
            _ => t("Pause playback after a while, or when the song ends").into(),
        };
        let sleeps = SLEEP_CHOICES.iter().map(|(s, label)| (*s, SharedString::from(t(label)))).collect();
        section(
            "Playback",
            vec![
                row(
                    "Normalise volume",
                    t("Loud songs are turned down to the same level, as on YouTube Music"),
                    switch("normalize", self.normalize).on_click(cx.listener(|this, _, _, cx| {
                        this.normalize = !this.normalize;
                        settings::set("normalize", if this.normalize { "1" } else { "0" });
                        this.audio.set_volume(this.output_volume());
                        cx.notify();
                    })),
                ),
                row(
                    "Autoplay",
                    t("Keep playing songs with similar vibes when the queue runs low"),
                    switch("autoplay", self.autoplay).on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_autoplay(cx);
                    })),
                ),
                row(
                    "Crossfade",
                    t("Songs that run into the next overlap by this long, one fading out as the other fades in"),
                    self.segmented(
                        "crossfade",
                        CROSSFADES.iter().map(|(s, label)| (*s, SharedString::from(t(label)))).collect(),
                        self.crossfade,
                        |this, secs, _, _| {
                            this.crossfade = secs;
                            settings::set("crossfade", &secs.to_string());
                        },
                        cx,
                    ),
                ),
                row(
                    "Equalizer",
                    t("Shape the sound: more bass, clearer voices, or your own curve"),
                    switch("eq", self.eq_on).on_click(cx.listener(|this, _, _, cx| {
                        let gains = this.eq_gains;
                        this.set_eq(!this.eq_on, gains);
                        cx.notify();
                    })),
                ),
                self.eq_editor(cx),
                row(
                    "Pick up where you left off",
                    t("When NabyTune opens, the last queue is back, paused where it was"),
                    switch("resume", settings::get("resume").is_none_or(|v| v != "0")).on_click(cx.listener(|_, _, _, cx| {
                        let on = settings::get("resume").is_none_or(|v| v != "0");
                        settings::set("resume", if on { "0" } else { "1" });
                        cx.notify();
                    })),
                ),
                row(
                    "Sleep timer",
                    sleep_hint,
                    self.segmented(
                        "sleep",
                        sleeps,
                        self.sleep,
                        |this, sleep, _, _| {
                            this.sleep = sleep;
                            this.sleep_at = match sleep {
                                Sleep::Minutes(m) => Some(Instant::now() + Duration::from_secs(m * 60)),
                                _ => None,
                            };
                        },
                        cx,
                    ),
                ),
            ],
        )
    }

    /// The chosen lyric look, playing four made-up lines on a loop: what Focus, Karaoke and
    /// the rest do is easier seen than read. Follows the blur and romaji switches too.
    pub(crate) fn lyric_preview(&self, window: &mut Window) -> AnyElement {
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        const ENGLISH: [(&str, &str); 4] =
            [("City lights are fading out", ""), ("Hold on to the night", ""), ("Sing it one more time", ""), ("Till the morning comes", "")];
        const JAPANESE: [(&str, &str); 4] = [
            ("夢ならばどれほどよかったでしょう", "Yume naraba dorehodo yokatta deshou"),
            ("未だにあなたのことを夢にみる", "Imadani anata no koto o yume ni miru"),
            ("忘れた物を取りに帰るように", "Wasureta mono o tori ni kaeru you ni"),
            ("古びた思い出の埃を払う", "Furubita omoide no hokori o harau"),
        ];
        // Moving: ask for the next frame (only while Settings shows).
        window.request_animation_frame();
        let lines = if self.show_romaji { &JAPANESE } else { &ENGLISH };
        let t = START.get_or_init(Instant::now).elapsed().as_secs_f32();
        let period = 2.8;
        let index = (t / period) as usize % lines.len();
        let frac = (t / period).fract();
        // Each line holds, then the view glides to the next over the period's last fifth.
        let glide = {
            let g = ((frac - 0.8) / 0.2).clamp(0.0, 1.0);
            g * g * (3.0 - 2.0 * g)
        };
        let look = self.look;
        let (base, dim) = match look {
            Look::Minimal => (16.0, 0.26),
            Look::Single => (26.0, 0.0),
            Look::Center => (20.0, 0.32),
            _ => (21.0, 0.3),
        };
        let height = 210.0;
        let spacing = base * 2.3 + if self.show_romaji { 14.0 } else { 0.0 };
        let centred = matches!(look, Look::Center | Look::Single);
        let mut stage = div()
            .relative()
            .w_full()
            .h(px(height))
            .overflow_hidden()
            .rounded(px(10.0))
            .bg(gpui::linear_gradient(
                135.0,
                gpui::linear_color_stop(rgba(appearance::accent_wash(0x2e)), 0.0),
                gpui::linear_color_stop(rgba(0x0000_0066), 1.0),
            ))
            .border_1()
            .border_color(rgba(STROKE));
        for d in -2i32..=2 {
            let (text, romaji) = lines[(index as i32 + d).rem_euclid(lines.len() as i32) as usize];
            let y = (d as f32 - glide) * spacing;
            // 1 on the sung line, falling to 0 a line away.
            let e = (1.0 - y.abs() / spacing).clamp(0.0, 1.0);
            let opacity = match look {
                Look::Single => e * e,
                _ => dim + (1.0 - dim) * e,
            };
            if opacity < 0.02 {
                continue;
            }
            let size = base * if look == Look::Center { 1.0 + 0.14 * e } else { 1.0 };
            let colour = if look == Look::Neon {
                let (a, w) = (accent(), 0xffffffu32);
                let mix = |s: u32| ((((w >> s) & 0xff) as f32 * (1.0 - e) + ((a >> s) & 0xff) as f32 * e) as u32) << s;
                mix(16) | mix(8) | mix(0)
            } else {
                0xffffff
            };
            let blur = (self.blur_lyrics && look.blurs()).then(|| (y.abs() / spacing * 1.6).min(4.5)).filter(|b| *b > 0.2);
            let mut words = div().flex().flex_wrap().when(centred, |w| w.justify_center());
            if look == Look::Karaoke && d == 0 && glide == 0.0 {
                // The sung line lights character by character over its time.
                let progress = (frac / 0.75).clamp(0.0, 1.0);
                let chars: Vec<char> = text.chars().collect();
                let lit = (chars.len() as f32 * progress) as usize;
                words = words
                    .child(div().child(chars[..lit].iter().collect::<String>()))
                    .child(div().text_color(gpui::hsla(0.0, 0.0, 1.0, 0.35)).child(chars[lit..].iter().collect::<String>()));
            } else {
                words = words.child(text);
            }
            let line = div()
                .absolute()
                .left(px(28.0))
                .right(px(28.0))
                .top(px(height / 2.0 - base * 0.7 + y))
                .flex()
                .flex_col()
                .when(centred, |l| l.items_center().text_center())
                .opacity(opacity)
                .font_family(crate::DISPLAY)
                .font_weight(if look == Look::Minimal { FontWeight::MEDIUM } else { FontWeight::BOLD })
                .text_size(px(size))
                .line_height(px(size * 1.3))
                .text_color(rgb(colour))
                .when_some(blur, |l, b| l.blur(px(b)))
                .child(words)
                .when(!romaji.is_empty(), |l| {
                    l.child(div().text_size(px(size * 0.56)).line_height(px(size * 0.75)).font_weight(FontWeight::MEDIUM).text_color(rgba(0xffffffcc)).child(romaji))
                });
            stage = stage.child(line);
        }
        div().p(px(16.0)).child(stage).into_any_element()
    }

    fn lyrics_section(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let looks = Look::ALL.iter().map(|l| (*l, SharedString::from(l.name()))).collect();
        let sources = SOURCES.iter().map(|(key, label)| (*key, SharedString::from(t(label)))).collect();
        let preferred = SOURCES.iter().map(|(key, _)| *key).find(|key| *key == self.preferred_source).unwrap_or("");
        let picked = settings::count_prefixed("lyrics_source.");
        let reset_hint: SharedString = match picked {
            0 => t("No song has a source picked by hand").into(),
            n => plural(n, "{} song keeps the source you picked for it", "{} songs keep the source you picked for them").into(),
        };
        section(
            "Lyrics",
            vec![
                self.lyric_preview(window),
                row(
                    "Romaji",
                    t("Lyrics in other scripts, in Latin letters under each line: Japanese, Chinese, Korean, Russian, Greek, Hindi, Thai, Arabic"),
                    switch("romaji", self.show_romaji).on_click(cx.listener(|this, _, _, cx| {
                        this.show_romaji = !this.show_romaji;
                        settings::set("lyrics_romaji", if this.show_romaji { "1" } else { "0" });
                        this.fetch_extras(cx);
                        cx.notify();
                    })),
                ),
                row(
                    "Blur other lines",
                    t("Lines away from the one being sung go softly out of focus"),
                    switch("blur", self.blur_lyrics).on_click(cx.listener(|this, _, _, cx| {
                        this.blur_lyrics = !this.blur_lyrics;
                        settings::set("blur_lyrics", if this.blur_lyrics { "1" } else { "0" });
                        cx.notify();
                    })),
                ),
                row(
                    "Lyric style",
                    self.look.hint(),
                    self.segmented(
                        "look",
                        looks,
                        self.look,
                        |this, look, _, _| {
                            this.look = look;
                            settings::set("lyric_look", look.key());
                        },
                        cx,
                    ),
                ),
                row(
                    "Preferred source",
                    t("Shown first when a song has it; the lyric menu can still switch"),
                    self.segmented(
                        "source",
                        sources,
                        preferred,
                        |this, source, _, _| {
                            this.preferred_source = source.to_string();
                            settings::set("lyrics_preferred", source);
                        },
                        cx,
                    ),
                ),
                self.timing_row(cx),
                row(
                    "Sources picked per song",
                    reset_hint,
                    div()
                        .id("reset-sources")
                        .h(px(32.0))
                        .px(px(14.0))
                        .flex()
                        .items_center()
                        .rounded(px(6.0))
                        .text_size(px(13.0))
                        .border_1()
                        .border_color(rgba(STROKE))
                        .bg(rgba(SUBTLE_STRONG))
                        .map(|d| if picked == 0 { d.opacity(0.4) } else { d.cursor_pointer().hover(|s| s.bg(rgba(0xffffff26))) })
                        .child(t("Reset"))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            if picked > 0 {
                                settings::remove_prefixed("lyrics_source.");
                                cx.notify();
                            }
                        })),
                ),
            ],
        )
    }

    /// The equalizer's presets and its five bands, each a bar to drag up or down.
    fn eq_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        const HEIGHT: f32 = 120.0;
        let on = self.eq_on;
        let gains = self.eq_gains;
        let preset = eq::preset_of(&gains).unwrap_or("");
        let mut presets = div().flex().flex_wrap().gap(px(6.0));
        for (n, (name, curve)) in eq::PRESETS.iter().enumerate() {
            let picked = *name == preset;
            let curve = *curve;
            presets = presets.child(
                div()
                    .id(("eq-preset", n))
                    .h(px(28.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .border_1()
                    .map(|d| if picked { d.bg(rgb(accent())).text_color(rgb(on_accent())).border_color(rgb(accent())) } else { d.border_color(rgba(STROKE)).hover(|s| s.bg(rgba(SUBTLE_STRONG))) })
                    .child(t(name))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_eq(true, curve);
                        cx.notify();
                    })),
            );
        }
        let bars = self.eq_bars.clone();
        let measure = bars.clone();
        let mut bands = div()
            .flex()
            .gap(px(18.0))
            .on_children_prepainted(move |bounds, _, _| {
                *measure.borrow_mut() = bounds.iter().map(|b| (f32::from(b.origin.y), f32::from(b.size.height))).collect();
            });
        for (i, gain) in gains.iter().copied().enumerate() {
            // 0 dB in the middle; the fill runs from there to the gain.
            let half = HEIGHT / 2.0;
            let level = gain / eq::RANGE * half;
            let fill_top = if level > 0.0 { half - level } else { half };
            let pick = {
                let bars = bars.clone();
                move |this: &mut NabyTune, y: f32| {
                    let Some((top, h)) = bars.borrow().get(i).copied() else { return };
                    let at = ((y - top) / h.max(1.0)).clamp(0.0, 1.0);
                    let mut g = this.eq_gains;
                    // Steps of half a dB; near 0 it settles on 0.
                    let v = ((0.5 - at) * 2.0 * eq::RANGE * 2.0).round() / 2.0;
                    g[i] = if v.abs() < 0.75 { 0.0 } else { v };
                    this.set_eq(true, g);
                }
            };
            let pick_move = pick.clone();
            bands = bands.child(
                div()
                    .id(("eq-band", i))
                    .w(px(36.0))
                    .h(px(HEIGHT))
                    .relative()
                    .cursor_pointer()
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                        pick(this, f32::from(e.position.y));
                        cx.notify();
                    }))
                    .on_mouse_move(cx.listener(move |this, e: &gpui::MouseMoveEvent, _, cx| {
                        if e.pressed_button == Some(gpui::MouseButton::Left) {
                            pick_move(this, f32::from(e.position.y));
                            cx.notify();
                        }
                    }))
                    .child(div().absolute().left(px(16.0)).w(px(4.0)).top_0().bottom_0().rounded_full().bg(rgba(0xffffff1f)))
                    .child(div().absolute().left(px(10.0)).w(px(16.0)).top(px(half)).h(px(1.0)).bg(rgba(0xffffff40)))
                    .child(div().absolute().left(px(16.0)).w(px(4.0)).top(px(fill_top)).h(px(level.abs())).rounded_full().bg(if on { rgb(accent()) } else { rgba(0xffffff66) }))
                    .child(
                        div()
                            .absolute()
                            .left(px(10.0))
                            .top(px(half - level - 8.0))
                            .size(px(16.0))
                            .rounded_full()
                            .bg(rgb(0xffffff))
                            .shadow_md()
                            .when(!on, |d| d.opacity(0.5)),
                    ),
            );
        }
        let mut labels = div().flex().gap(px(18.0));
        for (i, freq) in eq::BANDS.iter().enumerate() {
            let name = if *freq >= 1000.0 { format!("{}k", freq / 1000.0) } else { format!("{}", *freq as u32) };
            labels = labels.child(
                div()
                    .w(px(36.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(div().text_size(px(11.0)).text_color(rgba(SECONDARY)).child(name))
                    .child(div().text_size(px(10.0)).text_color(rgba(TERTIARY)).child(format!("{:+.1}", gains[i]))),
            );
        }
        div()
            .px(px(16.0))
            .py(px(14.0))
            .flex()
            .items_center()
            .gap(px(32.0))
            .child(div().flex().flex_col().gap(px(8.0)).child(bands).child(labels))
            .child(div().flex_1().flex().flex_col().gap(px(10.0)).child(div().text_size(px(12.0)).text_color(rgba(SECONDARY)).child(t("Presets"))).child(presets))
            .into_any_element()
    }

    /// The playing song's lyric timing: nudged earlier or later, kept for the song.
    fn timing_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let playing = self.current().is_some();
        let offset = self.lyric_offset;
        let hint: SharedString = if playing {
            t("When the lyrics run ahead or behind this song. [ and ] do it while it plays").into()
        } else {
            t("Play a song to nudge its lyrics earlier or later ([ and ] while it plays)").into()
        };
        let step = |id: &'static str, label: &'static str, by: i64| {
            div()
                .id(id)
                .h(px(30.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .rounded(px(6.0))
                .text_size(px(12.0))
                .border_1()
                .border_color(rgba(STROKE))
                .bg(rgba(SUBTLE_STRONG))
                .map(|d| if playing { d.cursor_pointer().hover(|s| s.bg(rgba(0xffffff26))) } else { d.opacity(0.4) })
                .child(t(label))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if by == 0 {
                        let back = -this.lyric_offset;
                        this.nudge_lyrics(back);
                    } else {
                        this.nudge_lyrics(by);
                    }
                    cx.notify();
                }))
        };
        row(
            "Lyric timing",
            hint,
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(step("timing-earlier", "Earlier", -250))
                .child(div().w(px(64.0)).text_center().text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).child(format!("{:+.2} s", offset as f32 / 1000.0)))
                .child(step("timing-later", "Later", 250))
                .child(step("timing-reset", "Reset", 0)),
        )
    }

    /// Discord, the tray and the mini player.
    fn integrations_section(&self, cx: &mut Context<Self>) -> gpui::Div {
        section(
            "Windows and Discord",
            vec![
                row(
                    "Show on Discord",
                    t("Your Discord status says what you're listening to, with the cover and a link"),
                    switch("discord", self.discord_on).on_click(cx.listener(|this, _, _, cx| {
                        this.discord_on = !this.discord_on;
                        settings::set("discord", if this.discord_on { "1" } else { "0" });
                        cx.notify();
                    })),
                ),
                row(
                    "Keep running in the tray",
                    if self.tray.is_some() { t("The close button hides NabyTune by the clock instead of quitting; the music goes on") } else { t("The tray icon isn't available right now") },
                    switch("close-to-tray", self.close_to_tray).on_click(cx.listener(|this, _, _, cx| {
                        this.close_to_tray = !this.close_to_tray;
                        settings::set("close_to_tray", if this.close_to_tray { "1" } else { "0" });
                        cx.notify();
                    })),
                ),
                row(
                    "Mini player",
                    t("A small window that stays on top, with the song, the sung line and the buttons (Ctrl+M)"),
                    div()
                        .id("open-mini")
                        .h(px(32.0))
                        .px(px(14.0))
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(6.0))
                        .cursor_pointer()
                        .text_size(px(13.0))
                        .border_1()
                        .border_color(rgba(STROKE))
                        .bg(rgba(SUBTLE_STRONG))
                        .hover(|s| s.bg(rgba(0xffffff26)))
                        .child(div().font_family(ICONS).text_size(px(12.0)).child("\u{E8A7}"))
                        .child(t("Open"))
                        .on_click(cx.listener(|this, _, _, cx| this.open_mini_player(cx))),
                ),
                row(
                    "Lyrics on the desktop",
                    t("The sung line floats over everything, games and other apps too (Ctrl+D)"),
                    switch("desk-lyrics", self.desk_lyrics.is_some()).on_click(cx.listener(|this, _, _, cx| this.toggle_desktop_lyrics(cx))),
                ),
                row(
                    "Lock the desktop lyrics",
                    t("Locked, clicks go through them to what's underneath; unlock to move or size them"),
                    switch("desk-lock", self.desk_locked).on_click(cx.listener(|this, _, _, cx| {
                        let locked = !this.desk_locked;
                        this.set_desktop_lyrics_locked(locked, cx);
                    })),
                ),
                row(
                    "Behind the desktop lyrics",
                    t(match crate::desktop_lyrics::backdrop() {
                        crate::desktop_lyrics::Backdrop::None => "Just the words, straight on whatever is underneath",
                        crate::desktop_lyrics::Backdrop::Shadow => "A soft dark glow with no edges, so the words read on any wallpaper",
                        crate::desktop_lyrics::Backdrop::Glass => "A light see-through plate behind the words",
                        crate::desktop_lyrics::Backdrop::Dark => "A dark plate, for busy backgrounds",
                    }),
                    self.segmented(
                        "desk-backdrop",
                        crate::desktop_lyrics::Backdrop::ALL.iter().map(|b| (*b, SharedString::from(b.name()))).collect(),
                        crate::desktop_lyrics::backdrop(),
                        |this, backdrop, _, cx| this.set_desktop_lyrics_backdrop(backdrop, cx),
                        cx,
                    ),
                ),
                row(
                    "Desktop lyrics alignment",
                    t("Left or right keeps the words to one side, so they can sit in a corner of the screen"),
                    self.segmented(
                        "desk-align",
                        crate::desktop_lyrics::Align::ALL.iter().map(|a| (*a, SharedString::from(a.name()))).collect(),
                        crate::desktop_lyrics::align(),
                        |this, align, _, cx| this.set_desktop_lyrics_align(align, cx),
                        cx,
                    ),
                ),
                row(
                    "Desktop lyrics opacity",
                    t("They stay on top of every window; fainter words get in the way less"),
                    self.segmented(
                        "desk-opacity",
                        crate::desktop_lyrics::OPACITIES.iter().map(|p| (*p, SharedString::from(format!("{p}%")))).collect(),
                        crate::desktop_lyrics::opacity(),
                        |this, percent, _, cx| this.set_desktop_lyrics_opacity(percent, cx),
                        cx,
                    ),
                ),
                row(
                    "Border round the desktop lyrics",
                    t("A thin line round the plate"),
                    switch("desk-border", crate::desktop_lyrics::bordered()).on_click(cx.listener(|this, _, _, cx| {
                        let on = !crate::desktop_lyrics::bordered();
                        this.set_desktop_lyrics_border(on, cx);
                    })),
                ),
            ],
        )
    }

    /// Every keyboard shortcut, in two columns.
    fn shortcuts_section(&self) -> gpui::Div {
        let mut grid = div().px(px(16.0)).py(px(12.0)).flex().flex_wrap();
        for (keys, what) in crate::SHORTCUTS {
            grid = grid.child(
                div()
                    .w(gpui::relative(0.5))
                    .h(px(34.0))
                    .pr_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_none()
                            .min_w(px(96.0))
                            .flex()
                            .child(div().px(px(8.0)).py(px(2.0)).rounded(px(5.0)).border_1().border_color(rgba(STROKE)).bg(rgba(SUBTLE_STRONG)).text_size(px(12.0)).font_weight(FontWeight::SEMIBOLD).child(t(keys))),
                    )
                    .child(div().text_size(px(13.0)).text_color(rgba(SECONDARY)).truncate().child(t(what))),
            );
        }
        section("Keyboard shortcuts", vec![grid.into_any_element()])
    }

    fn about_section(&self, cx: &mut Context<Self>) -> gpui::Div {
        let folder = std::env::var_os("APPDATA").map(|d| std::path::PathBuf::from(d).join("NabyTune"));
        let (update_hint, update_control) = self.update_control(cx);
        section(
            "About",
            vec![
                row(
                    "NabyTune",
                    tf("Version {} · native build (GPUI, Rust audio engine)", &[&env!("CARGO_PKG_VERSION")]),
                    div().text_size(px(12.0)).text_color(rgba(TERTIARY)).child(t("YouTube Music, your way")),
                ),
                row("Updates", update_hint, update_control),
                // GPL v3 asks for these to show in the app: where it comes from, the licence,
                // no warranty, and where the source is.
                row(
                    "Credits",
                    t("Inspired by and built on BitChord, © Kushagra Singh and its contributors. Changed into NabyTune by BillyNabil, since 2026."),
                    link_button("open-bitchord", "BitChord", BITCHORD, cx),
                ),
                row(
                    "Licence",
                    t("GNU General Public License v3: you may share and change NabyTune under its terms. It comes with no warranty of any kind. The source code is available on request."),
                    link_button("open-licence", "Read the licence", GPL, cx),
                ),
                row(
                    "Settings file",
                    folder.as_ref().map_or("%APPDATA%\\NabyTune".to_string(), |f| f.display().to_string()),
                    div()
                        .id("open-folder")
                        .h(px(32.0))
                        .px(px(14.0))
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(6.0))
                        .cursor_pointer()
                        .text_size(px(13.0))
                        .border_1()
                        .border_color(rgba(STROKE))
                        .bg(rgba(SUBTLE_STRONG))
                        .hover(|s| s.bg(rgba(0xffffff26)))
                        .child(div().font_family(ICONS).text_size(px(12.0)).child("\u{E838}"))
                        .child(t("Open folder"))
                        .on_click(cx.listener(move |_, _, _, _| {
                            if let Some(folder) = &folder {
                                let _ = std::process::Command::new("explorer").arg(folder).spawn();
                            }
                        })),
                ),
            ],
        )
    }
}
