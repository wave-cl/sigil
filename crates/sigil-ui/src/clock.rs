//! Times, as the person reading them keeps time.
//!
//! # Local, not UTC
//!
//! The terminal client formatted a clock as `at % 86_400` for a while, which is
//! UTC and is therefore the correct time in exactly one timezone. Everywhere
//! else it quietly showed the wrong hour, all day, on every screen anybody
//! looked at, and nothing about it looked broken. So everything here goes
//! through the system zone, and a moment that cannot be placed in one is left
//! **blank rather than guessed at** — a plausible wrong time is worse than no
//! time.
//!
//! The formats deliberately match `sqex-chat`'s, so the same message reads the
//! same in both clients.

/// The time of day: `14:32`.
pub fn clock(at: u64) -> String {
    match local(at) {
        Some(z) => z.strftime("%H:%M").to_string(),
        None => String::new(),
    }
}

/// The whole moment — day, year, seconds, zone. What belongs behind a pointer.
pub fn stamp(at: u64) -> String {
    match local(at) {
        Some(z) => z.strftime("%A, %-d %B %Y at %H:%M:%S %Z").to_string(),
        None => String::new(),
    }
}

/// The day a moment falls on, for deciding where a separator goes.
///
/// Compared rather than shown, so this only has to be unambiguous.
pub fn day_of(at: u64) -> Option<String> {
    Some(local(at)?.strftime("%Y-%m-%d").to_string())
}

/// How a day separator reads: "Today", "Yesterday", "Friday, 28 August" — and
/// **the year once it is not the current one**, because a bare date is a trap
/// on old history.
pub fn day_label(at: u64, now: u64) -> String {
    let (Some(z), Some(n)) = (local(at), local(now)) else {
        return String::new();
    };
    let day = |z: &jiff::Zoned| z.strftime("%Y-%m-%d").to_string();
    let today = day(&n);
    if day(&z) == today {
        return "Today".into();
    }
    if let Some(yesterday) = n.checked_sub(jiff::Span::new().days(1)).ok()
        && day(&z) == day(&yesterday)
    {
        return "Yesterday".into();
    }
    if z.year() == n.year() {
        z.strftime("%A, %-d %B").to_string()
    } else {
        z.strftime("%A, %-d %B %Y").to_string()
    }
}

/// A time for a conversation list: the clock today, the weekday this week, a
/// date before that. A list has room for four or five characters, not for a
/// full date on every row.
pub fn brief(at: u64, now: u64) -> String {
    let (Some(z), Some(n)) = (local(at), local(now)) else {
        return String::new();
    };
    let day = |z: &jiff::Zoned| z.strftime("%Y-%m-%d").to_string();
    if day(&z) == day(&n) {
        return z.strftime("%H:%M").to_string();
    }
    let week_ago = n.checked_sub(jiff::Span::new().days(6)).ok();
    match week_ago {
        Some(w) if z.timestamp() >= w.timestamp() => z.strftime("%a").to_string(),
        _ if z.year() == n.year() => z.strftime("%-d %b").to_string(),
        _ => z.strftime("%-d %b %Y").to_string(),
    }
}

fn local(at: u64) -> Option<jiff::Zoned> {
    let secs = i64::try_from(at).ok()?;
    Some(
        jiff::Timestamp::from_second(secs)
            .ok()?
            .to_zoned(jiff::tz::TimeZone::system()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-09-08 12:00:00 UTC.
    const NOW: u64 = 1_788_004_800;
    const DAY: u64 = 86_400;

    #[test]
    fn today_and_yesterday_are_named_rather_than_dated() {
        assert_eq!(day_label(NOW, NOW), "Today");
        assert_eq!(day_label(NOW - DAY, NOW), "Yesterday");
    }

    #[test]
    fn old_history_carries_its_year() {
        // A bare "Friday, 28 August" on something from two years ago is a trap:
        // it reads as recent and there is nothing on it saying otherwise.
        let old = NOW - 400 * DAY;
        assert!(
            day_label(old, NOW)
                .chars()
                .filter(|c| c.is_numeric())
                .count()
                >= 5,
            "a date from another year must carry it: {}",
            day_label(old, NOW)
        );
    }

    #[test]
    fn a_time_that_cannot_be_placed_is_blank_rather_than_guessed() {
        // Beyond what a timestamp can represent. A plausible wrong time is
        // worse than none, because nothing about it looks wrong.
        assert_eq!(clock(u64::MAX), "");
        assert_eq!(stamp(u64::MAX), "");
        assert_eq!(day_of(u64::MAX), None);
        assert_eq!(day_label(u64::MAX, NOW), "");
        assert_eq!(brief(u64::MAX, NOW), "");
    }

    #[test]
    fn a_list_time_gets_shorter_the_older_it_is() {
        // Today it is a clock, this week a weekday, older a date. A list row
        // has four or five characters of room, not a full date.
        assert!(brief(NOW, NOW).contains(':'));
        assert!(!brief(NOW - 3 * DAY, NOW).contains(':'));
        assert!(!brief(NOW - 40 * DAY, NOW).contains(':'));
    }

    #[test]
    fn the_day_key_and_the_day_label_agree_about_where_a_boundary_is() {
        // The separator is placed by `day_of` and written by `day_label`. If
        // they disagreed, a separator would say "Today" in the middle of
        // yesterday, or two days would run together with no line between them.
        assert_ne!(day_of(NOW), day_of(NOW - DAY));
        assert_ne!(day_label(NOW, NOW), day_label(NOW - DAY, NOW));
    }
}
