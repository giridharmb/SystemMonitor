//! Applications table: processes rolled up by app bundle, or listed raw.

use super::widgets::{AMBER, GREEN, RED, col};
use crate::sampler::ProcRow;
use crate::util::{bytes, rate};
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::*;
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct Row {
    pub name: String,
    /// Process count when grouped, PID otherwise.
    pub id: i32,
    pub cpu: f32,
    pub mem: u64,
    pub threads: u32,
    pub read: f32,
    pub write: f32,
    pub wakeups: f32,
    pub watts: f32,
}

/// Rolls processes up into their app. Pure so the numbers can be tested.
pub fn group(procs: &[ProcRow]) -> Vec<Row> {
    let mut by_app: HashMap<&str, Row> = HashMap::new();
    for p in procs {
        let r = by_app.entry(&p.app).or_insert_with(|| Row {
            name: p.app.clone(),
            ..Default::default()
        });
        r.id += 1;
        r.cpu += p.cpu;
        r.mem += p.mem;
        r.threads += p.threads;
        r.read += p.read_bps;
        r.write += p.write_bps;
        r.wakeups += p.wakeups;
        r.watts += p.watts;
    }
    by_app.into_values().collect()
}

pub fn flat(procs: &[ProcRow]) -> Vec<Row> {
    procs
        .iter()
        .map(|p| Row {
            name: p.name.clone(),
            id: p.pid,
            cpu: p.cpu,
            mem: p.mem,
            threads: p.threads,
            read: p.read_bps,
            write: p.write_bps,
            wakeups: p.wakeups,
            watts: p.watts,
        })
        .collect()
}

pub struct ProcTable {
    pub rows: Vec<Row>,
    pub grouped: bool,
    sort_col: usize,
    sort: ColumnSort,
}

impl ProcTable {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            grouped: true,
            sort_col: 2,
            sort: ColumnSort::Descending,
        }
    }

    pub fn sort_rows(&mut self) {
        let col = self.sort_col;
        let key = |r: &Row| -> f64 {
            match col {
                1 => r.id as f64,
                2 => r.cpu as f64,
                3 => r.mem as f64,
                4 => r.threads as f64,
                5 => r.read as f64,
                6 => r.write as f64,
                7 => r.wakeups as f64,
                8 => r.watts as f64,
                _ => 0.0,
            }
        };
        self.rows.sort_by(|a, b| {
            let o = if col == 0 {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            } else {
                key(a).total_cmp(&key(b))
            };
            // Name is the tie-break so idle rows do not shuffle between refreshes.
            let o = if matches!(self.sort, ColumnSort::Ascending) {
                o
            } else {
                o.reverse()
            };
            o.then_with(|| a.name.cmp(&b.name))
        });
    }
}

impl TableDelegate for ProcTable {
    fn columns_count(&self, _: &App) -> usize {
        9
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, ix: usize, _: &App) -> Column {
        let (key, label, w): (&'static str, &'static str, f32) = match ix {
            0 => ("name", "Name", 300.),
            1 => ("id", if self.grouped { "Procs" } else { "PID" }, 70.),
            2 => ("cpu", "CPU %", 80.),
            3 => ("mem", "Memory", 95.),
            4 => ("threads", "Threads", 75.),
            5 => ("read", "Disk read", 95.),
            6 => ("write", "Disk write", 95.),
            7 => ("wake", "Wakeups/s", 90.),
            _ => ("watts", "Energy", 80.),
        };
        let c = Column::new(key, label).width(w).sortable();
        if ix == self.sort_col {
            c.sort(self.sort)
        } else {
            c
        }
    }
    fn render_td(
        &mut self,
        row: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(r) = self.rows.get(row) else {
            return div().into_any_element();
        };
        let muted = cx.theme().muted_foreground;
        let small = |text: String, color: Hsla| {
            div()
                .text_xs()
                .text_color(color)
                .child(text)
                .into_any_element()
        };
        match col_ix {
            0 => div()
                .text_sm()
                .truncate()
                .child(r.name.clone())
                .into_any_element(),
            1 => small(r.id.to_string(), muted),
            2 => small(
                format!("{:.1}", r.cpu),
                col(if r.cpu > 100.0 {
                    RED
                } else if r.cpu > 30.0 {
                    AMBER
                } else {
                    GREEN
                }),
            ),
            3 => small(bytes(r.mem), cx.theme().foreground),
            4 => small(r.threads.to_string(), muted),
            5 => small(rate(r.read), muted),
            6 => small(rate(r.write), muted),
            7 => small(
                format!("{:.0}", r.wakeups),
                if r.wakeups > 500.0 { col(AMBER) } else { muted },
            ),
            _ => small(format!("{:.2} W", r.watts), muted),
        }
    }
    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        self.sort_col = col_ix;
        self.sort = sort;
        self.sort_rows();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(app: &str, pid: i32, cpu: f32, mem: u64) -> ProcRow {
        ProcRow {
            pid,
            name: format!("{app}-{pid}"),
            app: app.into(),
            cpu,
            mem,
            threads: 2,
            ..Default::default()
        }
    }

    #[::core::prelude::v1::test]
    fn group_rolls_up_helpers() {
        let rows = group(&[
            p("Chrome", 1, 10.0, 100),
            p("Chrome", 2, 5.0, 50),
            p("logd", 3, 1.0, 10),
        ]);
        let chrome = rows.iter().find(|r| r.name == "Chrome").unwrap();
        assert_eq!(
            (chrome.id, chrome.cpu, chrome.mem, chrome.threads),
            (2, 15.0, 150, 4)
        );
        assert_eq!(rows.len(), 2);
    }

    #[::core::prelude::v1::test]
    fn sorting_is_stable_for_equal_values() {
        let mut t = ProcTable::new();
        t.rows = flat(&[p("b", 1, 1.0, 1), p("a", 2, 1.0, 1), p("c", 3, 9.0, 1)]);
        t.sort_rows();
        let names: Vec<_> = t.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["c-3", "a-2", "b-1"]);
    }
}
