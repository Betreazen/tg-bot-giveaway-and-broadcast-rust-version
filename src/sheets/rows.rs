//! Rows of the five sheets, read from SQLite in the Python bot's layout and formats.
use super::Rows;
use crate::{
    db::Database,
    time::{self, fmt_msk, from_db},
};
use anyhow::Result;
use serde_json::{Value, json};
use sqlx::SqliteConnection;

const DATE_FORMAT: &str = "%Y-%m-%d %H:%M";
const DESCRIPTION_MAX: usize = 50;
const DESCRIPTION_KEEP: usize = 47;

/// All five sheets with headers, read in one transaction so the Overview totals match
/// the row counts of the detail sheets.
pub(super) async fn collect(db: &Database) -> Result<Vec<(&'static str, Rows)>> {
    let mut tx = db.pool().begin().await?;
    let users = users(&mut tx).await?;
    let participants = participants(&mut tx).await?;
    let winners = winners(&mut tx).await?;
    let giveaways = giveaways(&mut tx).await?;
    let (unique, suspicious): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(DISTINCT user_id) FROM participants),
                (SELECT COUNT(*) FROM users WHERE is_suspicious = 1)",
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.rollback().await?;
    let overview = overview([
        ("Уникальных пользователей (= строк в Users)", users.len()),
        ("Всего участий (= строк в Participants)", participants.len()),
        ("Уникальных участников", count(unique)),
        ("Подозрительных аккаунтов", count(suspicious)),
        ("Всего розыгрышей", giveaways.len()),
        ("Всего победителей (= строк в Winners)", winners.len()),
    ]);
    Ok(vec![
        (
            "Overview",
            with_header(&["Показатель", "Значение"], overview),
        ),
        ("Users", with_header(USERS_HEADER, users)),
        (
            "Participants",
            with_header(PARTICIPANTS_HEADER, participants),
        ),
        ("Winners", with_header(WINNERS_HEADER, winners)),
        (
            "Giveaways Summary",
            with_header(GIVEAWAYS_HEADER, giveaways),
        ),
    ])
}

const USERS_HEADER: &[&str] = &["User ID", "Username", "Joined At (MSK)", "Suspicious"];
const PARTICIPANTS_HEADER: &[&str] = &[
    "Giveaway ID",
    "User ID",
    "Username",
    "Joined At (MSK)",
    "Giveaway Start (MSK)",
    "Giveaway End (MSK)",
];
const WINNERS_HEADER: &[&str] = &["Giveaway ID", "User ID", "Username", "Selected At (MSK)"];
const GIVEAWAYS_HEADER: &[&str] = &[
    "ID",
    "Description",
    "Start (MSK)",
    "End (MSK)",
    "Duration (days)",
    "Total Participants",
    "Winners Count",
    "New Participants",
    "Status",
    "Created At (MSK)",
    "Created By Admin",
];

fn with_header(header: &[&str], rows: Rows) -> Rows {
    std::iter::once(header.iter().map(|h| json!(h)).collect())
        .chain(rows)
        .collect()
}

fn count(n: i64) -> usize {
    usize::try_from(n).unwrap_or_default()
}

fn overview(totals: [(&str, usize); 6]) -> Rows {
    totals
        .into_iter()
        .map(|(label, n)| vec![json!(label), json!(n)])
        .chain([vec![
            json!("Обновлено (МСК)"),
            json!(fmt_msk(time::now(), DATE_FORMAT)),
        ]])
        .collect()
}

/// NULL text becomes an empty cell, as gspread shows Python's None.
fn text(value: Option<String>) -> Value {
    Value::String(value.unwrap_or_default())
}

fn date(value: Option<&str>) -> Result<Value> {
    let text = match value {
        Some(stored) => fmt_msk(from_db(stored)?, DATE_FORMAT),
        None => String::new(),
    };
    Ok(Value::String(text))
}

async fn users(conn: &mut SqliteConnection) -> Result<Rows> {
    let rows: Vec<(i64, Option<String>, String, bool)> = sqlx::query_as(
        "SELECT user_id, username, joined_at, is_suspicious FROM users ORDER BY joined_at, user_id",
    )
    .fetch_all(conn)
    .await?;
    rows.into_iter()
        .map(|(user_id, username, joined_at, suspicious)| {
            let mark = if suspicious { "Да" } else { "" };
            Ok(vec![
                json!(user_id),
                text(username),
                date(Some(&joined_at))?,
                json!(mark),
            ])
        })
        .collect()
}

