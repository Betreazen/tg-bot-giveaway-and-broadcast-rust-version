use super::Database;
use crate::{
    state::Media,
    time::{Time, from_db, now, to_db},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, sqlite::SqliteRow};

/// What one mailing sends to every recipient.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Content {
    pub text: Option<String>,
    pub media: Option<Media>,
    /// Adds the "🎁 Участвовать" URL button (announcements).
    pub join_button: bool,
}

/// Decides the final report text shown to the admin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MailingKind {
    Broadcast,
    Announce,
    AnnounceNew,
    Results,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Audience {
    Nobody,
    /// Everyone registered before the mailing was queued.
    Users,
    /// A snapshot of `ADMIN_IDS`.
    Admins(Vec<i64>),
}

pub struct NewMailing {
    pub kind: MailingKind,
    pub content: Content,
    pub to_channel: bool,
    pub audience: Audience,
    pub rps: u32,
    pub report_chat: i64,
    pub report_message: i32,
}

pub struct Mailing {
    pub id: i64,
    pub kind: MailingKind,
    pub content: Content,
    pub to_channel: bool,
    pub audience: Audience,
    pub rps: u32,
    pub report_chat: i64,
    pub report_message: i32,
    pub finished: bool,
    pub channel_done: bool,
    pub channel_sent: bool,
    pub cursor: Option<i64>,
    pub total: i64,
    pub sent: i64,
    pub failed: i64,
    pub created_at: Time,
    pub started_at: Option<Time>,
}

fn mailing(row: SqliteRow) -> Result<Mailing> {
    let json = |column: &str| -> Result<String> { Ok(row.try_get(column)?) };
    let started_at: Option<String> = row.try_get("started_at")?;
    let status: String = row.try_get("status")?;
    Ok(Mailing {
        id: row.try_get("id")?,
        kind: serde_json::from_str(&json("kind")?).context("mailing kind")?,
        content: serde_json::from_str(&json("content")?).context("mailing content")?,
        to_channel: row.try_get("to_channel")?,
        audience: serde_json::from_str(&json("audience")?).context("mailing audience")?,
        rps: row.try_get("rps")?,
        report_chat: row.try_get("report_chat")?,
        report_message: row.try_get("report_message")?,
        finished: status == "done",
        channel_done: row.try_get("channel_done")?,
        channel_sent: row.try_get("channel_sent")?,
        cursor: row.try_get("cursor")?,
        total: row.try_get("total")?,
        sent: row.try_get("sent")?,
        failed: row.try_get("failed")?,
        created_at: from_db(row.try_get("created_at")?)?,
        started_at: started_at.as_deref().map(from_db).transpose()?,
    })
}

impl Database {
    /// Queues a mailing; the recipient count is fixed now (the audience is a snapshot).
    pub async fn enqueue_mailing(&self, new: &NewMailing) -> Result<i64> {
        let created_at = to_db(now());
        let total = match &new.audience {
            Audience::Nobody => 0,
            Audience::Users => {
                sqlx::query_scalar("SELECT count(*) FROM users WHERE joined_at <= ?")
                    .bind(&created_at)
                    .fetch_one(&self.pool)
                    .await?
            }
            Audience::Admins(ids) => i64::try_from(ids.len())?,
        };
        Ok(sqlx::query_scalar(
            "INSERT INTO mailings (kind, content, to_channel, audience, rps, report_chat,
                report_message, total, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(serde_json::to_string(&new.kind)?)
        .bind(serde_json::to_string(&new.content)?)
        .bind(new.to_channel)
        .bind(serde_json::to_string(&new.audience)?)
        .bind(new.rps)
        .bind(new.report_chat)
        .bind(new.report_message)
        .bind(total)
        .bind(created_at)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn mailing(&self, id: i64) -> Result<Option<Mailing>> {
        let row = sqlx::query("SELECT * FROM mailings WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(mailing).transpose()
    }

    /// The mailing to work on: an interrupted one first, else the oldest queued.
    /// A send that was in flight during a crash is counted as failed, never repeated.
    pub async fn next_mailing(&self) -> Result<Option<Mailing>> {
        let id: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM mailings WHERE status <> 'done'
             ORDER BY status = 'running' DESC, id LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(id) = id else { return Ok(None) };
        sqlx::query(
            "UPDATE mailings SET status = 'running', started_at = coalesce(started_at, ?),
                failed = failed + in_flight, in_flight = 0
             WHERE id = ?",
        )
        .bind(to_db(now()))
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.mailing(id).await
    }

    /// Recorded before the channel post, like `begin_send`: a crash never posts twice.
    pub async fn begin_channel(&self, id: i64) -> Result<()> {
        sqlx::query("UPDATE mailings SET channel_done = 1 WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn end_channel(&self, id: i64, sent: bool) -> Result<()> {
        sqlx::query("UPDATE mailings SET channel_sent = ? WHERE id = ?")
            .bind(sent)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Next registered recipients after `after`, limited to users present at enqueue time.
    pub async fn user_recipients(
        &self,
        mailing: &Mailing,
        after: Option<i64>,
        limit: i64,
    ) -> Result<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT user_id FROM users WHERE joined_at <= ? AND user_id > ?
             ORDER BY user_id LIMIT ?",
        )
        .bind(to_db(mailing.created_at))
        .bind(after.unwrap_or(i64::MIN))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Persisted before the send, so a crash can never lead to a second copy.
    pub async fn begin_send(&self, id: i64, recipient: i64) -> Result<()> {
        sqlx::query("UPDATE mailings SET cursor = ?, in_flight = 1 WHERE id = ?")
            .bind(recipient)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn end_send(&self, id: i64, delivered: bool) -> Result<()> {
        let sql = if delivered {
            "UPDATE mailings SET sent = sent + 1, in_flight = 0 WHERE id = ?"
        } else {
            "UPDATE mailings SET failed = failed + 1, in_flight = 0 WHERE id = ?"
        };
        sqlx::query(sql).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn finish_mailing(&self, id: i64) -> Result<Mailing> {
        sqlx::query("UPDATE mailings SET status = 'done', finished_at = ? WHERE id = ?")
            .bind(to_db(now()))
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.mailing(id).await?.context("mailing disappeared")
    }
}
