//! One render method per tab. Only the visible tab's charts are ever built.

use super::Monitor;
use super::table::ProcTable;
use super::widgets::*;
use crate::logs::Source;
use crate::sampler::ProcRow;
use crate::util::{bytes, duration, rate};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::chart::RadarChart;
use gpui_kit::component::input::Input;
use gpui_kit::component::progress::{Progress, ProgressCircle};
use gpui_kit::component::table::DataTable;
use gpui_kit::component::{
    ActiveTheme, Selectable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

const TALL: f32 = 190.;
const SHORT: f32 = 130.;

fn columns_for(width: f32, min: f32) -> usize {
    ((width - 24.) / min).floor().max(1.) as usize
}

fn top<K: PartialOrd>(procs: &[ProcRow], n: usize, key: impl Fn(&ProcRow) -> K) -> Vec<&ProcRow> {
    let mut v: Vec<&ProcRow> = procs.iter().collect();
    v.sort_by(|a, b| {
        key(b)
            .partial_cmp(&key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(n);
    v
}

fn thermal_name(level: u64) -> (&'static str, u32) {
    match level {
        0 => ("Nominal", GREEN),
        1 => ("Moderate", AMBER),
        2 => ("Heavy", RED),
        3 => ("Trapping", RED),
        _ => ("Sleeping", RED),
    }
}

fn pressure_name(level: u64) -> (&'static str, u32) {
    match level {
        4 => ("Critical", RED),
        2 => ("Warning", AMBER),
        _ => ("Normal", GREEN),
    }
}

fn kv(k: impl Into<SharedString>, v: impl Into<SharedString>, cx: &App) -> Div {
    h_flex()
        .justify_between()
        .gap_3()
        .text_xs()
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(k.into()),
        )
        .child(div().truncate().child(v.into()))
}

fn cluster_label(raw: &str) -> String {
    match raw {
        "ECPU" => "Efficiency".into(),
        "PCPU" => "Performance 0".into(),
        "PCPU1" => "Performance 1".into(),
        o => o.to_string(),
    }
}

impl Monitor {
    pub fn overview(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let score = self.health.score as f32;
        let color = col(if score >= 80.0 {
            GREEN
        } else if score >= 50.0 {
            AMBER
        } else {
            RED
        });
        let health = card("Resource health · heuristic", cx).child(
            h_flex()
                .gap_4()
                .items_center()
                .child(
                    ProgressCircle::new("health")
                        .value(score)
                        .color(color)
                        .size(px(96.))
                        .flex_none()
                        .child(div().size_full().flex().items_center().justify_center().text_2xl().font_semibold().text_color(color).child(self.health.score.to_string())),
                )
                .child(v_flex().gap_1().flex_1().min_w_0().map(|d| {
                    if self.health.issues.is_empty() {
                        d.child(div().text_sm().child("No problems detected")).child(div().text_xs().text_color(cx.theme().muted_foreground).child("No thresholds crossed in the available readings. This score is a guide, not a hardware diagnostic."))
                    } else {
                        d.children(self.health.issues.iter().map(|i| {
                            let c = col(if i.severity == crate::health::Severity::Critical { RED } else { AMBER });
                            h_flex().gap_2().text_xs().child(div().size_2().flex_none().rounded_full().bg(c)).child(i.text.clone())
                        }))
                    }
                })),
        );

        let s = &self.s;
        let radar_rows = vec![
            ("CPU", self.h.cpu.last() as f64),
            ("Memory", self.h.mem_used.last() as f64),
            ("GPU", self.h.gpu.last() as f64),
            (
                "Swap",
                if s.mem.swap_total > 0 {
                    s.mem.swap_used as f64 / s.mem.swap_total as f64 * 100.0
                } else {
                    0.0
                },
            ),
            (
                "Disk full",
                self.volumes
                    .iter()
                    .find(|v| v.mount == "/")
                    .map_or(0.0, |v| {
                        (1.0 - v.free as f64 / v.total.max(1) as f64) * 100.0
                    }),
            ),
            ("Thermal", s.thermal as f64 * 25.0),
        ];
        let radar = card("Resource balance", cx).child(
            div().h(px(150.)).w_full().child(
                RadarChart::new(radar_rows)
                    .id("radar")
                    .appear(false)
                    .value(|d: &(&'static str, f64)| d.1)
                    .label(|d| SharedString::from(d.0))
                    .max_value(100.0)
                    .outer_radius(58.)
                    .stroke(col(BLUE))
                    .fill(col(BLUE).opacity(0.25))
                    .dot(),
            ),
        );

        let system = card("This Mac", cx)
            .child(kv(
                "Chip",
                format!("{} · {} GPU cores", self.info.cpu_name, self.info.gpu_cores),
                cx,
            ))
            .child(kv("Model", self.info.model.clone(), cx))
            .child(kv("OS", self.info.os.clone(), cx))
            .child(kv("Host", self.info.host.clone(), cx))
            .child(kv("Uptime", duration(s.uptime), cx))
            .child(kv(
                "Load",
                format!("{:.2} {:.2} {:.2}", s.load[0], s.load[1], s.load[2]),
                cx,
            ))
            .child(kv("Processes", self.procs.len().to_string(), cx));

        let cols = columns_for(width, 340.).min(3);
        let cards = [
            chart_card("CPU", format!("{:.0}%", self.h.cpu.last()), col(BLUE), cx).child(timeline(
                "o-cpu",
                &[s_("Total", BLUE, &self.h.cpu)],
                Some(100.),
                pct,
                SHORT,
            )),
            chart_card(
                "Memory",
                format!("{:.0}%", self.h.mem_used.last()),
                col(VIOLET),
                cx,
            )
            .child(timeline(
                "o-mem",
                &[s_("Used", VIOLET, &self.h.mem_used)],
                Some(100.),
                pct,
                SHORT,
            )),
            chart_card(
                "GPU",
                reading(s.gpu.available, format!("{:.0}%", self.h.gpu.last())),
                col(TEAL),
                cx,
            )
            .child(timeline(
                "o-gpu",
                &[s_("Device", TEAL, &self.h.gpu)],
                Some(100.),
                pct,
                SHORT,
            )),
            chart_card(
                "Power",
                reading(
                    s.power.is_some(),
                    format!("{:.1} W", self.h.power_total.last()),
                ),
                col(AMBER),
                cx,
            )
            .child(timeline(
                "o-pow",
                &[s_("SoC + DRAM", AMBER, &self.h.power_total)],
                None,
                watts,
                SHORT,
            )),
            chart_card(
                "Disk",
                format!("↓ {} ↑ {}", rate(s.disk.read), rate(s.disk.write)),
                col(PINK),
                cx,
            )
            .child(timeline(
                "o-disk",
                &[
                    s_("Read", PINK, &self.h.disk_read),
                    s_("Write", AMBER, &self.h.disk_write),
                ],
                None,
                rate_fmt,
                SHORT,
            )),
            chart_card(
                "Network",
                format!(
                    "↓ {} ↑ {}",
                    rate(self.h.net_rx.last()),
                    rate(self.h.net_tx.last())
                ),
                col(GREEN),
                cx,
            )
            .child(timeline(
                "o-net",
                &[
                    s_("In", GREEN, &self.h.net_rx),
                    s_("Out", BLUE, &self.h.net_tx),
                ],
                None,
                rate_fmt,
                SHORT,
            )),
        ];

        let rows: Vec<_> = top(&self.procs, 6, |p| p.cpu)
            .into_iter()
            .map(|p| (p.app.clone(), p.cpu as f64, load_color(p.cpu / 2.0)))
            .collect();
        let crashes = card(
            format!("Crash reports, last 7 days ({})", self.crashes.len()),
            cx,
        )
        .map(|c| {
            if self.crashes.is_empty() {
                c.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("None"),
                )
            } else {
                c.children(
                    self.crashes
                        .iter()
                        .take(6)
                        .map(|x| kv(x.name.clone(), format!("{} ago", duration(x.age_secs)), cx)),
                )
            }
        });

        v_flex()
            .gap_3()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "Live · 1 second counters · 3 minute history · GPU {} · Power {} · Logs {}",
                        if s.gpu.available {
                            "available"
                        } else {
                            "unavailable"
                        },
                        if s.power.is_some() {
                            "available"
                        } else {
                            "unavailable"
                        },
                        if !self.log.capturing {
                            "off (open Logs)"
                        } else if self.log.status.is_none() {
                            "streaming"
                        } else {
                            "unavailable"
                        }
                    )),
            )
            .child(
                grid(columns_for(width, 340.).min(3))
                    .child(health)
                    .child(radar)
                    .child(system),
            )
            .child(grid(cols).children(cards))
            .child(
                grid(columns_for(width, 480.).min(2))
                    .child(card("Top processes by CPU", cx).child(ranking(
                        "o-top",
                        rows,
                        |v| format!("{v:.0}%"),
                        170.,
                    )))
                    .child(crashes),
            )
    }

    pub fn cpu(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.s;
        let total = self.h.cpu.last();
        let sys = self.h.cpu_sys.last();
        let power = s.power.as_ref();
        let tiles = grid(columns_for(width, 190.).min(5))
            .child(stat(
                "Total",
                format!("{total:.1}%"),
                format!("{} logical cores", self.info.core_kinds.len()),
                load_color(total),
                cx,
            ))
            .child(stat(
                "User",
                format!("{:.1}%", total - sys),
                "includes nice",
                col(BLUE),
                cx,
            ))
            .child(stat(
                "System",
                format!("{sys:.1}%"),
                "kernel time",
                col(RED),
                cx,
            ))
            .child(stat(
                "Load average",
                format!("{:.2}", s.load[0]),
                format!("5m {:.2} · 15m {:.2}", s.load[1], s.load[2]),
                col(VIOLET),
                cx,
            ))
            .child(stat(
                "Package power",
                power.map_or("Unavailable".into(), |p| format!("{:.1} W", p.cpu_w)),
                "CPU clusters",
                col(AMBER),
                cx,
            ));

        let per_core: Vec<_> = self
            .h
            .cores
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let kind = self.info.core_kinds.get(i).copied().unwrap_or('C');
                (
                    format!("{kind}{i}"),
                    r.last() as f64,
                    col(if kind == 'E' { TEAL } else { BLUE }),
                )
            })
            .collect();

        let clusters = power.map(|p| p.clusters.clone()).unwrap_or_default();
        let cluster_tiles = grid(columns_for(width, 190.)).children(clusters.iter().map(|c| {
            stat(
                cluster_label(&c.name),
                format!("{:.0} MHz", c.mhz),
                format!("{:.0}% active", c.active),
                col(if c.name.starts_with('E') { TEAL } else { BLUE }),
                cx,
            )
        }));

        let freq_series: Vec<Series> = self
            .h
            .clusters
            .iter()
            .enumerate()
            .map(|(i, (n, r))| {
                let name = match n.as_str() {
                    "ECPU" => "Efficiency",
                    "PCPU" => "Performance 0",
                    _ => "Performance 1",
                };
                s_(name, [TEAL, BLUE, VIOLET][i.min(2)], r)
            })
            .collect();

        let core_group = |kind: char, title: &'static str, color: u32| {
            let tiles: Vec<_> =
                self.h
                    .cores
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| self.info.core_kinds.get(*i) == Some(&kind))
                    .map(|(i, ring)| {
                        let v = ring.last();
                        chart_card(format!("Core {i}"), format!("{v:.0}%"), load_color(v), cx)
                            .child(spark(
                                ElementId::Name(format!("core-{i}").into()),
                                ring,
                                100.,
                                col(color),
                                44.,
                            ))
                    })
                    .collect();
            (!tiles.is_empty()).then(|| {
                v_flex()
                    .gap_2()
                    .child(section(title, cx))
                    .child(grid(columns_for(width, 190.)).children(tiles))
            })
        };

        let rows: Vec<_> = top(&self.procs, 8, |p| p.cpu)
            .into_iter()
            .map(|p| (p.name.clone(), p.cpu as f64, load_color(p.cpu / 2.0)))
            .collect();

        v_flex()
            .gap_3()
            .child(tiles)
            .child(
                chart_card(
                    "CPU usage, user + system (red)",
                    format!("{total:.1}%"),
                    load_color(total),
                    cx,
                )
                .child(timeline(
                    "c-total",
                    &[
                        s_("Total", BLUE, &self.h.cpu),
                        s_("System", RED, &self.h.cpu_sys),
                    ],
                    Some(100.),
                    pct,
                    TALL,
                )),
            )
            .child(
                card("Usage per core now (E efficiency, P performance)", cx)
                    .child(columns("c-bars", per_core, 150.)),
            )
            .children(core_group('P', "Performance cores", BLUE))
            .children(core_group('E', "Efficiency cores", TEAL))
            .children(core_group('C', "Cores", BLUE))
            .child(cluster_tiles)
            .when(!freq_series.is_empty(), |d| {
                d.child(
                    chart_card(
                        "Effective cluster frequency (clock × active time)",
                        "",
                        col(BLUE),
                        cx,
                    )
                    .child(timeline("c-freq", &freq_series, None, mhz, TALL)),
                )
            })
            .child(card("Top processes (100% = one core)", cx).child(ranking(
                "c-top",
                rows,
                |v| format!("{v:.0}%"),
                210.,
            )))
    }

    pub fn gpu(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let (g, p) = (&self.s.gpu, self.s.power.as_ref());
        let cols = columns_for(width, 480.).min(2);
        v_flex()
            .gap_3()
            .child(
                grid(columns_for(width, 190.))
                    .child(stat("Device", reading(g.available, format!("{:.0}%", g.device)), format!("{} GPU cores", self.info.gpu_cores), load_color(g.device), cx))
                    .child(stat("Renderer", reading(g.available, format!("{:.0}%", g.renderer)), "fragment work", col(VIOLET), cx))
                    .child(stat("Tiler", reading(g.available, format!("{:.0}%", g.tiler)), "geometry work", col(PINK), cx))
                    .child(stat("GPU memory", reading(g.available, bytes(g.mem_used)), format!("{} allocated", bytes(g.mem_alloc)), col(TEAL), cx))
                    .child(stat("Clock", p.map_or("Unavailable".into(), |p| format!("{:.0} MHz", p.gpu_mhz)), "average while active", col(BLUE), cx))
                    .child(stat("Power", p.map_or("Unavailable".into(), |p| format!("{:.1} W", p.gpu_w)), p.map_or("No power counters".into(), |p| format!("Neural Engine {:.1} W", p.ane_w)), col(AMBER), cx)),
            )
            .when(!g.available, |d| d.child(div().text_sm().text_color(col(AMBER)).child("GPU counters unavailable on this Mac. The charts below have no measurements.")))
            .child(chart_card("GPU utilization", reading(g.available, format!("{:.0}%", g.device)), load_color(g.device), cx).child(timeline("g-util", &[s_("Device", TEAL, &self.h.gpu), s_("Renderer", VIOLET, &self.h.gpu_renderer), s_("Tiler", PINK, &self.h.gpu_tiler)], Some(100.), pct, TALL)))
            .child(
                grid(cols)
                    .child(chart_card("GPU clock", "", col(BLUE), cx).child(timeline("g-clk", &[s_("Clock", BLUE, &self.h.gpu_mhz)], None, mhz, SHORT)))
                    .child(chart_card("GPU power", "", col(AMBER), cx).child(timeline("g-pow", &[s_("GPU", AMBER, &self.h.power_gpu), s_("Neural Engine", GREEN, &self.h.power_ane)], None, watts, SHORT)))
                    .child(chart_card("GPU memory in use", bytes(g.mem_used), col(TEAL), cx).child(timeline("g-mem", &[s_("In use", TEAL, &self.h.gpu_mem)], None, bytes_fmt, SHORT))),
            )
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!(
                "macOS schedules the {} GPU cores as one device and publishes no per-core counters, so utilization is shown per pipeline stage instead.",
                self.info.gpu_cores
            )))
    }

    pub fn memory(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let m = &self.s.mem;
        let (p_name, p_color) = pressure_name(m.pressure);
        let slice = |n, v: u64, c| (n, v as f32, col(c), bytes(v));
        let composition = vec![
            slice("App memory", m.vm.app, BLUE),
            slice("Wired", m.vm.wired, AMBER),
            slice("Compressed", m.vm.compressed, PINK),
            slice("Cached files", m.vm.cached, TEAL),
            slice("Free", m.free, GRAY),
        ];
        let cols = columns_for(width, 480.).min(2);
        let rows: Vec<_> = top(&self.procs, 8, |p| p.mem)
            .into_iter()
            .map(|p| (p.name.clone(), p.mem as f64, col(VIOLET)))
            .collect();
        v_flex()
            .gap_3()
            .child(
                grid(columns_for(width, 190.).min(5))
                    .child(stat(
                        "Used",
                        bytes(m.used()),
                        format!("of {}", bytes(m.total)),
                        col(VIOLET),
                        cx,
                    ))
                    .child(stat(
                        "Pressure",
                        p_name,
                        format!("kernel reports {}% free", m.free_pct),
                        col(p_color),
                        cx,
                    ))
                    .child(stat(
                        "Swap",
                        bytes(m.swap_used),
                        format!(
                            "of {} · in {:.0}/s out {:.0}/s",
                            bytes(m.swap_total),
                            m.swapin,
                            m.swapout
                        ),
                        col(if m.swap_used > 0 { AMBER } else { GREEN }),
                        cx,
                    ))
                    .child(stat(
                        "Page outs",
                        format!("{:.0}/s", m.pageout),
                        format!("page ins {:.0}/s", m.pagein),
                        col(RED),
                        cx,
                    ))
                    .child(stat(
                        "Compressions",
                        format!("{:.0}/s", m.compress),
                        format!("decompressions {:.0}/s", m.decompress),
                        col(PINK),
                        cx,
                    )),
            )
            .child(
                grid(cols)
                    .child(card("Composition", cx).child(donut("m-donut", composition, cx)))
                    .child(
                        chart_card(
                            "Memory used",
                            format!("{:.0}%", self.h.mem_used.last()),
                            col(VIOLET),
                            cx,
                        )
                        .child(timeline(
                            "m-used",
                            &[s_("Used", VIOLET, &self.h.mem_used)],
                            Some(100.),
                            pct,
                            170.,
                        )),
                    ),
            )
            .child(
                chart_card("Breakdown over time", "", col(BLUE), cx).child(timeline(
                    "m-parts",
                    &[
                        s_("App", BLUE, &self.h.mem_app),
                        s_("Wired", AMBER, &self.h.mem_wired),
                        s_("Compressed", PINK, &self.h.mem_compressed),
                        s_("Swap", RED, &self.h.swap),
                    ],
                    None,
                    bytes_fmt,
                    TALL,
                )),
            )
            .child(
                grid(cols)
                    .child(
                        chart_card("Paging", "pages/s", col(RED), cx).child(timeline(
                            "m-page",
                            &[
                                s_("Page in", TEAL, &self.h.page_in),
                                s_("Page out", RED, &self.h.page_out),
                            ],
                            None,
                            count,
                            SHORT,
                        )),
                    )
                    .child(
                        chart_card("Compressor", "pages/s", col(PINK), cx).child(timeline(
                            "m-comp",
                            &[
                                s_("Compress", PINK, &self.h.compress),
                                s_("Decompress", BLUE, &self.h.decompress),
                            ],
                            None,
                            count,
                            SHORT,
                        )),
                    ),
            )
            .child(
                card("Top processes by footprint", cx)
                    .child(ranking("m-top", rows, bytes_fmt, 210.)),
            )
    }

    pub fn disk(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let d = self.s.disk;
        let cols = columns_for(width, 480.).min(2);
        let volumes =
            card("Volumes", cx).children(self.volumes.iter().enumerate().map(|(i, v)| {
                let used = (1.0 - v.free as f32 / v.total.max(1) as f32) * 100.0;
                let flags = [(v.read_only, " · read-only"), (v.removable, " · removable")]
                    .iter()
                    .filter(|f| f.0)
                    .map(|f| f.1)
                    .collect::<String>();
                v_flex()
                    .gap_1()
                    .child(kv(
                        format!(
                            "{}  ({})",
                            if v.name.is_empty() {
                                "Untitled"
                            } else {
                                &v.name
                            },
                            v.mount
                        ),
                        format!(
                            "{} free of {} · {}{flags}",
                            bytes(v.free),
                            bytes(v.total),
                            v.fs
                        ),
                        cx,
                    ))
                    .child(
                        Progress::new(ElementId::NamedInteger("vol".into(), i as u64))
                            .value(used)
                            .color(load_color(used))
                            .h_2(),
                    )
            }));
        let reads: Vec<_> = top(&self.procs, 8, |p| p.read_bps + p.write_bps)
            .into_iter()
            .filter(|p| p.read_bps + p.write_bps > 0.0)
            .map(|p| (p.name.clone(), (p.read_bps + p.write_bps) as f64, col(PINK)))
            .collect();
        v_flex()
            .gap_3()
            .child(
                grid(columns_for(width, 190.).min(5))
                    .child(stat(
                        "Read",
                        rate(d.read),
                        format!("{:.0} IOPS", d.read_iops),
                        col(PINK),
                        cx,
                    ))
                    .child(stat(
                        "Write",
                        rate(d.write),
                        format!("{:.0} IOPS", d.write_iops),
                        col(AMBER),
                        cx,
                    ))
                    .child(stat(
                        "Read latency",
                        format!("{:.2} ms", d.read_ms),
                        "per operation",
                        col(TEAL),
                        cx,
                    ))
                    .child(stat(
                        "Write latency",
                        format!("{:.2} ms", d.write_ms),
                        "per operation",
                        col(BLUE),
                        cx,
                    ))
                    .child(stat(
                        "I/O errors",
                        d.errors.to_string(),
                        "since boot",
                        col(if d.errors > 0 { RED } else { GREEN }),
                        cx,
                    )),
            )
            .child(volumes)
            .child(chart_card("Throughput", "", col(PINK), cx).child(timeline(
                "d-tp",
                &[
                    s_("Read", PINK, &self.h.disk_read),
                    s_("Write", AMBER, &self.h.disk_write),
                ],
                None,
                rate_fmt,
                TALL,
            )))
            .child(
                grid(cols)
                    .child(chart_card("IOPS", "", col(BLUE), cx).child(timeline(
                        "d-iops",
                        &[
                            s_("Read", TEAL, &self.h.iops_read),
                            s_("Write", BLUE, &self.h.iops_write),
                        ],
                        None,
                        count,
                        SHORT,
                    )))
                    .child(chart_card("Latency", "", col(TEAL), cx).child(timeline(
                        "d-lat",
                        &[
                            s_("Read", TEAL, &self.h.lat_read),
                            s_("Write", BLUE, &self.h.lat_write),
                        ],
                        None,
                        ms,
                        SHORT,
                    ))),
            )
            .when(!reads.is_empty(), |c| {
                c.child(
                    card("Top processes by disk I/O", cx)
                        .child(ranking("d-top", reads, rate_fmt, 210.)),
                )
            })
    }

    pub fn network(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let mut ifs: Vec<_> = self
            .s
            .net
            .iter()
            .filter(|n| n.total_rx + n.total_tx > 0)
            .collect();
        ifs.sort_by_key(|n| std::cmp::Reverse(n.total_rx + n.total_tx));
        let interfaces = card("Interfaces", cx).children(ifs.iter().map(|n| {
            h_flex()
                .gap_3()
                .text_xs()
                .child(div().w(px(90.)).font_semibold().child(n.name.clone()))
                .child(
                    div()
                        .flex_1()
                        .text_color(col(GREEN))
                        .child(format!("↓ {}", rate(n.rx))),
                )
                .child(
                    div()
                        .flex_1()
                        .text_color(col(BLUE))
                        .child(format!("↑ {}", rate(n.tx))),
                )
                .child(
                    div()
                        .flex_1()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "total ↓ {} ↑ {}",
                            bytes(n.total_rx),
                            bytes(n.total_tx)
                        )),
                )
                .child(
                    div()
                        .w(px(70.))
                        .text_color(col(if n.errors > 0 { RED } else { GRAY }))
                        .child(format!("{} errors", n.errors)),
                )
        }));
        let ports = card(format!("Listening TCP ports ({})", self.ports.len()), cx).map(|c| {
            if self.ports.is_empty() {
                c.child(div().text_xs().text_color(cx.theme().muted_foreground).child("Scanning… refreshes every 15 s. Only your own processes are visible without root."))
            } else {
                c.children(self.ports.iter().map(|(cmd, pid, addr)| kv(format!("{cmd} ({pid})"), addr.clone(), cx)))
            }
        });
        v_flex()
            .gap_3()
            .child(
                grid(columns_for(width, 240.).min(2))
                    .child(stat(
                        "Download",
                        rate(self.h.net_rx.last()),
                        "all links except loopback",
                        col(GREEN),
                        cx,
                    ))
                    .child(stat(
                        "Upload",
                        rate(self.h.net_tx.last()),
                        "all links except loopback",
                        col(BLUE),
                        cx,
                    )),
            )
            .child(chart_card("Traffic", "", col(GREEN), cx).child(timeline(
                "n-tp",
                &[
                    s_("In", GREEN, &self.h.net_rx),
                    s_("Out", BLUE, &self.h.net_tx),
                ],
                None,
                rate_fmt,
                TALL,
            )))
            .child(interfaces)
            .child(ports)
    }

    pub fn apps(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let grouped = self.table.read(cx).delegate().grouped;
        let group = super::table::group(&self.procs);
        let mut by_cpu: Vec<_> = group
            .iter()
            .map(|r| (r.name.clone(), r.cpu as f64, load_color(r.cpu / 2.0)))
            .collect();
        by_cpu.sort_by(|a, b| b.1.total_cmp(&a.1));
        by_cpu.truncate(6);
        let mut by_mem: Vec<_> = group
            .iter()
            .map(|r| (r.name.clone(), r.mem as f64, col(VIOLET)))
            .collect();
        by_mem.sort_by(|a, b| b.1.total_cmp(&a.1));
        by_mem.truncate(6);
        v_flex()
            .size_full()
            .gap_3()
            .child(
                grid(columns_for(width, 480.).min(2))
                    .child(card("Apps by CPU (helpers rolled up)", cx).child(ranking("a-cpu", by_cpu, |v| format!("{v:.0}%"), 130.)))
                    .child(card("Apps by memory", cx).child(ranking("a-mem", by_mem, bytes_fmt, 130.))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("by-app").small().label("By app").selected(grouped).on_click(cx.listener(|this, _, _, cx| this.set_grouped(true, cx))))
                    .child(Button::new("by-proc").small().label("All processes").selected(!grouped).on_click(cx.listener(|this, _, _, cx| this.set_grouped(false, cx))))
                    .child(div().w(px(260.)).child(Input::new(&self.filter).small().cleanable(true)))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("{} rows · {} processes · protected counters may be zero; memory falls back to RSS", self.table.read(cx).delegate().rows.len(), self.procs.len()))),
            )
            .child(div().flex_1().min_h_0().rounded_lg().border_1().border_color(cx.theme().border).overflow_hidden().child(DataTable::<ProcTable>::new(&self.table).bordered(false).stripe(true).small()))
    }

    pub fn power(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.s.power.clone().unwrap_or_default();
        let (t_name, t_color) = thermal_name(self.s.thermal);
        let hottest = self.temps.iter().map(|t| t.1).fold(0.0, f32::max);
        let cols = columns_for(width, 480.).min(2);
        let slices = vec![
            ("CPU", p.cpu_w, col(BLUE), format!("{:.2} W", p.cpu_w)),
            ("GPU", p.gpu_w, col(TEAL), format!("{:.2} W", p.gpu_w)),
            (
                "Neural Engine",
                p.ane_w,
                col(GREEN),
                format!("{:.2} W", p.ane_w),
            ),
            ("DRAM", p.dram_w, col(AMBER), format!("{:.2} W", p.dram_w)),
        ];
        let mut hot = self.temps.clone();
        hot.sort_by(|a, b| b.1.total_cmp(&a.1));
        hot.truncate(10);
        let temp_rows: Vec<_> = hot
            .into_iter()
            .map(|(n, v)| (n, v as f64, load_color(v)))
            .collect();
        let energy: Vec<_> = top(&self.procs, 8, |r| r.watts)
            .into_iter()
            .filter(|r| r.watts > 0.0)
            .map(|r| (r.app.clone(), r.watts as f64, col(AMBER)))
            .collect();

        let battery = card("Battery", cx).map(|c| match &self.battery {
            None => c.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("No battery in this Mac"),
            ),
            Some(b) => {
                let state = if b.charging {
                    "Charging"
                } else if b.on_ac {
                    "On power adapter"
                } else {
                    "On battery"
                };
                let left = if b.minutes_left > 0 && b.minutes_left < 6000 {
                    format!("{}h {:02}m", b.minutes_left / 60, b.minutes_left % 60)
                } else {
                    "calculating".to_string()
                };
                c.child(kv("Charge", format!("{}% · {state}", b.percent), cx))
                    .child(
                        Progress::new("batt")
                            .value(b.percent as f32)
                            .color(col(if b.percent < 20 { RED } else { GREEN }))
                            .h_2(),
                    )
                    .child(kv("Time remaining", left, cx))
                    .child(kv(
                        "Health",
                        format!(
                            "{:.0}% of design ({} of {} mAh)",
                            b.health_pct(),
                            b.max_mah,
                            b.design_mah
                        ),
                        cx,
                    ))
                    .child(kv("Cycles", b.cycles.to_string(), cx))
                    .child(kv(
                        "Power flow",
                        format!("{:+.1} W at {:.1} V", b.watts(), b.voltage_v),
                        cx,
                    ))
                    .child(kv("Temperature", format!("{:.1} °C", b.temperature_c), cx))
            }
        });

        v_flex()
            .gap_3()
            .child(
                grid(columns_for(width, 190.).min(4))
                    .child(stat("SoC + DRAM power", reading(self.s.power.is_some(), format!("{:.1} W", self.h.power_total.last())), "live from the energy model", col(AMBER), cx))
                    .child(stat("Thermal pressure", t_name, "from the kernel", col(t_color), cx))
                    .child(stat("Hottest sensor", reading(!self.temps.is_empty(), format!("{hottest:.0} °C")), format!("{} sensors", self.temps.len()), load_color(hottest), cx))
                    .child(stat("Top app energy", energy.first().map_or("—".to_string(), |e| format!("{:.3} W", e.1)), energy.first().map_or(String::new(), |e| e.0.clone()), col(PINK), cx)),
            )
            .when(self.s.power.is_none(), |d| d.child(div().text_sm().text_color(col(AMBER)).child("Power telemetry unavailable. IOReport support depends on this Mac and macOS version.")))
            .child(chart_card("Power by component", reading(self.s.power.is_some(), format!("{:.1} W", self.h.power_total.last())), col(AMBER), cx).child(timeline("p-all", &[s_("CPU", BLUE, &self.h.power_cpu), s_("GPU", TEAL, &self.h.power_gpu), s_("Neural Engine", GREEN, &self.h.power_ane), s_("DRAM", AMBER, &self.h.power_dram)], None, watts, TALL)))
            .child(grid(cols).child(card("Where the power goes", cx).child(donut("p-donut", slices, cx))).child(battery))
            .child(
                grid(cols)
                    .child(card("Hottest sensors (°C)", cx).child(ranking("p-temp", temp_rows, |v| format!("{v:.0} °C"), 240.)))
                    .when(!energy.is_empty(), |g| g.child(card("Energy by process", cx).child(ranking("p-energy", energy, |v| format!("{v:.3} W"), 240.)))),
            )
    }

    pub fn logs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let log = &self.log;
        let source_buttons = Source::ALL.iter().map(|(src, name)| {
            let src = *src;
            Button::new(SharedString::from(*name))
                .small()
                .label(*name)
                .selected(log.source == src)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.log.source = src;
                    this.rebuild_log_index(cx);
                    cx.notify();
                }))
        });
        let count = log.visible.len();
        let list = uniform_list(
            "log-rows",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|row| {
                        let &ix = this.log.visible.get(row)?;
                        let e = this.log.entries.get(ix as usize)?;
                        let (time, level, process, message) = (
                            e.time.clone(),
                            e.level,
                            e.process.clone(),
                            e.message.clone(),
                        );
                        let muted = cx.theme().muted_foreground;
                        Some(
                            h_flex()
                                .id(row)
                                .w_full()
                                .h(px(22.))
                                .gap_2()
                                .px_2()
                                .text_xs()
                                .font_family("Menlo")
                                .hover(|d| d.bg(cx.theme().secondary))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.log.selected = this.log.entries.get(ix as usize).cloned();
                                    this.log.follow = false;
                                    cx.notify();
                                }))
                                .child(div().w(px(92.)).flex_none().text_color(muted).child(time))
                                .child(
                                    div()
                                        .size_2()
                                        .flex_none()
                                        .rounded_full()
                                        .bg(Monitor::level_color(level)),
                                )
                                .child(
                                    div()
                                        .w(px(130.))
                                        .flex_none()
                                        .truncate()
                                        .text_color(col(BLUE))
                                        .child(process),
                                )
                                .child(div().flex_1().min_w_0().truncate().child(message)),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&log.scroll)
        .size_full();

        v_flex()
            .size_full()
            .gap_2()
            .child(h_flex().gap_2().flex_wrap().children(source_buttons).child(div().w(px(300.)).child(Input::new(&log.query).small().cleanable(true))))
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("pause").small().label(if log.paused { "Resume" } else { "Pause" }).on_click(cx.listener(|this, _, _, cx| {
                        this.log.paused = !this.log.paused;
                        cx.notify();
                    })))
                    .child(Button::new("follow").small().label("Follow").selected(log.follow).on_click(cx.listener(|this, _, _, cx| {
                        this.log.follow = !this.log.follow;
                        cx.notify();
                    })))
                    .child(Button::new("clear").small().ghost().label("Clear").on_click(cx.listener(|this, _, _, cx| {
                        this.log.buffer.pending.lock().unwrap().clear();
                        this.log.entries.clear();
                        this.log.visible.clear();
                        this.log.selected = None;
                        cx.notify();
                    })))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!(
                        "{count} shown of {} · {:.0} errors/min · sensitive values appear as <private>, as macOS redacts them",
                        log.entries.len(),
                        self.h.log_errors.last()
                    ))),
            )
            .when_some(log.status.clone(), |d, msg| d.child(div().p_2().rounded_md().text_xs().bg(col(RED).opacity(0.2)).child(format!("Log stream unavailable: {msg}"))))
            .when(count == 0, |d| d.child(div().text_xs().text_color(cx.theme().muted_foreground).child("Waiting for matching log events. Try All key logs or clear the search.")))
            .child(div().flex_1().min_h_0().rounded_lg().border_1().border_color(cx.theme().border).overflow_hidden().child(list))
            .when_some(log.selected.clone(), |d, e| {
                d.child(
                    v_flex()
                        .id("log-detail")
                        .max_h(px(130.))
                        .overflow_y_scroll()
                        .gap_1()
                        .p_2()
                        .rounded_lg()
                        .border_1()
                        .border_color(cx.theme().border)
                        .text_xs()
                        .child(div().text_color(Monitor::level_color(e.level)).child(format!("{} · {:?} · {} · {}", e.time, e.level, e.process, e.subsystem)))
                        .child(e.message),
                )
            })
    }
}

use super::widgets::s as s_;
