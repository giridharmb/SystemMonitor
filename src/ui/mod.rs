//! The application view: owns the latest sample and history, and renders one tab at a time.

mod table;
mod tabs;
pub mod widgets;

use crate::health::{self, Health};
use crate::logs::{self, Level, LogBuffer, LogEntry, Source};
use crate::mac::Battery;
use crate::sampler::{self, Control, Crash, Info, ProcRow, Sample, Volume};
use crate::util::Ring;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::tab::{Tab as UiTab, TabBar};
use gpui_kit::component::table::TableState;
use gpui_kit::component::{ActiveTheme, TitleBar, h_flex, v_flex};
use gpui_kit::*;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use table::ProcTable;

#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub enum Tab {
    #[default]
    Overview,
    Cpu,
    Gpu,
    Memory,
    Disk,
    Network,
    Apps,
    Power,
    Logs,
}

impl Tab {
    pub const ALL: [(Tab, &str); 9] = [
        (Tab::Overview, "Overview"),
        (Tab::Cpu, "CPU"),
        (Tab::Gpu, "GPU"),
        (Tab::Memory, "Memory"),
        (Tab::Disk, "Disk"),
        (Tab::Network, "Network"),
        (Tab::Apps, "Apps"),
        (Tab::Power, "Power"),
        (Tab::Logs, "Logs"),
    ];

    pub fn from_name(name: &str) -> Option<Tab> {
        Self::ALL
            .iter()
            .find(|t| t.1.eq_ignore_ascii_case(name))
            .map(|t| t.0)
    }

    fn uses_procs(self) -> bool {
        matches!(
            self,
            Tab::Overview | Tab::Cpu | Tab::Memory | Tab::Disk | Tab::Apps | Tab::Power
        )
    }
}

#[derive(Default)]
pub struct Hist {
    pub cpu: Ring,
    pub cpu_sys: Ring,
    pub cores: Vec<Ring>,
    pub clusters: Vec<(String, Ring)>,
    pub mem_used: Ring,
    pub mem_app: Ring,
    pub mem_wired: Ring,
    pub mem_compressed: Ring,
    pub swap: Ring,
    pub page_in: Ring,
    pub page_out: Ring,
    pub compress: Ring,
    pub decompress: Ring,
    pub gpu: Ring,
    pub gpu_renderer: Ring,
    pub gpu_tiler: Ring,
    pub gpu_mem: Ring,
    pub gpu_mhz: Ring,
    pub power_cpu: Ring,
    pub power_gpu: Ring,
    pub power_ane: Ring,
    pub power_dram: Ring,
    pub power_total: Ring,
    pub disk_read: Ring,
    pub disk_write: Ring,
    pub iops_read: Ring,
    pub iops_write: Ring,
    pub lat_read: Ring,
    pub lat_write: Ring,
    pub net_rx: Ring,
    pub net_tx: Ring,
    pub log_errors: Ring,
}

pub struct LogView {
    pub capturing: bool,
    pub buffer: Arc<LogBuffer>,
    pub entries: VecDeque<LogEntry>,
    pub visible: Vec<u32>,
    pub source: Source,
    pub query: Entity<InputState>,
    pub paused: bool,
    pub follow: bool,
    pub selected: Option<LogEntry>,
    pub scroll: UniformListScrollHandle,
    pub status: Option<String>,
    problems_seen: u64,
    problems_at: Instant,
}

pub struct Monitor {
    pub tab: Tab,
    focus: FocusHandle,
    pub info: Info,
    ctl: Arc<Control>,
    pub s: Sample,
    pub h: Hist,
    pub health: Health,
    pub procs: Arc<Vec<ProcRow>>,
    pub volumes: Vec<Volume>,
    pub temps: Vec<(String, f32)>,
    pub battery: Option<Battery>,
    pub ports: Vec<(String, i32, String)>,
    pub crashes: Vec<Crash>,
    pub table: Entity<TableState<ProcTable>>,
    pub filter: Entity<InputState>,
    pub log: LogView,
    was_active: bool,
    ticks: u64,
    _subs: Vec<Subscription>,
}

