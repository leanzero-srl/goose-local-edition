//! Agent Work's clock. Since Q-228 (L6) it lives in `goose::loop_clock`, the one clock Agent Work
//! and session loops share; this module re-exports it so the desk's `window::…` paths stay put.

pub use goose::loop_clock::*;

/// Agent Work's clock pinned through Agent Work's own path: `agent.yaml` fixtures parsed by
/// `AgentManifest` (so `WorkWindow`'s serde defaults are in play), `DeskClock::new` called exactly
/// as `mod.rs` calls it, then `next_tick` / `is_open` / `local_label` over a grid of instants that
/// crosses weekends, the window edges and the 2026-10-25 DST change. The golden table was
/// generated from the clock BEFORE it moved to `goose::loop_clock` (Q-228 L6); a difference
/// after the move is a behaviour change, not a move.
#[cfg(test)]
mod agent_work_clock_pin {
    use super::super::manifest::AgentManifest;
    use super::*;
    use chrono::{DateTime, Duration, Utc};

    fn fixtures() -> Vec<(&'static str, String)> {
        vec![
            ("starter", AgentManifest::starter("pin")),
            ("serde-defaults", "name: d\n".to_string()),
            (
                "zurich-weekdays-15m",
                "name: z\ntimezone: Europe/Zurich\ncadence: 15m\nwindow:\n  days: [mon, tue, wed, thu, fri]\n  from: \"08:30\"\n  to: \"17:45\"\n".to_string(),
            ),
            (
                "every-day-partial-window",
                "name: e\ntimezone: America/New_York\ncadence: 2h\nwindow:\n  days: []\n  from: \"22:00\"\n".to_string(),
            ),
            (
                "every-day-45m",
                "name: n\ntimezone: America/New_York\ncadence: 45m\nwindow:\n  days: []\n  from: \"07:15\"\n  to: \"19:40\"\n".to_string(),
            ),
            (
                "always-90s",
                "name: a\ntimezone: Asia/Tokyo\ncadence: 90s\nwindow:\n  always: true\n".to_string(),
            ),
        ]
    }

    fn fmt(t: Option<DateTime<Utc>>) -> String {
        match t {
            Some(t) => t.to_rfc3339(),
            None => "none".to_string(),
        }
    }

    fn table() -> String {
        let start = DateTime::parse_from_rfc3339("2026-10-22T05:17:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut out = String::new();
        for (name, yaml) in fixtures() {
            let m: AgentManifest = serde_yaml::from_str(&yaml).unwrap();
            m.validate().unwrap();
            let clock = DeskClock::new(&m.timezone, &m.window, &m.cadence).unwrap();
            out.push_str(&format!(
                "# {name} tz={} cadence={} window={:?}\n",
                m.timezone, m.cadence, m.window
            ));
            for step in 0..42 {
                let now = start + Duration::minutes(step * 173);
                for last in [
                    None,
                    Some(now - Duration::minutes(7)),
                    Some(now - Duration::minutes(29)),
                    Some(now - Duration::hours(3)),
                ] {
                    let (next, why) = clock.next_tick(last, now);
                    let label = match next {
                        Some(t) => clock.local_label(t),
                        None => "-".to_string(),
                    };
                    out.push_str(&format!(
                        "{} last={} open={} -> {} ({why}) {label}\n",
                        now.to_rfc3339(),
                        fmt(last),
                        clock.is_open(now),
                        fmt(next),
                    ));
                }
            }
        }
        out
    }

    #[test]
    fn agent_work_next_tick_is_identical_to_the_pre_move_golden() {
        assert_eq!(table(), include_str!("clock_pin.golden.txt"));
    }
}