type ParticipantRow = (
    i64,
    i64,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
);

async fn participants(conn: &mut SqliteConnection) -> Result<Rows> {
    // LEFT JOIN, as in Python: a participant of a deleted giveaway still gets a row.
    let rows: Vec<ParticipantRow> = sqlx::query_as(
        "SELECT p.giveaway_id, p.user_id, p.username_snapshot, p.joined_at, g.start_at, g.end_at
             FROM participants p LEFT JOIN giveaways g ON g.id = p.giveaway_id ORDER BY p.id",
    )
    .fetch_all(conn)
    .await?;
    rows.into_iter()
        .map(
            |(giveaway_id, user_id, username, joined_at, start_at, end_at)| {
                Ok(vec![
                    json!(giveaway_id),
                    json!(user_id),
                    text(username),
                    date(Some(&joined_at))?,
                    date(start_at.as_deref())?,
                    date(end_at.as_deref())?,
                ])
            },
        )
        .collect()
}

async fn winners(conn: &mut SqliteConnection) -> Result<Rows> {
    let rows: Vec<(i64, i64, Option<String>, String)> = sqlx::query_as(
        "SELECT giveaway_id, user_id, username_snapshot, created_at FROM winners ORDER BY id",
    )
    .fetch_all(conn)
    .await?;
    rows.into_iter()
        .map(|(giveaway_id, user_id, username, created_at)| {
            Ok(vec![
                json!(giveaway_id),
                json!(user_id),
                text(username),
                date(Some(&created_at))?,
            ])
        })
        .collect()
}

/// "New participants": users whose first participation, by (joined_at, id), is this giveaway.
const GIVEAWAYS_SQL: &str = "
    WITH parts AS (SELECT giveaway_id, COUNT(*) AS n FROM participants GROUP BY giveaway_id),
    wins AS (SELECT giveaway_id, COUNT(*) AS n FROM winners GROUP BY giveaway_id),
    firsts AS (
        SELECT giveaway_id, COUNT(*) AS n FROM (
            SELECT giveaway_id,
                   ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY joined_at, id) AS rank
            FROM participants
        ) WHERE rank = 1 GROUP BY giveaway_id
    )
    SELECT g.id, g.description, g.start_at, g.end_at, g.is_active, g.created_at,
           g.created_by_admin_id, COALESCE(p.n, 0), COALESCE(w.n, 0), COALESCE(f.n, 0)
    FROM giveaways g
    LEFT JOIN parts p ON p.giveaway_id = g.id
    LEFT JOIN wins w ON w.giveaway_id = g.id
    LEFT JOIN firsts f ON f.giveaway_id = g.id
    ORDER BY g.id";

type GiveawayRow = (
    i64,
    String,
    String,
    String,
    bool,
    String,
    i64,
    i64,
    i64,
    i64,
);

async fn giveaways(conn: &mut SqliteConnection) -> Result<Rows> {
    let rows: Vec<GiveawayRow> = sqlx::query_as(GIVEAWAYS_SQL).fetch_all(conn).await?;
    rows.into_iter().map(giveaway_row).collect()
}

fn giveaway_row(row: GiveawayRow) -> Result<Vec<Value>> {
    let (id, description, start_at, end_at, active, created_at, admin, parts, wins, firsts) = row;
    let days = (from_db(&end_at)? - from_db(&start_at)?).num_days();
    let status = if active {
        "Активен"
    } else {
        "Завершен"
    };
    Ok(vec![
        json!(id),
        json!(shorten(&description)),
        date(Some(&start_at))?,
        date(Some(&end_at))?,
        json!(days),
        json!(parts),
        json!(wins),
        json!(firsts),
        json!(status),
        date(Some(&created_at))?,
        json!(admin),
    ])
}

/// Python: longer than 50 characters → first 47 plus "...".
fn shorten(description: &str) -> String {
    if description.chars().count() <= DESCRIPTION_MAX {
        return description.to_owned();
    }
    let kept: String = description.chars().take(DESCRIPTION_KEEP).collect();
    format!("{kept}...")
}
