//! Timestamps are stored in UTC and always shown in Moscow time (UTC+3, no DST),
//! regardless of the server's timezone — a fixed product rule of the Python bot.
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, FixedOffset, NaiveTime, SecondsFormat, Utc};

pub type Time = DateTime<Utc>;

fn msk() -> FixedOffset {
    FixedOffset::east_opt(3 * 3600).expect("valid offset")
}

pub fn now() -> Time {
    Utc::now()
}

/// Storage format: RFC 3339, UTC, microseconds — lossless for Postgres timestamptz
/// and lexicographically sortable.
pub fn to_db(time: Time) -> String {
    time.to_rfc3339_opts(SecondsFormat::Micros, true)
}

pub fn from_db(text: &str) -> Result<Time> {
    Ok(DateTime::parse_from_rfc3339(text)
        .with_context(|| format!("invalid timestamp {text:?}"))?
        .with_timezone(&Utc))
}

pub fn fmt_msk(time: Time, format: &str) -> String {
    time.with_timezone(&msk()).format(format).to_string()
}

/// Start and end of a new giveaway for the wizard's start option and duration.
pub fn calculate_dates(start_option: &str, duration_days: i64, now: Time) -> (Time, Time) {
    let start = match start_option {
        "1h" => now + Duration::hours(1),
        "3h" => now + Duration::hours(3),
        "6h" => now + Duration::hours(6),
        "tomorrow" => {
            let noon = NaiveTime::from_hms_opt(12, 0, 0).expect("valid time");
            let tomorrow = (now.with_timezone(&msk()) + Duration::days(1)).date_naive();
            tomorrow
                .and_time(noon)
                .and_local_timezone(msk())
                .single()
                .expect("fixed offset is unambiguous")
                .with_timezone(&Utc)
        }
        _ => now,
    };
    (start, start + Duration::days(duration_days))
}

pub fn format_dates_display(start: Time, end: Time) -> String {
    format!(
        "🗓 Начало: {} МСК\n⏰ Окончание: {} МСК\n📅 Длительность: {} дн.",
        fmt_msk(start, "%d.%m.%Y %H:%M"),
        fmt_msk(end, "%d.%m.%Y %H:%M"),
        (end - start).num_days()
    )
}
