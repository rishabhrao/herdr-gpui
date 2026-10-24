//! Times, durations, and token counts as the section prints them. The
//! daemon reports Unix seconds; they are shown in this computer's zone.

use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};

/// Unix seconds now.
pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The day `at` falls on in `zone`.
pub(crate) fn day<Tz: TimeZone>(at: i64, zone: &Tz) -> Option<NaiveDate> {
    DateTime::from_timestamp(at, 0).map(|at| at.with_timezone(zone).date_naive())
}

/// When something happened or will: the time alone today, the weekday
/// and time within a week either side, else the date.
pub(crate) fn when(at: i64, now: i64) -> String {
    when_in(at, now, &Local)
}

pub(crate) fn when_in<Tz: TimeZone>(at: i64, now: i64, zone: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let (Some(at_time), Some(today)) = (
        DateTime::from_timestamp(at, 0).map(|at| at.with_timezone(zone)),
        day(now, zone),
    ) else {
        return "—".to_owned();
    };
    let days = (at_time.date_naive() - today).num_days();
    match days {
        0 => at_time.format("%H:%M").to_string(),
        -6..=6 => at_time.format("%a %H:%M").to_string(),
        _ if at_time.year() == today.year() => at_time.format("%b %-d").to_string(),
        _ => at_time.format("%b %-d %Y").to_string(),
    }
}

/// How long until `at`, for what fires soon: `in 12 min`, `in 1 h 12 min`.
/// Past a day it is [`when`].
pub(crate) fn until(at: i64, now: i64) -> String {
    let left = at - now;
    match left {
        ..=59 => "now".to_owned(),
        60..3600 => format!("in {} min", left / 60),
        3600..86_400 => match (left % 3600) / 60 {
            0 => format!("in {} h", left / 3600),
            minutes => format!("in {} h {minutes} min", left / 3600),
        },
        _ => when(at, now),
    }
}

/// A duration in its two largest units: `3s`, `12m04s`, `2h 04m`, `4 d`.
pub(crate) fn span(seconds: i64) -> String {
    let seconds = seconds.max(0);
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m{:02}s", seconds / 60, seconds % 60),
        3600..86_400 => format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60),
        _ => format!("{} d", seconds / 86_400),
    }
}

/// A token count, short: `940`, `340k`, `1.2M`.
pub(crate) fn tokens(count: u64) -> String {
    match count {
        0..1000 => count.to_string(),
        1000..1_000_000 => format!("{}k", count / 1000),
        _ => {
            let tenths = count / 100_000;
            if tenths.is_multiple_of(10) {
                format!("{}M", tenths / 10)
            } else {
                format!("{}.{}M", tenths / 10, tenths % 10)
            }
        }
    }
}

/// Bytes, short: `512 KB`, `3.1 GB`.
pub(crate) fn bytes(count: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if count < 1024 {
        return format!("{count} B");
    }
    let mut value = count as f64 / 1024.;
    let mut unit = 0;
    while value >= 1024. && unit + 1 < UNITS.len() {
        value /= 1024.;
        unit += 1;
    }
    if value < 10. {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// CPU time in milliseconds, as [`span`] prints it.
pub(crate) fn cpu(milliseconds: u64) -> String {
    span(i64::try_from(milliseconds / 1000).unwrap_or(i64::MAX))
}
