mod dialogues;
mod giveaways;
mod users;
mod winners;

pub use giveaways::{Giveaway, NewGiveaway};
pub use winners::{Draw, Winner, format_winner_list};

use anyhow::{Result, ensure};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{path::Path, time::Duration};

const SCHEMA_VERSION: i64 = 1;

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Opens (creating if needed) `dir/bot.db` and applies the idempotent schema.
    pub async fn open(dir: &Path) -> Result<Self> {
        tokio::fs::create_dir_all(dir).await?;
        let options = SqliteConnectOptions::new()
            .filename(dir.join("bot.db"))
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_secs(5));
        // One connection: the bot is a single process and SQLite serialises writers anyway.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&pool)
            .await?;
        ensure!(
            version <= SCHEMA_VERSION,
            "database schema is newer than this application"
        );
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(include_str!("../../migrations/0001_schema.sql"))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn check(&self) -> Result<()> {
        let check: String = sqlx::query_scalar("PRAGMA quick_check")
            .fetch_one(&self.pool)
            .await?;
        ensure!(check == "ok", "SQLite integrity check failed: {check}");
        Ok(())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}
