use anyhow::{Context, Result, bail, ensure};
use std::path::PathBuf;

const DEFAULT_DATA_DIR: &str = "/var/lib/tg-bot-giveaway-and-broadcast";

/// `DATA_DIR` alone, for commands that need no bot credentials (`--healthcheck`, `import`).
pub fn data_dir_from_env() -> PathBuf {
    std::env::var("DATA_DIR")
        .ok()
        .map(|dir| dir.trim().to_owned())
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| DEFAULT_DATA_DIR.into(), PathBuf::from)
}

// Deliberately no Debug: the configuration owns the bot token.
pub struct Config {
    pub token: String,
    pub admin_ids: Vec<i64>,
    pub channel_id: i64,
    pub join_url: String,
    pub broadcast_rps: u32,
    pub announce_rps: u32,
    pub max_retries: u32,
    pub sheets_enabled: bool,
    pub google_credentials_path: Option<PathBuf>,
    pub spreadsheet_id: Option<String>,
    pub log_level: String,
    pub data_dir: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Self::parse(|key| std::env::var(key).ok())
    }

    /// Every value is trimmed: production `.env` files were edited on Windows (CRLF)
    /// and some lines carry leading spaces, both of which Python tolerated.
    pub fn parse(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let value = |key: &str| {
            get(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let required = |key: &str| value(key).with_context(|| format!("{key} is required"));
        let number = |key: &str, default: u32, min: u32| -> Result<u32> {
            let n = match value(key) {
                Some(v) => v.parse().with_context(|| format!("invalid {key}"))?,
                None => default,
            };
            ensure!(n >= min, "{key} must be at least {min}");
            Ok(n)
        };
        let token = required("BOT_TOKEN")?;
        let admin_ids = required("ADMIN_IDS")?
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<i64>().context("invalid ADMIN_IDS entry"))
            .collect::<Result<Vec<_>>>()?;
        ensure!(!admin_ids.is_empty(), "ADMIN_IDS is required");
        Ok(Self {
            token,
            admin_ids,
            channel_id: required("CHANNEL_ID")?
                .parse()
                .context("CHANNEL_ID must be a numeric chat id")?,
            join_url: required("JOIN_URL")?,
            broadcast_rps: number("BROADCAST_RPS", 20, 1)?,
            announce_rps: number("ANNOUNCE_RPS", 20, 1)?,
            max_retries: number("MAX_RETRIES", 5, 0)?,
            sheets_enabled: flag(value("SHEETS_SYNC_ENABLED").as_deref())?,
            google_credentials_path: value("GOOGLE_CREDENTIALS_PATH").map(PathBuf::from),
            spreadsheet_id: value("SPREADSHEET_ID"),
            log_level: value("LOG_LEVEL").unwrap_or_else(|| "INFO".into()),
            data_dir: value("DATA_DIR").map_or_else(|| DEFAULT_DATA_DIR.into(), PathBuf::from),
        })
    }

    pub fn is_admin(&self, user_id: i64) -> bool {
        self.admin_ids.contains(&user_id)
    }
}

// Same spellings pydantic accepts for booleans.
fn flag(value: Option<&str>) -> Result<bool> {
    match value.map(str::to_ascii_lowercase).as_deref() {
        None => Ok(false),
        Some("1" | "true" | "t" | "yes" | "y" | "on") => Ok(true),
        Some("0" | "false" | "f" | "no" | "n" | "off") => Ok(false),
        Some(_) => bail!("invalid SHEETS_SYNC_ENABLED"),
    }
}
