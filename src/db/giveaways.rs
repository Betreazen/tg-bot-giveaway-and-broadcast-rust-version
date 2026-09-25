use super::Database;
use crate::{
    state::Media,
    time::{Time, from_db, now, to_db},
};
use anyhow::Result;
use sqlx::{Row, sqlite::SqliteRow};

#[derive(Clone, Debug, PartialEq)]
pub struct Giveaway {
    pub id: i64,
    pub start_at: Time,
    pub end_at: Time,
    pub description: String,
    pub num_winners: i64,
    pub is_active: bool,
    pub media: Media,
    pub ended_at: Option<Time>,
}

pub struct NewGiveaway {
    pub start_at: Time,
    pub end_at: Time,
    pub description: String,
    pub num_winners: i64,
    pub media: Media,
    pub created_by: i64,
}

const COLUMNS: &str = "id, start_at, end_at, description, num_winners, is_active,
    announce_media_type, announce_media_file_id, ended_at";

fn giveaway(row: SqliteRow) -> Result<Giveaway> {
    let ended_at: Option<String> = row.try_get("ended_at")?;
    Ok(Giveaway {
        id: row.try_get("id")?,
        start_at: from_db(row.try_get("start_at")?)?,
        end_at: from_db(row.try_get("end_at")?)?,
        description: row.try_get("description")?,
        num_winners: row.try_get("num_winners")?,
        is_active: row.try_get("is_active")?,
        media: Media {
            kind: row.try_get("announce_media_type")?,
            file_id: row.try_get("announce_media_file_id")?,
        },
        ended_at: ended_at.as_deref().map(from_db).transpose()?,
    })
}

impl Database {
    pub async fn active_giveaway(&self) -> Result<Option<Giveaway>> {
        let sql = format!("SELECT {COLUMNS} FROM giveaways WHERE is_active = 1");
        let row = sqlx::query(&sql).fetch_optional(&self.pool).await?;
        row.map(giveaway).transpose()
    }

    pub async fn giveaway(&self, id: i64) -> Result<Option<Giveaway>> {
        let sql = format!("SELECT {COLUMNS} FROM giveaways WHERE id = ?");
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(giveaway).transpose()
    }

    /// Deactivates every giveaway and creates the new active one, atomically.
    pub async fn create_giveaway(&self, new: &NewGiveaway) -> Result<i64> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE giveaways SET is_active = 0 WHERE is_active = 1")
            .execute(&mut *tx)
            .await?;
        let id = sqlx::query_scalar(
            "INSERT INTO giveaways (start_at, end_at, description, num_winners, is_active,
                announce_media_file_id, announce_media_type, created_by_admin_id, created_at)
             VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?) RETURNING id",
        )
        .bind(to_db(new.start_at))
        .bind(to_db(new.end_at))
        .bind(&new.description)
        .bind(new.num_winners)
        .bind(&new.media.file_id)
        .bind(&new.media.kind)
        .bind(new.created_by)
        .bind(to_db(now()))
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn end_giveaway(&self, id: i64) -> Result<Option<Giveaway>> {
        sqlx::query("UPDATE giveaways SET ended_at = ?, is_active = 0 WHERE id = ?")
            .bind(to_db(now()))
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.giveaway(id).await
    }

    pub async fn is_participant(&self, giveaway_id: i64, user_id: i64) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM participants WHERE giveaway_id = ? AND user_id = ?)",
        )
        .bind(giveaway_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Idempotent: a repeated or concurrent join keeps the first row.
    pub async fn add_participant(
        &self,
        giveaway_id: i64,
        user_id: i64,
        username: Option<&str>,
        end_snapshot: Time,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO participants
                (giveaway_id, user_id, joined_at, username_snapshot, giveaway_end_snapshot)
             VALUES (?, ?, ?, ?, ?) ON CONFLICT(giveaway_id, user_id) DO NOTHING",
        )
        .bind(giveaway_id)
        .bind(user_id)
        .bind(to_db(now()))
        .bind(username)
        .bind(to_db(end_snapshot))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn participant_count(&self, giveaway_id: i64) -> Result<i64> {
        Ok(
            sqlx::query_scalar("SELECT count(*) FROM participants WHERE giveaway_id = ?")
                .bind(giveaway_id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn attempts(&self, giveaway_id: i64, user_id: i64) -> Result<i64> {
        let attempts: Option<i64> = sqlx::query_scalar(
            "SELECT attempts FROM verification_attempts WHERE giveaway_id = ? AND user_id = ?",
        )
        .bind(giveaway_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(attempts.unwrap_or(0))
    }

    /// Records one wrong answer and returns the new total.
    pub async fn add_attempt(&self, giveaway_id: i64, user_id: i64) -> Result<i64> {
        self.bump_attempts(giveaway_id, user_id, "attempts + 1", 1)
            .await
    }

    /// Blocks the user for this giveaway (three attempts used up).
    pub async fn block(&self, giveaway_id: i64, user_id: i64) -> Result<()> {
        self.bump_attempts(giveaway_id, user_id, "max(attempts, 3)", 3)
            .await
            .map(drop)
    }

    async fn bump_attempts(&self, g: i64, u: i64, update: &str, initial: i64) -> Result<i64> {
        let sql = format!(
            "INSERT INTO verification_attempts (giveaway_id, user_id, attempts) VALUES (?, ?, ?)
             ON CONFLICT(giveaway_id, user_id) DO UPDATE SET attempts = {update}
             RETURNING attempts"
        );
        Ok(sqlx::query_scalar(&sql)
            .bind(g)
            .bind(u)
            .bind(initial)
            .fetch_one(&self.pool)
            .await?)
    }
}
