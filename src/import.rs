//! `bot import <dir>`: loads the Python bot's Postgres export into an empty database.
//!
//! The switch-over script dumps each table with
//! `psql -At -c "SET TIME ZONE 'UTC'" -c "SELECT row_to_json(t) FROM <table> t ORDER BY <pk>"`
//! into `<table>.jsonl`. Everything is inserted in one transaction: any error leaves the
//! database untouched. Error messages name the file and line but never the row content,
//! which holds personal data.
use crate::{
    db::Database,
    time::{from_db, to_db},
};
use anyhow::{Context, Result, anyhow, ensure};
use serde::{Deserialize, de::DeserializeOwned};
use sqlx::SqliteConnection;
use std::{
    fmt,
    fs::File,
    io::{BufRead, BufReader, Lines},
    path::Path,
};

const TABLES: [&str; 4] = ["users", "giveaways", "participants", "winners"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportReport {
    pub users: i64,
    pub suspicious: i64,
    pub giveaways: i64,
    pub participants: i64,
    pub winners: i64,
}

/// Exactly five `key=value` lines: the switch-over script parses them.
impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "users={}\nsuspicious={}\ngiveaways={}\nparticipants={}\nwinners={}",
            self.users, self.suspicious, self.giveaways, self.participants, self.winners
        )
    }
}

pub async fn run(db: &Database, dir: &Path) -> Result<ImportReport> {
    let users = Source::open(dir, "users")?;
    let giveaways = Source::open(dir, "giveaways")?;
    let participants = Source::open(dir, "participants")?;
    let winners = Source::open(dir, "winners")?;

    let mut tx = db.pool().begin().await?;
    ensure_empty(&mut tx).await?;
    let counts = [
        users.load::<UserRow>(&mut tx).await?,
        giveaways.load::<GiveawayRow>(&mut tx).await?,
        participants.load::<ParticipantRow>(&mut tx).await?,
        winners.load::<WinnerRow>(&mut tx).await?,
    ];
    let report = verify(&mut tx, counts).await?;
    tx.commit().await?;
    Ok(report)
}

async fn ensure_empty(conn: &mut SqliteConnection) -> Result<()> {
    for table in TABLES {
        let sql = format!("SELECT EXISTS(SELECT 1 FROM {table})");
        let has_rows: bool = sqlx::query_scalar(&sql).fetch_one(&mut *conn).await?;
        ensure!(
            !has_rows,
            "the database already has data in {table}; import only into an empty database"
        );
    }
    Ok(())
}

/// Row counts must match the files line for line, else the transaction is rolled back.
async fn verify(conn: &mut SqliteConnection, counts: [i64; 4]) -> Result<ImportReport> {
    for (table, expected) in TABLES.into_iter().zip(counts) {
        let sql = format!("SELECT count(*) FROM {table}");
        let actual: i64 = sqlx::query_scalar(&sql).fetch_one(&mut *conn).await?;
        ensure!(
            actual == expected,
            "{table}: {actual} rows in the database, {expected} in the file"
        );
    }
    let suspicious = sqlx::query_scalar("SELECT count(*) FROM users WHERE is_suspicious = 1")
        .fetch_one(&mut *conn)
        .await?;
    let [users, giveaways, participants, winners] = counts;
    Ok(ImportReport {
        users,
        suspicious,
        giveaways,
        participants,
        winners,
    })
}

struct Source {
    name: String,
    lines: Lines<BufReader<File>>,
}

impl Source {
    fn open(dir: &Path, table: &str) -> Result<Self> {
        let name = format!("{table}.jsonl");
        let file = File::open(dir.join(&name)).with_context(|| format!("cannot open {name}"))?;
        Ok(Self {
            name,
            lines: BufReader::new(file).lines(),
        })
    }

    /// Inserts every non-empty line; returns how many rows were read.
    async fn load<R: Record>(self, conn: &mut SqliteConnection) -> Result<i64> {
        let mut rows = 0;
        for (index, line) in self.lines.enumerate() {
            let at = format!("{}:{}", self.name, index + 1);
            let line = line.with_context(|| at.clone())?;
            if line.trim().is_empty() {
                continue;
            }
            let row: R =
                serde_json::from_str(&line).map_err(|e| anyhow!("{at}: {}", json_error(&e)))?;
            row.insert(conn).await.with_context(|| at.clone())?;
            rows += 1;
        }
        Ok(rows)
    }
}

