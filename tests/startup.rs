use std::process::{Command, Output};
use tg_bot_giveaway_and_broadcast::network::redact;

const VARS: [&str; 6] = [
    "BOT_TOKEN",
    "ADMIN_IDS",
    "CHANNEL_ID",
    "JOIN_URL",
    "DATA_DIR",
    "RUST_LOG",
];

fn bot(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tg-bot-giveaway-and-broadcast"));
    for var in VARS {
        command.env_remove(var);
    }
    command
        .args(args)
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn full_env(data: &str) -> Vec<(&'static str, String)> {
    vec![
        (
            "BOT_TOKEN",
            "123456789:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
        ),
        ("ADMIN_IDS", "1".into()),
        ("CHANNEL_ID", "-1001".into()),
        ("JOIN_URL", "https://t.me/x".into()),
        ("DATA_DIR", data.into()),
    ]
}

fn run_with(args: &[&str], env: &[(&'static str, String)]) -> Output {
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    bot(args, &env)
}

#[test]
fn version_is_printed() {
    let output = bot(&["--version"], &[]);
    assert!(output.status.success());
    assert!(stdout(&output).starts_with("tg-bot-giveaway-and-broadcast "));
}

#[test]
fn missing_token_exits_before_creating_data() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let output = bot(&[], &[("DATA_DIR", data.to_str().unwrap())]);
    assert!(!output.status.success());
    assert!(!data.exists());
    assert!(stderr(&output).contains("BOT_TOKEN"));
}

#[test]
fn unknown_argument_is_rejected() {
    let output = bot(&["--bogus"], &[]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("--check"));
}

#[test]
fn check_creates_the_database_without_telegram() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let output = run_with(&["--check"], &full_env(data.to_str().unwrap()));
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("ok"));
    assert!(data.join("bot.db").exists());
}

#[test]
fn healthcheck_needs_a_fresh_heartbeat() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().to_str().unwrap();
    assert!(run_with(&["--check"], &full_env(data)).status.success());
    let output = bot(&["--healthcheck"], &[("DATA_DIR", data)]);
    assert!(!output.status.success());
    std::fs::write(root.path().join(".heartbeat"), "1").unwrap();
    let output = bot(&["--healthcheck"], &[("DATA_DIR", data)]);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn import_command_prints_the_report() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/pg");
    let output = bot(
        &["import", fixtures],
        &[("DATA_DIR", data.to_str().unwrap())],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    for key in [
        "users=",
        "suspicious=",
        "giveaways=",
        "participants=",
        "winners=",
    ] {
        assert!(
            out.lines().any(|l| l.starts_with(key)),
            "{key} missing: {out}"
        );
    }
    let again = bot(
        &["import", fixtures],
        &[("DATA_DIR", data.to_str().unwrap())],
    );
    assert!(!again.status.success(), "second import must be refused");
}

#[test]
fn token_is_redacted() {
    let token = "123456789:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let message =
        format!("error sending request for url (https://api.telegram.org/bot{token}/GetUpdates)");
    let redacted = redact(&message, token);
    assert!(!redacted.contains(token));
    assert!(redacted.contains("bot[REDACTED]/GetUpdates"));
}