impl Monitor {
    pub fn new(tab: Tab, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let info = sampler::info();
        let ctl = Arc::new(Control::default());
        ctl.wants_procs.store(tab.uses_procs(), Ordering::Relaxed);
        ctl.wants_ports
            .store(tab == Tab::Network, Ordering::Relaxed);
        ctl.window_active.store(true, Ordering::Relaxed);

        let (tx, rx) = smol::channel::bounded(4);
        sampler::spawn(ctl.clone(), info.total_mem, tx);
        cx.spawn(async move |this, cx| {
            while let Ok(sample) = rx.recv().await {
                if this
                    .update(cx, |this, cx| this.on_sample(sample, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        // Logs animate faster than the 1 Hz metrics, but only while they are on screen.
        cx.spawn(async move |this, cx| {
            loop {
                smol::Timer::after(Duration::from_millis(250)).await;
                if this
                    .update(cx, |this, cx| {
                        if this.tab == Tab::Logs
                            && !this.log.paused
                            && this.ctl.window_active.load(Ordering::Relaxed)
                        {
                            this.drain_logs(cx)
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let table = cx.new(|cx| {
            TableState::new(ProcTable::new(), window, cx)
                .col_selectable(false)
                .col_movable(false)
        });
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter by name…"));
        let query = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search message, process or subsystem…")
        });
        let subs = vec![
            cx.subscribe(&filter, |this, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.rebuild_table(cx);
                }
            }),
            cx.subscribe(&query, |this, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.rebuild_log_index(cx);
                    cx.notify();
                }
            }),
            cx.observe_window_activation(window, |this, window, _| {
                this.ctl
                    .window_active
                    .store(window.is_window_active(), Ordering::Relaxed);
            }),
        ];

        let hist = Hist {
            cores: vec![Ring::default(); info.core_kinds.len()],
            ..Default::default()
        };
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            tab,
            focus,
            info,
            ctl,
            s: Sample::default(),
            h: hist,
            health: Health {
                score: 100,
                issues: Vec::new(),
            },
            procs: Arc::default(),
            volumes: Vec::new(),
            temps: Vec::new(),
            battery: None,
            ports: Vec::new(),
            crashes: Vec::new(),
            table,
            filter,
            log: LogView {
                capturing: tab == Tab::Logs,
                buffer: if tab == Tab::Logs {
                    logs::start()
                } else {
                    Arc::new(LogBuffer::default())
                },
                entries: VecDeque::new(),
                visible: Vec::new(),
                source: Source::All,
                query,
                paused: false,
                follow: true,
                selected: None,
                scroll: UniformListScrollHandle::new(),
                status: None,
                problems_seen: 0,
                problems_at: Instant::now(),
            },
            was_active: true,
            ticks: 0,
            _subs: subs,
        }
    }

    fn on_sample(&mut self, mut s: Sample, cx: &mut Context<Self>) {
        self.record(&s);
        if let Some(p) = s.procs.take() {
            self.procs = p;
            if self.tab == Tab::Apps {
                self.rebuild_table(cx);
            }
        }
        if let Some(v) = s.volumes.take() {
            self.volumes = v;
        }
        if let Some(t) = s.temps.take() {
            self.temps = t;
        }
        if let Some(b) = s.battery.take() {
            self.battery = b;
        }
        if let Some(p) = s.ports.take() {
            self.ports = p;
        }
        if let Some(c) = s.crashes.take() {
            self.crashes = c;
        }
        self.s = s;
        self.update_health();
        self.ticks += 1;

        // Rendering is what costs the most. A background window repaints every 5 s, enough to stay
        // current in Mission Control without burning GPU time nobody is watching.
        let active = self.ctl.window_active.load(Ordering::Relaxed);
        if active || self.was_active || self.ticks.is_multiple_of(5) {
            cx.notify();
        }
        self.was_active = active;
    }

    fn record(&mut self, s: &Sample) {
        let n = s.core_user.len().max(1) as f32;
        let (user, sys) = (
            s.core_user.iter().sum::<f32>() / n,
            s.core_sys.iter().sum::<f32>() / n,
        );
        let h = &mut self.h;
        h.cpu.push(user + sys);
        h.cpu_sys.push(sys);
        for (i, ring) in h.cores.iter_mut().enumerate() {
            ring.push(
                s.core_user.get(i).copied().unwrap_or(0.0)
                    + s.core_sys.get(i).copied().unwrap_or(0.0),
            );
        }
        let m = &s.mem;
        h.mem_used.push(if m.total > 0 {
            m.used() as f32 / m.total as f32 * 100.0
        } else {
            0.0
        });
        h.mem_app.push(m.vm.app as f32);
        h.mem_wired.push(m.vm.wired as f32);
        h.mem_compressed.push(m.vm.compressed as f32);
        h.swap.push(m.swap_used as f32);
        h.page_in.push(m.pagein);
        h.page_out.push(m.pageout);
        h.compress.push(m.compress);
        h.decompress.push(m.decompress);
        h.gpu.push(s.gpu.device);
        h.gpu_renderer.push(s.gpu.renderer);
        h.gpu_tiler.push(s.gpu.tiler);
        h.gpu_mem.push(s.gpu.mem_used as f32);
        h.disk_read.push(s.disk.read);
        h.disk_write.push(s.disk.write);
        h.iops_read.push(s.disk.read_iops);
        h.iops_write.push(s.disk.write_iops);
        h.lat_read.push(s.disk.read_ms);
        h.lat_write.push(s.disk.write_ms);
        // Loopback would double count local traffic against the real links.
        let real = s.net.iter().filter(|n| n.name != "lo0");
        let (rx, tx) = real.fold((0.0, 0.0), |a, n| (a.0 + n.rx, a.1 + n.tx));
        h.net_rx.push(rx);
        h.net_tx.push(tx);
        if let Some(p) = &s.power {
            h.power_cpu.push(p.cpu_w);
            h.power_gpu.push(p.gpu_w);
            h.power_ane.push(p.ane_w);
            h.power_dram.push(p.dram_w);
            h.power_total.push(p.cpu_w + p.gpu_w + p.ane_w + p.dram_w);
            h.gpu_mhz.push(p.gpu_mhz);
            for c in &p.clusters {
                match h.clusters.iter_mut().find(|x| x.0 == c.name) {
                    Some(x) => x.1.push(c.mhz * c.active / 100.0),
                    None => {
                        let mut ring = Ring::default();
                        ring.push(c.mhz * c.active / 100.0);
                        h.clusters.push((c.name.clone(), ring));
                    }
                }
            }
        }
        let log = &mut self.log;
        if log.capturing && log.problems_at.elapsed() >= Duration::from_secs(10) {
            let total = log.buffer.problems.load(Ordering::Relaxed);
            let per_min =
                (total - log.problems_seen) as f32 * 60.0 / log.problems_at.elapsed().as_secs_f32();
            log.problems_seen = total;
            log.problems_at = Instant::now();
            h.log_errors.push(per_min);
        }
        if log.capturing {
            log.status.clone_from(&log.buffer.status.lock().unwrap());
        }
    }

    fn update_health(&mut self) {
        let s = &self.s;
        let writable = self.volumes.iter().filter(|v| !v.read_only && v.total > 0);
        let min_free = writable
            .map(|v| v.free as f32 / v.total as f32 * 100.0)
            .fold(100.0, f32::min);
        self.health = health::evaluate(health::Inputs {
            cpu: self.h.cpu.last(),
            mem_pressure: s.mem.pressure,
            swapout_pages_per_sec: s.mem.swapout,
            disk_min_free_pct: min_free,
            disk_latency_ms: s.disk.worst_latency(),
            thermal: s.thermal,
            log_errors_per_min: if self.log.capturing {
                self.h.log_errors.last()
            } else {
                0.
            },
            crashes_24h: self.crashes.iter().filter(|c| c.age_secs < 86400).count() as u32,
            battery_health_pct: self.battery.as_ref().map(|b| b.health_pct()),
        });
    }

    pub fn rebuild_table(&mut self, cx: &mut Context<Self>) {
        let needle = self.filter.read(cx).value().to_lowercase();
        let procs = self.procs.clone();
        self.table.update(cx, |t, cx| {
            let d = t.delegate_mut();
            let mut rows = if d.grouped {
                table::group(&procs)
            } else {
                table::flat(&procs)
            };
            if !needle.is_empty() {
                rows.retain(|r| r.name.to_lowercase().contains(&needle));
            }
            d.rows = rows;
            d.sort_rows();
            cx.notify();
        });
    }

    pub fn set_grouped(&mut self, grouped: bool, cx: &mut Context<Self>) {
        self.table.update(cx, |t, cx| {
            t.delegate_mut().grouped = grouped;
            t.refresh(cx);
        });
        self.rebuild_table(cx);
        cx.notify();
    }

    fn drain_logs(&mut self, cx: &mut Context<Self>) {
        let log = &mut self.log;
        let fresh: Vec<LogEntry> = log.buffer.pending.lock().unwrap().drain(..).collect();
        if fresh.is_empty() {
            return;
        }
        log.entries.extend(fresh);
        let over = log.entries.len().saturating_sub(5000);
        log.entries.drain(..over);
        self.rebuild_log_index(cx);
        if self.log.follow && !self.log.visible.is_empty() {
            self.log
                .scroll
                .scroll_to_item(self.log.visible.len() - 1, ScrollStrategy::Bottom);
        }
        cx.notify();
    }

    pub fn rebuild_log_index(&mut self, cx: &App) {
        let q = self.log.query.read(cx).value().to_lowercase();
        let source = self.log.source;
        self.log.visible = self
            .log
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.matches(source, &q))
            .map(|(i, _)| i as u32)
            .collect();
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if tab == Tab::Logs && !self.log.capturing {
            self.log.buffer = logs::start();
            self.log.capturing = true;
            self.log.problems_seen = 0;
            self.log.problems_at = Instant::now();
            self.log.status = None;
            self.h.log_errors = Ring::default();
        } else if tab != Tab::Logs && self.log.capturing {
            logs::stop();
            self.drain_logs(cx);
            self.log.capturing = false;
            self.log.status = None;
        }
        self.tab = tab;
        self.ctl
            .wants_procs
            .store(tab.uses_procs(), Ordering::Relaxed);
        self.ctl
            .wants_ports
            .store(tab == Tab::Network, Ordering::Relaxed);
        match tab {
            Tab::Apps => self.rebuild_table(cx),
            Tab::Logs => self.drain_logs(cx),
            _ => {}
        }
        cx.notify();
    }

    pub fn level_color(level: Level) -> Hsla {
        widgets::col(match level {
            Level::Fault => widgets::RED,
            Level::Error => widgets::AMBER,
            Level::Default => widgets::GRAY,
        })
    }
}

impl Render for Monitor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = Tab::ALL.iter().position(|t| t.0 == self.tab).unwrap_or(0);
        let width = f32::from(window.viewport_size().width);
        let body: AnyElement = match self.tab {
            Tab::Overview => self.overview(width, cx).into_any_element(),
            Tab::Cpu => self.cpu(width, cx).into_any_element(),
            Tab::Gpu => self.gpu(width, cx).into_any_element(),
            Tab::Memory => self.memory(width, cx).into_any_element(),
            Tab::Disk => self.disk(width, cx).into_any_element(),
            Tab::Network => self.network(width, cx).into_any_element(),
            Tab::Apps => self.apps(width, cx).into_any_element(),
            Tab::Power => self.power(width, cx).into_any_element(),
            Tab::Logs => self.logs(cx).into_any_element(),
        };
        // Apps and Logs manage their own virtualized scrolling.
        let own_scroll = matches!(self.tab, Tab::Apps | Tab::Logs);
        v_flex()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, action: &crate::SelectTab, _, cx| {
                if let Some(&(tab, _)) = Tab::ALL.get(action.index) {
                    this.set_tab(tab, cx);
                }
            }))
            .size_full()
            .bg(cx.theme().background)
            .child(
                TitleBar::new().child(
                    TabBar::new("tabs")
                        .segmented()
                        .px_0()
                        .py(px(2.))
                        .bg(cx.theme().title_bar)
                        .selected_index(selected)
                        .on_click(
                            cx.listener(|this, ix: &usize, _, cx| {
                                this.set_tab(Tab::ALL[*ix].0, cx)
                            }),
                        )
                        .children(Tab::ALL.iter().map(|t| UiTab::new().label(t.1))),
                ),
            )
            .child(if own_scroll {
                div()
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .child(body)
                    .into_any_element()
            } else {
                div()
                    .id("scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .child(body)
                    .into_any_element()
            })
            .child(self.status_bar(cx))
    }
}