/// serde_json quotes offending values ("invalid type: string \"…\""); keep only messages
/// that name columns, never values.
fn json_error(error: &serde_json::Error) -> String {
    let message = error.to_string();
    if message.starts_with("unknown field") || message.starts_with("missing field") {
        message
    } else {
        format!(
            "invalid JSON row ({:?} error at column {})",
            error.classify(),
            error.column()
        )
    }
}

/// Normalizes a Postgres timestamptz to the storage format without echoing the value.
fn timestamp(value: &str, column: &str) -> Result<String> {
    from_db(value)
        .map(to_db)
        .map_err(|_| anyhow!("invalid timestamp in {column}"))
}

fn optional_timestamp(value: Option<&str>, column: &str) -> Result<Option<String>> {
    value.map(|value| timestamp(value, column)).transpose()
}

trait Record: DeserializeOwned {
    async fn insert(self, conn: &mut SqliteConnection) -> Result<()>;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserRow {
    user_id: i64,
    username: Option<String>,
    joined_at: String,
    is_suspicious: bool,
}

impl Record for UserRow {
    async fn insert(self, conn: &mut SqliteConnection) -> Result<()> {
        sqlx::query(
            "INSERT INTO users (user_id, username, joined_at, is_suspicious) VALUES (?, ?, ?, ?)",
        )
        .bind(self.user_id)
        .bind(self.username)
        .bind(timestamp(&self.joined_at, "joined_at")?)
        .bind(i64::from(self.is_suspicious))
        .execute(conn)
        .await?;
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GiveawayRow {
    id: i64,
    start_at: String,
    end_at: String,
    description: String,
    num_winners: i64,
    is_active: bool,
    announce_text: Option<String>,
    announce_media_file_id: String,
    announce_media_type: String,
    created_by_admin_id: i64,
    created_at: String,
    ended_at: Option<String>,
}

impl Record for GiveawayRow {
    async fn insert(self, conn: &mut SqliteConnection) -> Result<()> {
        sqlx::query(
            "INSERT INTO giveaways (id, start_at, end_at, description, num_winners, is_active,
                announce_text, announce_media_file_id, announce_media_type,
                created_by_admin_id, created_at, ended_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(self.id)
        .bind(timestamp(&self.start_at, "start_at")?)
        .bind(timestamp(&self.end_at, "end_at")?)
        .bind(self.description)
        .bind(self.num_winners)
        .bind(i64::from(self.is_active))
        .bind(self.announce_text)
        .bind(self.announce_media_file_id)
        .bind(self.announce_media_type)
        .bind(self.created_by_admin_id)
        .bind(timestamp(&self.created_at, "created_at")?)
        .bind(optional_timestamp(self.ended_at.as_deref(), "ended_at")?)
        .execute(conn)
        .await?;
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParticipantRow {
    id: i64,
    giveaway_id: i64,
    user_id: i64,
    joined_at: String,
    username_snapshot: Option<String>,
    giveaway_end_snapshot: String,
}

impl Record for ParticipantRow {
    async fn insert(self, conn: &mut SqliteConnection) -> Result<()> {
        sqlx::query(
            "INSERT INTO participants
                (id, giveaway_id, user_id, joined_at, username_snapshot, giveaway_end_snapshot)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(self.id)
        .bind(self.giveaway_id)
        .bind(self.user_id)
        .bind(timestamp(&self.joined_at, "joined_at")?)
        .bind(self.username_snapshot)
        .bind(timestamp(
            &self.giveaway_end_snapshot,
            "giveaway_end_snapshot",
        )?)
        .execute(conn)
        .await?;
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WinnerRow {
    id: i64,
    giveaway_id: i64,
    user_id: i64,
    username_snapshot: Option<String>,
    giveaway_end_snapshot: String,
    created_at: String,
}

impl Record for WinnerRow {
    async fn insert(self, conn: &mut SqliteConnection) -> Result<()> {
        sqlx::query(
            "INSERT INTO winners
                (id, giveaway_id, user_id, username_snapshot, giveaway_end_snapshot, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(self.id)
        .bind(self.giveaway_id)
        .bind(self.user_id)
        .bind(self.username_snapshot)
        .bind(timestamp(
            &self.giveaway_end_snapshot,
            "giveaway_end_snapshot",
        )?)
        .bind(timestamp(&self.created_at, "created_at")?)
        .execute(conn)
        .await?;
        Ok(())
    }
}
