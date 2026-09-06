//! When the desk is open and when the next tick starts. All arithmetic in the agent's own IANA
//! zone (the host zone differs from the client's — the recurring trap on every desk), returned
//! as UTC instants. Nothing here bounds model work: a cadence is the gap between tick STARTS.

use chrono::{DateTime, Datelike, Duration, NaiveTime, TimeZone, Timelike, Utc, Weekday};
use chrono_tz::Tz;

use super::manifest::WorkWindow;

pub fn parse_cadence(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.len().checked_sub(1)?);
    let n: i64 = num.trim().parse().ok()?;
    if n <= 0 {
        return None;
    }
    match unit {
        "s" => Some(Duration::seconds(n)),
        "m" => Some(Duration::minutes(n)),
        "h" => Some(Duration::hours(n)),
        _ => None,
    }
}

pub fn parse_hm(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.trim().split_once(':')?;
    NaiveTime::from_hms_opt(h.parse().ok()?, m.parse().ok()?, 0)
}

pub fn parse_day(s: &str) -> Option<Weekday> {
    match s.trim().to_ascii_lowercase().as_str() {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct DeskClock {
    pub tz: Tz,
    pub window: WorkWindow,
    pub cadence: Duration,
}

impl DeskClock {
    pub fn new(tz: &str, window: &WorkWindow, cadence: &str) -> Option<Self> {
        Some(Self {
            tz: tz.parse().ok()?,
            window: window.clone(),
            cadence: parse_cadence(cadence)?,
        })
    }

    fn days(&self) -> Vec<Weekday> {
        self.window
            .days
            .iter()
            .filter_map(|d| parse_day(d))
            .collect()
    }

    fn bounds(&self) -> (NaiveTime, NaiveTime) {
        (
            parse_hm(&self.window.from).unwrap_or(NaiveTime::from_hms_opt(0, 0, 0).unwrap()),
            parse_hm(&self.window.to).unwrap_or(NaiveTime::from_hms_opt(23, 59, 0).unwrap()),
        )
    }

    pub fn is_open(&self, at: DateTime<Utc>) -> bool {
        if self.window.always {
            return true;
        }
        let local = at.with_timezone(&self.tz);
        let days = self.days();
        if !days.is_empty() && !days.contains(&local.weekday()) {
            return false;
        }
        let (from, to) = self.bounds();
        let t = local.time();
        t >= from && t < to
    }

    /// The first instant at or after `at` when the desk is open. Searches day by day for two
    /// weeks — a window with no matching day at all returns None (a named absence for the caller).
    pub fn next_open(&self, at: DateTime<Utc>) -> Option<DateTime<Utc>> {
        if self.is_open(at) {
            return Some(at);
        }
        let local = at.with_timezone(&self.tz);
        let days = self.days();
        let (from, _) = self.bounds();
        for offset in 0..15 {
            let day = local.date_naive() + Duration::days(offset);
            if !days.is_empty() && !days.contains(&day.weekday()) {
                continue;
            }
            let candidate_naive = day.and_time(from);
            let Some(candidate) = self.tz.from_local_datetime(&candidate_naive).single() else {
                continue;
            };
            let candidate = candidate.with_timezone(&Utc);
            if candidate > at && self.is_open(candidate) {
                return Some(candidate);
            }
        }
        None
    }

    /// When the next tick starts, and why. `last_start` = the previous tick's start (None on the
    /// first tick → now). The candidate is last_start + cadence, pulled into the window when the
    /// desk is closed at that moment.
    pub fn next_tick(
        &self,
        last_start: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> (Option<DateTime<Utc>>, &'static str) {
        let candidate = match last_start {
            Some(t) => {
                let c = t + self.cadence;
                if c < now {
                    now
                } else {
                    c
                }
            }
            None => now,
        };
        if self.is_open(candidate) {
            let reason = if last_start.is_none() {
                "first tick"
            } else if candidate == now {
                "overdue — the previous tick overran the cadence"
            } else {
                "cadence"
            };
            return (Some(candidate), reason);
        }
        match self.next_open(candidate) {
            Some(t) => (Some(t), "desk closed — waits for the window to open"),
            None => (None, "the window names no day the desk is open"),
        }
    }

    pub fn local_label(&self, at: DateTime<Utc>) -> String {
        let l = at.with_timezone(&self.tz);
        format!(
            "{} {:02}:{:02} {}",
            l.weekday(),
            l.hour(),
            l.minute(),
            self.tz.name()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock() -> DeskClock {
        DeskClock::new(
            "Europe/Zurich",
            &WorkWindow {
                days: vec![
                    "mon".into(),
                    "tue".into(),
                    "wed".into(),
                    "thu".into(),
                    "fri".into(),
                ],
                from: "09:00".into(),
                to: "18:00".into(),
                always: false,
            },
            "30m",
        )
        .unwrap()
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn cadence_parses_units_and_refuses_junk() {
        assert_eq!(parse_cadence("30m"), Some(Duration::minutes(30)));
        assert_eq!(parse_cadence("2h"), Some(Duration::hours(2)));
        assert_eq!(parse_cadence("90s"), Some(Duration::seconds(90)));
        assert_eq!(parse_cadence("soon"), None);
        assert_eq!(parse_cadence("0m"), None);
    }

    #[test]
    fn window_is_evaluated_in_the_desk_zone() {
        let c = clock();
        // 2026-09-07 is a Monday. 07:30Z = 09:30 Zurich (CEST) → open; 06:30Z = 08:30 → closed.
        assert!(c.is_open(utc("2026-09-07T07:30:00Z")));
        assert!(!c.is_open(utc("2026-09-07T06:30:00Z")));
        // Saturday is closed regardless of hour.
        assert!(!c.is_open(utc("2026-09-05T10:00:00Z")));
    }

    #[test]
    fn next_tick_follows_cadence_inside_the_window_and_the_opening_outside_it() {
        let c = clock();
        let last = utc("2026-09-07T07:30:00Z");
        let (t, why) = c.next_tick(Some(last), utc("2026-09-07T07:31:00Z"));
        assert_eq!(t, Some(utc("2026-09-07T08:00:00Z")));
        assert_eq!(why, "cadence");
        // Friday 15:50Z = 17:50 Zurich; +30m lands at 18:20, closed → Monday 09:00 Zurich = 07:00Z.
        let (t, why) = c.next_tick(
            Some(utc("2026-09-04T15:50:00Z")),
            utc("2026-09-04T15:51:00Z"),
        );
        assert_eq!(t, Some(utc("2026-09-07T07:00:00Z")));
        assert!(why.starts_with("desk closed"));
        // An overrun tick: the candidate is in the past → now.
        let now = utc("2026-09-07T09:00:00Z");
        let (t, why) = c.next_tick(Some(utc("2026-09-07T07:30:00Z")), now);
        assert_eq!(t, Some(now));
        assert!(why.starts_with("overdue"));
    }

    #[test]
    fn an_always_open_desk_never_waits_for_a_window() {
        let mut c = clock();
        c.window.always = true;
        assert!(c.is_open(utc("2026-09-05T02:00:00Z")));
        let (t, _) = c.next_tick(None, utc("2026-09-05T02:00:00Z"));
        assert_eq!(t, Some(utc("2026-09-05T02:00:00Z")));
    }
}
