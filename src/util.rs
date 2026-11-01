//! Small pure helpers shared by the sampler and the UI.

use std::collections::VecDeque;

pub const HISTORY: usize = 180;

/// Fixed-length history. Starts full of zeros so charts keep a stable x axis from the first sample.
#[derive(Clone)]
pub struct Ring(VecDeque<f32>);

impl Default for Ring {
    fn default() -> Self {
        Self(VecDeque::from(vec![0.0; HISTORY]))
    }
}

impl Ring {
    pub fn push(&mut self, v: f32) {
        self.0.pop_front();
        self.0.push_back(v);
    }
    pub fn last(&self) -> f32 {
        self.0.back().copied().unwrap_or(0.0)
    }
    pub fn max(&self) -> f32 {
        self.0.iter().copied().fold(0.0, f32::max)
    }
    pub fn tail(&self, n: usize) -> Vec<f32> {
        self.0
            .iter()
            .skip(self.0.len().saturating_sub(n))
            .copied()
            .collect()
    }
}

pub fn bytes(v: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut f = v as f64;
    let mut i = 0;
    while f >= 1024.0 && i < UNITS.len() - 1 {
        f /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{v} B")
    } else {
        format!("{f:.1} {}", UNITS[i])
    }
}

pub fn rate(v: f32) -> String {
    format!("{}/s", bytes(v.max(0.0) as u64))
}

pub fn duration(mut secs: u64) -> String {
    let d = secs / 86400;
    secs %= 86400;
    let (h, m) = (secs / 3600, secs % 3600 / 60);
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else {
        format!("{h}h {m}m")
    }
}

/// Per-core `[user, system, idle, nice]` tick delta to percent `(user + nice, system)`.
/// Counters are u32 and wrap on long uptimes, hence wrapping_sub.
pub fn core_usage(prev: [u32; 4], cur: [u32; 4]) -> (f32, f32) {
    let d: [u64; 4] = std::array::from_fn(|i| cur[i].wrapping_sub(prev[i]) as u64);
    let total: u64 = d.iter().sum();
    if total == 0 {
        return (0.0, 0.0);
    }
    (
        (d[0] + d[3]) as f32 / total as f32 * 100.0,
        d[1] as f32 / total as f32 * 100.0,
    )
}

/// The app a process belongs to: its outermost `.app` bundle, so Chrome's helpers roll up into Chrome.
pub fn app_name<'a>(exe: &'a str, process: &'a str) -> &'a str {
    exe.split('/')
        .find_map(|c| c.strip_suffix(".app"))
        .filter(|n| !n.is_empty())
        .unwrap_or(process)
}

/// Listening sockets from `lsof -F pcn`: `(process, pid, address)`.
pub fn parse_lsof(out: &str) -> Vec<(String, i32, String)> {
    let (mut pid, mut cmd) = (0, String::new());
    let mut rows = Vec::new();
    for line in out.lines() {
        match line.split_at_checked(1) {
            Some(("p", v)) => pid = v.parse().unwrap_or(0),
            Some(("c", v)) => cmd = v.to_string(),
            Some(("n", v)) => rows.push((cmd.clone(), pid, v.to_string())),
            _ => {}
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    rows.dedup();
    rows
}

/// Run a local probe with a deadline and bounded output; drain concurrently so a full pipe cannot stall it.
pub fn command_output(
    command: &mut std::process::Command,
    timeout: std::time::Duration,
) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().expect("stdout piped");
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(256 * 1024)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(error) => break Err(error),
            Ok(None) if std::time::Instant::now() >= deadline => {
                break Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "probe timed out",
                ));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    };
    if status.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let bytes = reader
        .join()
        .map_err(|_| std::io::Error::other("probe reader failed"))??;
    status.map(|_| bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn ring_stays_fixed_length() {
        let mut r = Ring::default();
        for i in 0..HISTORY + 5 {
            r.push(i as f32);
        }
        assert_eq!(r.tail(usize::MAX).len(), HISTORY);
        assert_eq!(r.last(), (HISTORY + 4) as f32);
        assert_eq!(r.tail(2), vec![(HISTORY + 3) as f32, (HISTORY + 4) as f32]);
    }

    #[::core::prelude::v1::test]
    fn local_probe_has_a_deadline() {
        let error = command_output(
            std::process::Command::new("/bin/sleep").arg("2"),
            std::time::Duration::from_millis(30),
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(
            command_output(
                std::process::Command::new("/bin/echo").arg("ok"),
                std::time::Duration::from_secs(1)
            )
            .unwrap(),
            b"ok\n"
        );
    }

    #[::core::prelude::v1::test]
    fn formats() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(5 << 40), "5.0 TB");
        assert_eq!(rate(-3.0), "0 B/s");
        assert_eq!(duration(90_061), "1d 1h 1m");
    }

    #[::core::prelude::v1::test]
    fn core_usage_handles_idle_and_wraparound() {
        assert_eq!(core_usage([0, 0, 0, 0], [0, 0, 0, 0]), (0.0, 0.0));
        assert_eq!(core_usage([0, 0, 0, 0], [40, 10, 50, 0]), (40.0, 10.0));
        assert_eq!(core_usage([u32::MAX, 0, 0, 0], [49, 0, 50, 0]), (50.0, 0.0));
    }

    #[::core::prelude::v1::test]
    fn core_usage_does_not_overflow_when_user_and_nice_are_large() {
        let (user, system) = core_usage([0; 4], [u32::MAX, 0, 0, u32::MAX]);
        assert_eq!((user, system), (100.0, 0.0));
    }

    #[::core::prelude::v1::test]
    fn app_grouping() {
        let helper = "/Applications/Google Chrome.app/Contents/Frameworks/G.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper";
        assert_eq!(app_name(helper, "x"), "Google Chrome");
        assert_eq!(app_name("/usr/libexec/logd", "logd"), "logd");
        assert_eq!(app_name("", "kernel_task"), "kernel_task");
    }

    #[::core::prelude::v1::test]
    fn lsof_parsing_dedupes_v4_v6() {
        let o = "p10\ncnginx\nf3\nn*:80\nf4\nn*:80\np20\ncpostgres\nf5\nn127.0.0.1:5432\n";
        assert_eq!(
            parse_lsof(o),
            vec![
                ("nginx".to_string(), 10, "*:80".to_string()),
                ("postgres".to_string(), 20, "127.0.0.1:5432".to_string())
            ]
        );
    }
}
