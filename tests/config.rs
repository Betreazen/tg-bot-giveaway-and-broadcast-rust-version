use std::collections::HashMap;
use tg_bot_giveaway_and_broadcast::config::Config;

fn parse(overrides: &[(&str, &str)]) -> anyhow::Result<Config> {
    let mut env: HashMap<&str, &str> = HashMap::from([
        ("BOT_TOKEN", "123:abc"),
        ("ADMIN_IDS", "111, 222 ,333"),
        ("CHANNEL_ID", "-1001234567890"),
        ("JOIN_URL", "https://t.me/x?start=join"),
    ]);
    env.extend(overrides.iter().copied());
    Config::parse(|key| env.get(key).map(|v| v.to_string()))
}

#[test]
fn admin_ids_are_parsed_and_trimmed() {
    let config = parse(&[]).unwrap();
    assert_eq!(config.admin_ids, [111, 222, 333]);
    assert!(config.is_admin(222));
    assert!(!config.is_admin(999));
}

#[test]
fn channel_id_is_integer() {
    assert_eq!(parse(&[]).unwrap().channel_id, -1001234567890);
}

#[test]
fn defaults_match_python() {
    let config = parse(&[]).unwrap();
    assert_eq!(config.broadcast_rps, 20);
    assert_eq!(config.announce_rps, 20);
    assert_eq!(config.max_retries, 5);
    assert!(!config.sheets_enabled);
    assert_eq!(config.log_level, "INFO");
    assert_eq!(config.join_url, "https://t.me/x?start=join");
}

#[test]
fn crlf_and_spaces_are_trimmed() {
    let config = parse(&[
        ("BOT_TOKEN", " 123:abc\r"),
        ("BROADCAST_RPS", "10\r"),
        ("ANNOUNCE_RPS", " 7 "),
        ("SHEETS_SYNC_ENABLED", " true\r"),
        ("GOOGLE_CREDENTIALS_PATH", " /etc/x/sa.json\r"),
        ("SPREADSHEET_ID", " abc "),
        ("CHANNEL_ID", "-100123\r"),
    ])
    .unwrap();
    assert_eq!(config.token, "123:abc");
    assert_eq!(config.broadcast_rps, 10);
    assert_eq!(config.announce_rps, 7);
    assert!(config.sheets_enabled);
    assert_eq!(
        config.google_credentials_path.unwrap().to_str(),
        Some("/etc/x/sa.json")
    );
    assert_eq!(config.spreadsheet_id.as_deref(), Some("abc"));
    assert_eq!(config.channel_id, -100123);
}

#[test]
fn sheets_flag_accepts_pydantic_spellings() {
    for yes in ["true", "True", "1", "yes", "on"] {
        assert!(
            parse(&[("SHEETS_SYNC_ENABLED", yes)])
                .unwrap()
                .sheets_enabled
        );
    }
    for no in ["false", "0", "no", "off", ""] {
        assert!(
            !parse(&[("SHEETS_SYNC_ENABLED", no)])
                .unwrap()
                .sheets_enabled
        );
    }
    assert!(parse(&[("SHEETS_SYNC_ENABLED", "maybe")]).is_err());
}

#[test]
fn empty_optional_values_become_none() {
    let config = parse(&[("GOOGLE_CREDENTIALS_PATH", " "), ("SPREADSHEET_ID", "")]).unwrap();
    assert!(config.google_credentials_path.is_none());
    assert!(config.spreadsheet_id.is_none());
}

#[test]
fn required_values_are_enforced() {
    for key in ["BOT_TOKEN", "ADMIN_IDS", "CHANNEL_ID", "JOIN_URL"] {
        assert!(parse(&[(key, " ")]).is_err(), "{key} must be required");
    }
    assert!(parse(&[("CHANNEL_ID", "@channel")]).is_err());
    assert!(parse(&[("ADMIN_IDS", "1,abc")]).is_err());
}

#[test]
fn rates_must_be_positive() {
    assert!(parse(&[("BROADCAST_RPS", "0")]).is_err());
    assert!(parse(&[("ANNOUNCE_RPS", "-1")]).is_err());
    assert!(parse(&[("MAX_RETRIES", "x")]).is_err());
}
