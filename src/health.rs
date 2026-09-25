//! Liveness for `--healthcheck`: a heartbeat file written by the running bot plus a
//! read-only SQLite check. It says nothing about Telegram reachability.
use anyhow::{Result, ensure};
use std::{path::Path, time::Duration};

const FILE: &str = ".heartbeat";
const INTERVAL: Duration = Duration::from_secs(15);
const STALE_AFTER: Duration = Duration::from_secs(90);

pub async fn heartbeat(data_dir: &Path) {
    let path = data_dir.join(FILE);
    loop {
        let stamp = crate::time::now().timestamp().to_string();
        if let Err(error) = tokio::fs::write(&path, stamp).await {
            tracing::warn!(%error, "heartbeat write failed");
        }
        tokio::time::sleep(INTERVAL).await;
    }
}

pub async fn healthcheck(data_dir: &Path) -> Result<()> {
    let modified = tokio::fs::metadata(data_dir.join(FILE)).await?.modified()?;
    ensure!(modified.elapsed()? < STALE_AFTER, "heartbeat is stale");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(data_dir.join("bot.db"))
        .read_only(true)
        .busy_timeout(Duration::from_secs(3));
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let result: String = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(&pool)
        .await?;
    pool.close().await;
    ensure!(result == "ok", "SQLite check failed: {result}");
    Ok(())
}
