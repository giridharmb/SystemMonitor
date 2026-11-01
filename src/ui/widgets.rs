//! Shared building blocks: cards, stat tiles and thin wrappers that set our chart defaults.

use crate::util::{HISTORY, Ring};
use gpui_kit::component::chart::{AreaChart, BarChart, PieChart};
use gpui_kit::component::plot::shape::BarAlignment;
use gpui_kit::component::{ActiveTheme, StyledExt as _, h_flex, v_flex};
use gpui_kit::*;
use std::rc::Rc;

pub const BLUE: u32 = 0x4f9dff;
pub const VIOLET: u32 = 0xa78bfa;
pub const TEAL: u32 = 0x2dd4bf;
pub const AMBER: u32 = 0xfbbf24;
pub const PINK: u32 = 0xf472b6;
pub const GREEN: u32 = 0x4ade80;
pub const RED: u32 = 0xf87171;
pub const GRAY: u32 = 0x6b7280;

pub fn col(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Green to amber to red by how loaded a 0-100 value is.
pub fn load_color(pct: f32) -> Hsla {
    col(if pct >= 85.0 {
        RED
    } else if pct >= 60.0 {
        AMBER
    } else {
        GREEN
    })
}

pub fn card(title: impl Into<SharedString>, cx: &App) -> Div {
    v_flex()
        .gap_2()
        .p_3()
        .min_w_0()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary.opacity(0.35))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
                .child(title.into()),
        )
}

/// A card whose header also shows the current reading on the right.
pub fn chart_card(
    title: impl Into<SharedString>,
    value: impl Into<SharedString>,
    color: Hsla,
    cx: &App,
) -> Div {
    v_flex()
        .gap_1()
        .p_3()
        .min_w_0()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary.opacity(0.35))
        .child(
            h_flex()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(color)
                        .child(value.into()),
                ),
        )
}

pub fn stat(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    sub: impl Into<SharedString>,
    color: Hsla,
    cx: &App,
) -> Div {
    h_flex()
        .gap_2()
        .p_2()
        .min_w_0()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary.opacity(0.35))
        .child(div().w(px(3.)).h(px(40.)).rounded_sm().bg(color))
        .child(
            v_flex()
                .min_w_0()
                .child(
                    div()
                        .text_xs()
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .child(label.into()),
                )
                .child(
                    div()
                        .text_lg()
                        .font_semibold()
                        .truncate()
                        .child(value.into()),
                )
                .child(
                    div()
                        .text_xs()
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .child(sub.into()),
                ),
        )
}

pub fn grid(cols: usize) -> Div {
    div().grid().grid_cols(cols.max(1) as u16).gap_3().w_full()
}

pub fn section(title: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_sm()
        .font_semibold()
        .text_color(cx.theme().foreground)
        .pt_1()
        .child(title.into())
}

pub struct Series<'a> {
    pub name: &'static str,
    pub color: u32,
    pub ring: &'a Ring,
}

pub fn s<'a>(name: &'static str, color: u32, ring: &'a Ring) -> Series<'a> {
    Series { name, color, ring }
}

fn gradient(color: Hsla, strength: f32) -> Background {
    linear_gradient(
        0.,
        linear_color_stop(color.opacity(strength), 1.),
        linear_color_stop(color.opacity(0.02), 0.),
    )
}

/// Time-series chart over the whole history. `top` pins the y axis (use it for percentages);
/// `None` scales to the peak in view. `appear(false)` matters: without it the draw-in animation
/// would run the renderer at 120 Hz instead of once per sample.
pub fn timeline(
    id: impl Into<ElementId>,
    series: &[Series],
    top: Option<f64>,
    fmt: fn(f64) -> String,
    height: f32,
) -> impl IntoElement {
    let peak = series.iter().map(|x| x.ring.max()).fold(0.0, f32::max) as f64;
    let top = top.unwrap_or((peak * 1.15).max(1.0));
    let mut chart = AreaChart::new(0..HISTORY)
        .id(id)
        .appear(false)
        .x(|i: &usize| -> SharedString {
            let ago = HISTORY - 1 - *i;
            if ago == 0 {
                "now".into()
            } else {
                format!("-{ago}s").into()
            }
        })
        .x_tick_count(4)
        .y_domain(0.0, top)
        .y_axis(true)
        .y_tick_count(3)
        .y_tick_format(fmt)
        .y_padding(0., 0.);
    for x in series {
        let data: Rc<[f32]> = x.ring.tail(HISTORY).into();
        let color = col(x.color);
        chart = chart
            .y(move |i: &usize| data[*i] as f64)
            .stroke(color)
            .fill(gradient(color, if series.len() > 1 { 0.18 } else { 0.35 }))
            .linear()
            .name(x.name);
    }
    div().w_full().h(px(height)).child(chart)
}

