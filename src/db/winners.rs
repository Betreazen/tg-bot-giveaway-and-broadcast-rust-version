use super::{Database, Giveaway};
use crate::time::{Time, from_db, now, to_db};
use anyhow::Result;
use rand::Rng;
use sqlx::SqliteExecutor;

#[derive(Clone, Debug, PartialEq)]
pub struct Winner {
    pub user_id: i64,
    pub username_snapshot: Option<String>,
    pub giveaway_end_snapshot: Time,
}

#[derive(Debug)]
pub enum Draw {
    Winners(Vec<Winner>),
    /// No participants, or every participant is marked suspicious.
    NoParticipants,
}

impl Database {
    /// Draws winners once per giveaway: a repeated call returns the stored result.
    /// Suspicious participants are removed from the pool; the draw is uniform among the rest.
    pub async fn draw_winners(&self, giveaway: &Giveaway, rng: &mut impl Rng) -> Result<Draw> {
        let mut tx = self.pool.begin().await?;
        let existing = winners_in(&mut *tx, giveaway.id).await?;
        if !existing.is_empty() {
            return Ok(Draw::Winners(existing));
        }
        let pool: Vec<(i64, Option<String>)> = sqlx::query_as(
            "SELECT p.user_id, p.username_snapshot FROM participants p
             LEFT JOIN users u ON u.user_id = p.user_id
             WHERE p.giveaway_id = ? AND coalesce(u.is_suspicious, 0) = 0
             ORDER BY p.id",
        )
        .bind(giveaway.id)
        .fetch_all(&mut *tx)
        .await?;
        if pool.is_empty() {
            return Ok(Draw::NoParticipants);
        }
        let count = usize::try_from(giveaway.num_winners)?.min(pool.len());
        let snapshot = giveaway.ended_at.unwrap_or(giveaway.end_at);
        let created_at = to_db(now());
        for index in rand::seq::index::sample(rng, pool.len(), count) {
            let (user_id, username) = &pool[index];
            sqlx::query(
                "INSERT INTO winners
                    (giveaway_id, user_id, username_snapshot, giveaway_end_snapshot, created_at)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(giveaway.id)
            .bind(user_id)
            .bind(username)
            .bind(to_db(snapshot))
            .bind(&created_at)
            .execute(&mut *tx)
            .await?;
        }
        let winners = winners_in(&mut *tx, giveaway.id).await?;
        tx.commit().await?;
        Ok(Draw::Winners(winners))
    }

    pub async fn winners(&self, giveaway_id: i64) -> Result<Vec<Winner>> {
        winners_in(&self.pool, giveaway_id).await
    }
}

async fn winners_in(executor: impl SqliteExecutor<'_>, giveaway_id: i64) -> Result<Vec<Winner>> {
    let rows: Vec<(i64, Option<String>, String)> = sqlx::query_as(
        "SELECT user_id, username_snapshot, giveaway_end_snapshot FROM winners
         WHERE giveaway_id = ? ORDER BY id",
    )
    .bind(giveaway_id)
    .fetch_all(executor)
    .await?;
    rows.into_iter()
        .map(|(user_id, username_snapshot, end)| {
            Ok(Winner {
                user_id,
                username_snapshot,
                giveaway_end_snapshot: from_db(&end)?,
            })
        })
        .collect()
}

/// `1. @name` or `1. ID: 42` per line, as in the Python bot.
pub fn format_winner_list(winners: &[Winner]) -> String {
    if winners.is_empty() {
        return "No winners".into();
    }
    winners
        .iter()
        .enumerate()
        .map(
            |(i, w)| match w.username_snapshot.as_deref().filter(|n| !n.is_empty()) {
                Some(name) => format!("{}. @{name}", i + 1),
                None => format!("{}. ID: {}", i + 1, w.user_id),
            },
        )
        .collect::<Vec<_>>()
        .join("\n")
}