impl Monitor {
    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mem_pct = self.h.mem_used.last();
        let item = |label: &'static str, value: String| {
            h_flex()
                .gap_1()
                .child(div().text_color(cx.theme().muted_foreground).child(label))
                .child(value)
        };
        h_flex()
            .h_7()
            .px_3()
            .gap_4()
            .justify_between()
            .text_xs()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tab_bar)
            .child(
                h_flex()
                    .gap_4()
                    .child(item("CPU", format!("{:.0}%", self.h.cpu.last())))
                    .child(item("Memory", format!("{mem_pct:.0}%")))
                    .child(item(
                        "GPU",
                        if self.s.gpu.available {
                            format!("{:.0}%", self.h.gpu.last())
                        } else {
                            "Unavailable".into()
                        },
                    ))
                    .child(item(
                        "Power",
                        if self.s.power.is_some() {
                            format!("{:.1} W", self.h.power_total.last())
                        } else {
                            "Unavailable".into()
                        },
                    ))
                    .child(item("Health", self.health.score.to_string())),
            )
            .child(
                // The monitor reports on itself: overhead is a feature, so keep it honest.
                h_flex()
                    .gap_4()
                    .child(item(
                        "This app",
                        format!(
                            "{:.1}% CPU · {}",
                            self.s.self_cpu,
                            crate::util::bytes(self.s.self_rss)
                        ),
                    ))
                    .child(item("Sample", format!("{:.1} ms", self.s.sample_ms))),
            )
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        logs::stop();
    }
}
