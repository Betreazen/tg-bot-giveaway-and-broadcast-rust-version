use super::Database;
use crate::time::{now, to_db};
use anyhow::Result;

impl Database {
    /// Registers the user or refreshes the username; a missing username keeps the stored one.
    pub async fn upsert_user(&self, user_id: i64, username: Option<&str>) -> Result<()> {
        sqlx::query(
            "INSERT INTO users (user_id, username, joined_at) VALUES (?, ?, ?)
             ON CONFLICT(user_id) DO UPDATE SET username = coalesce(excluded.username, username)",
        )
        .bind(user_id)
        .bind(username)
        .bind(to_db(now()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Sets the flag on the user with this (case-insensitive) username; returns its id.
    pub async fn set_suspicious(&self, username: &str, suspicious: bool) -> Result<Option<i64>> {
        Ok(sqlx::query_scalar(
            "UPDATE users SET is_suspicious = ? WHERE lower(username) = lower(?) RETURNING user_id",
        )
        .bind(suspicious)
        .bind(username)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Suspicious users ordered like Postgres `ORDER BY username, user_id` (NULLs last).
    pub async fn suspicious_users(&self) -> Result<Vec<(i64, Option<String>)>> {
        Ok(sqlx::query_as(
            "SELECT user_id, username FROM users WHERE is_suspicious = 1
             ORDER BY username IS NULL, username, user_id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn user_count(&self) -> Result<i64> {
        Ok(sqlx::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&self.pool)
            .await?)
    }
}
