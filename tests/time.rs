use chrono::{TimeZone, Utc};
use tg_bot_giveaway_and_broadcast::time::{
    calculate_dates, fmt_msk, format_dates_display, from_db, to_db,
};

#[test]
fn utc_is_shown_as_moscow_time() {
    let dt = Utc.with_ymd_and_hms(2024, 1, 1, 9, 0, 0).unwrap();
    assert_eq!(fmt_msk(dt, "%d.%m.%Y %H:%M"), "01.01.2024 12:00");
    assert_eq!(fmt_msk(dt, "%Y-%m-%d %H:%M"), "2024-01-01 12:00");
    // No DST: summer is also +3.
    let summer = Utc.with_ymd_and_hms(2024, 7, 1, 21, 30, 0).unwrap();
    assert_eq!(fmt_msk(summer, "%d.%m.%Y %H:%M"), "02.07.2024 00:30");
}

#[test]
fn db_format_round_trips_microseconds() {
    let dt = Utc
        .with_ymd_and_hms(2026, 7, 27, 13, 1, 2)
        .unwrap()
        .checked_add_signed(chrono::Duration::microseconds(123_456))
        .unwrap();
    let stored = to_db(dt);
    assert_eq!(stored, "2026-07-27T13:01:02.123456Z");
    assert_eq!(from_db(&stored).unwrap(), dt);
}

#[test]
fn db_parser_accepts_postgres_json_output() {
    let expected = Utc.with_ymd_and_hms(2026, 1, 5, 15, 6, 0).unwrap();
    assert_eq!(from_db("2026-01-05T15:06:00+00:00").unwrap(), expected);
    assert_eq!(from_db("2026-01-05T18:06:00+03:00").unwrap(), expected);
    assert_eq!(
        from_db("2026-01-05T15:06:00.5+00:00").unwrap(),
        expected + chrono::Duration::milliseconds(500)
    );
    assert!(from_db("yesterday").is_err());
}

#[test]
fn start_options_are_relative_to_moscow_now() {
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 22, 30, 0).unwrap(); // 01:30 MSK, 2 Jan
    let (start, end) = calculate_dates("now", 3, now);
    assert_eq!((start, end), (now, now + chrono::Duration::days(3)));
    assert_eq!(
        calculate_dates("1h", 1, now).0,
        now + chrono::Duration::hours(1)
    );
    assert_eq!(
        calculate_dates("3h", 1, now).0,
        now + chrono::Duration::hours(3)
    );
    assert_eq!(
        calculate_dates("6h", 1, now).0,
        now + chrono::Duration::hours(6)
    );
    let (start, end) = calculate_dates("tomorrow", 7, now);
    assert_eq!(start, Utc.with_ymd_and_hms(2026, 1, 3, 9, 0, 0).unwrap());
    assert_eq!(end, Utc.with_ymd_and_hms(2026, 1, 10, 9, 0, 0).unwrap());
    assert_eq!(calculate_dates("bogus", 1, now).0, now);
}

#[test]
fn dates_display_matches_python() {
    let start = Utc.with_ymd_and_hms(2026, 1, 3, 9, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2026, 1, 10, 9, 0, 0).unwrap();
    assert_eq!(
        format_dates_display(start, end),
        "🗓 Начало: 03.01.2026 12:00 МСК\n⏰ Окончание: 10.01.2026 12:00 МСК\n📅 Длительность: 7 дн."
    );
}
