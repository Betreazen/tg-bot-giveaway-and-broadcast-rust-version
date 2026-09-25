/// Bare lowercase Telegram username from `@name`, `t.me/name`, a full link or `name`.
pub fn parse_username(raw: &str) -> Option<String> {
    let text = raw.trim();
    let text = text.split(['?', '#']).next().unwrap_or_default();
    let text = text.trim_end_matches('/');
    let text = text.rsplit('/').next().unwrap_or_default();
    let text = text.trim().trim_start_matches('@').trim();
    let valid = (4..=32).contains(&text.len())
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    valid.then(|| text.to_ascii_lowercase())
}