/// Axis-free, non-interactive sparkline for dense grids. One minute of history keeps paths short.
pub fn spark(
    id: impl Into<ElementId>,
    ring: &Ring,
    top: f64,
    color: Hsla,
    height: f32,
) -> impl IntoElement {
    const POINTS: usize = 60;
    let data: Rc<[f32]> = ring.tail(POINTS).into();
    let chart = AreaChart::new(0..POINTS)
        .id(id)
        .appear(false)
        .interactive(false)
        .grid(false)
        .x_axis(false)
        .x(|_: &usize| SharedString::default())
        .y_domain(0.0, top)
        .y_padding(1., 0.)
        .y(move |i: &usize| data[*i] as f64)
        .stroke(color)
        .fill(gradient(color, 0.4))
        .linear();
    div().w_full().h(px(height)).child(chart)
}

/// Horizontal ranking: name, value, color.
pub fn ranking(
    id: impl Into<ElementId>,
    rows: Vec<(String, f64, Hsla)>,
    fmt: fn(f64) -> String,
    height: f32,
) -> impl IntoElement {
    let chart = BarChart::new(rows)
        .id(id)
        .appear(false)
        .alignment(BarAlignment::Left)
        .band(|d: &(String, f64, Hsla)| SharedString::from(d.0.clone()))
        .value(|d| d.1)
        .fill(|d, _, _, _| d.2)
        .label(move |d| fmt(d.1))
        .corner_radii(px(3.))
        .padding_inner(0.25)
        .grid(false);
    div().w_full().h(px(height)).child(chart)
}

/// Vertical per-item bars with a percentage axis (the library scales to the visible peak).
pub fn columns(
    id: impl Into<ElementId>,
    rows: Vec<(String, f64, Hsla)>,
    height: f32,
) -> impl IntoElement {
    let chart = BarChart::new(rows)
        .id(id)
        .appear(false)
        .band(|d: &(String, f64, Hsla)| SharedString::from(d.0.clone()))
        .value(|d| d.1)
        .fill(|d, _, _, _| d.2)
        .corner_radii(px(3.))
        .padding_inner(0.3)
        .value_axis(true)
        .value_tick_count(3)
        .value_tick_format(|v| format!("{v:.0}%"));
    div().w_full().h(px(height)).child(chart)
}

/// Donut with a legend beside it. Slices: name, value, color, formatted value.
pub fn donut(
    id: impl Into<ElementId>,
    rows: Vec<(&'static str, f32, Hsla, String)>,
    cx: &App,
) -> impl IntoElement {
    let legend = rows.clone();
    h_flex()
        .gap_4()
        .items_center()
        .child(
            div().size(px(170.)).flex_none().child(
                PieChart::new(rows)
                    .id(id)
                    .appear(false)
                    .value(|d: &(&'static str, f32, Hsla, String)| d.1)
                    .color(|d| d.2)
                    .inner_radius(50.)
                    .outer_radius(80.)
                    .pad_angle(0.03)
                    .tooltip_name(|d| SharedString::from(d.0))
                    .tooltip_value(|d, _, _| SharedString::from(d.3.clone())),
            ),
        )
        .child(
            v_flex()
                .gap_1()
                .flex_1()
                .min_w_0()
                .children(legend.into_iter().map(|d| {
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .child(div().size_2().rounded_sm().bg(d.2))
                        .child(
                            div()
                                .flex_1()
                                .text_color(cx.theme().muted_foreground)
                                .child(d.0),
                        )
                        .child(d.3)
                })),
        )
}

pub fn pct(v: f64) -> String {
    format!("{v:.0}%")
}
pub fn bytes_fmt(v: f64) -> String {
    crate::util::bytes(v.max(0.0) as u64)
}
pub fn rate_fmt(v: f64) -> String {
    crate::util::rate(v as f32)
}
pub fn watts(v: f64) -> String {
    format!("{v:.1} W")
}
pub fn mhz(v: f64) -> String {
    format!("{v:.0} MHz")
}
pub fn count(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.1}k", v / 1000.0)
    } else {
        format!("{v:.0}")
    }
}
pub fn ms(v: f64) -> String {
    format!("{v:.1} ms")
}

/// An unsupported source must not be confused with a measured idle device.
pub fn reading(available: bool, value: String) -> String {
    if available {
        value
    } else {
        "Unavailable".into()
    }
}
