use super::Database;
use crate::{
    state::Dialogue,
    time::{now, to_db},
};
use anyhow::{Context, Result};

impl Database {
    pub async fn load_dialogue(&self, user_id: i64) -> Result<Option<Dialogue>> {
        let state: Option<String> =
            sqlx::query_scalar("SELECT state FROM dialogues WHERE user_id = ?")
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await?;
        state
            .map(|s| serde_json::from_str(&s).context("invalid stored dialogue"))
            .transpose()
    }

    pub async fn save_dialogue(&self, user_id: i64, dialogue: &Dialogue) -> Result<()> {
        sqlx::query(
            "INSERT INTO dialogues (user_id, state, updated_at) VALUES (?, ?, ?)
             ON CONFLICT(user_id) DO UPDATE SET state = excluded.state,
                updated_at = excluded.updated_at",
        )
        .bind(user_id)
        .bind(serde_json::to_string(dialogue)?)
        .bind(to_db(now()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn clear_dialogue(&self, user_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM dialogues WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
