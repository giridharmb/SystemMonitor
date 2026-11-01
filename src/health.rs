//! Turns raw metrics into one explainable 0-100 health score.

#[derive(Clone, Copy, Default, Debug)]
pub struct Inputs {
    pub cpu: f32,
    /// `kern.memorystatus_vm_pressure_level`: 1 normal, 2 warning, 4 critical.
    pub mem_pressure: u64,
    pub swapout_pages_per_sec: f32,
    pub disk_min_free_pct: f32,
    pub disk_latency_ms: f32,
    pub thermal: u64,
    pub log_errors_per_min: f32,
    pub crashes_24h: u32,
    pub battery_health_pct: Option<f32>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Severity {
    Warning,
    Critical,
}

#[derive(Clone, Debug)]
pub struct Issue {
    pub severity: Severity,
    pub text: String,
}

#[derive(Default, Clone, Debug)]
pub struct Health {
    pub score: u32,
    pub issues: Vec<Issue>,
}

pub fn evaluate(i: Inputs) -> Health {
    let mut penalty = 0;
    let mut issues = Vec::new();
    let mut hit = |points: u32, severity, text: String| {
        penalty += points;
        issues.push(Issue { severity, text });
    };
    use Severity::*;

    match i.mem_pressure {
        4 => hit(35, Critical, "Memory pressure is critical".into()),
        2 => hit(15, Warning, "Memory pressure is elevated".into()),
        _ => {}
    }
    match i.thermal {
        0 => {}
        1 => hit(
            10,
            Warning,
            "Thermal pressure is moderate, CPU may throttle".into(),
        ),
        _ => hit(
            30,
            Critical,
            "Thermal pressure is heavy, performance may be throttled".into(),
        ),
    }
    if i.cpu >= 90.0 {
        hit(15, Warning, format!("CPU is saturated at {:.0}%", i.cpu));
    }
    // macOS grows swap dynamically; a full allocated swapfile alone does not signal pressure.
    if i.swapout_pages_per_sec >= 100.0 && i.mem_pressure >= 2 {
        hit(
            10,
            Warning,
            format!("Active swap writes: {:.0} pages/s", i.swapout_pages_per_sec),
        );
    }
    if i.disk_min_free_pct < 5.0 {
        hit(
            25,
            Critical,
            format!("A volume has only {:.1}% free", i.disk_min_free_pct),
        );
    } else if i.disk_min_free_pct < 15.0 {
        hit(
            10,
            Warning,
            format!("A volume has only {:.1}% free", i.disk_min_free_pct),
        );
    }
    if i.disk_latency_ms > 50.0 {
        hit(
            10,
            Warning,
            format!("Disk latency is high at {:.0} ms", i.disk_latency_ms),
        );
    }
    if i.log_errors_per_min > 600.0 {
        hit(
            5,
            Warning,
            format!("{:.0} system log errors per minute", i.log_errors_per_min),
        );
    }
    if i.crashes_24h >= 5 {
        hit(
            10,
            Warning,
            format!("{} crash reports in the last 24 hours", i.crashes_24h),
        );
    }
    if let Some(b) = i.battery_health_pct.filter(|b| *b > 0.0 && *b < 80.0) {
        hit(
            5,
            Warning,
            format!("Battery capacity is down to {b:.0}% of design"),
        );
    }
    Health {
        score: 100u32.saturating_sub(penalty),
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fine() -> Inputs {
        Inputs {
            mem_pressure: 1,
            disk_min_free_pct: 60.0,
            ..Default::default()
        }
    }

    #[::core::prelude::v1::test]
    fn healthy_system_scores_full_marks() {
        let h = evaluate(fine());
        assert_eq!(h.score, 100);
        assert!(h.issues.is_empty());
    }

    #[::core::prelude::v1::test]
    fn each_problem_lowers_score_and_is_explained() {
        let h = evaluate(Inputs {
            mem_pressure: 4,
            thermal: 2,
            ..fine()
        });
        assert_eq!(h.score, 35);
        assert_eq!(h.issues.len(), 2);
        assert!(h.issues.iter().all(|i| i.severity == Severity::Critical));
    }

    #[::core::prelude::v1::test]
    fn score_saturates_at_zero() {
        let h = evaluate(Inputs {
            cpu: 100.0,
            mem_pressure: 4,
            swapout_pages_per_sec: 1000.0,
            disk_min_free_pct: 1.0,
            disk_latency_ms: 100.0,
            thermal: 4,
            log_errors_per_min: 1000.0,
            crashes_24h: 9,
            battery_health_pct: Some(50.0),
        });
        assert_eq!(h.score, 0);
    }

    #[::core::prelude::v1::test]
    fn swap_activity_requires_memory_pressure_to_reduce_health() {
        assert_eq!(
            evaluate(Inputs {
                swapout_pages_per_sec: 1000.,
                ..fine()
            })
            .score,
            100
        );
        assert_eq!(
            evaluate(Inputs {
                swapout_pages_per_sec: 1000.,
                mem_pressure: 2,
                ..fine()
            })
            .score,
            75
        );
    }

    #[::core::prelude::v1::test]
    fn battery_only_counts_when_known() {
        assert_eq!(
            evaluate(Inputs {
                battery_health_pct: Some(0.0),
                ..fine()
            })
            .score,
            100
        );
        assert_eq!(
            evaluate(Inputs {
                battery_health_pct: Some(79.0),
                ..fine()
            })
            .score,
            95
        );
    }
}
