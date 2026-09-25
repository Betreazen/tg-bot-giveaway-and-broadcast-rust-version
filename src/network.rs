use std::time::Duration;

// Bot::new uses teloxide-core's 17-second HTTP deadline. Keep the server's
// long-poll wait below it, leaving time for connection and response transfer.
pub const POLLING_TIMEOUT: Duration = Duration::from_secs(10);

// Google OAuth and Sheets calls; a full rewrite of ~18k rows is one request.
pub const SHEETS_TIMEOUT: Duration = Duration::from_secs(60);

/// reqwest errors embed the request URL, which contains the bot token.
pub fn redact(message: &str, token: &str) -> String {
    if token.is_empty() {
        message.to_owned()
    } else {
        message.replace(token, "[REDACTED]")
    }
}
