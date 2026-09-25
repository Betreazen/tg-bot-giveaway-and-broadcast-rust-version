use std::time::Duration;

// Bot::new uses teloxide-core's 17-second HTTP deadline. Keep the server's
// long-poll wait below it, leaving time for connection and response transfer.
pub const POLLING_TIMEOUT: Duration = Duration::from_secs(10);

// Google OAuth and Sheets calls; a full rewrite of ~18k rows is one request.
pub const SHEETS_TIMEOUT: Duration = Duration::from_secs(60);
