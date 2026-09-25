#![allow(dead_code)] // each test binary uses a different subset

use serde_json::{Value, json};
use teloxide::Bot;
use tg_bot_giveaway_and_broadcast::{config::Config, db::Database};
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::method};

pub const ADMIN: i64 = 99;
pub const CHANNEL: i64 = -100500;
pub const JOIN_URL: &str = "https://t.me/test_bot?start=join";

pub fn config(dir: &std::path::Path) -> Config {
    let mut config = Config::parse(|key| {
        Some(
            match key {
                "BOT_TOKEN" => "123:fake",
                "ADMIN_IDS" => "99,98",
                "CHANNEL_ID" => "-100500",
                "JOIN_URL" => JOIN_URL,
                "BROADCAST_RPS" | "ANNOUNCE_RPS" => "1000",
                "MAX_RETRIES" => "2",
                _ => return None,
            }
            .into(),
        )
    })
    .unwrap();
    config.data_dir = dir.to_owned();
    config
}

pub async fn db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path()).await.unwrap();
    (dir, db)
}

pub fn ok_message(chat: i64) -> Value {
    json!({"ok": true, "result": {"message_id": 123, "date": 0,
        "chat": {"id": chat, "type": "private"}, "text": "ok"}})
}

pub fn api_error(code: u16, description: &str) -> Value {
    json!({"ok": false, "error_code": code, "description": description})
}

pub fn retry_after(seconds: u64) -> Value {
    json!({"ok": false, "error_code": 429,
        "description": format!("Too Many Requests: retry after {seconds}"),
        "parameters": {"retry_after": seconds}})
}

/// A server that answers every POST with a successful message.
pub async fn server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_message(1)))
        .mount(&server)
        .await;
    server
}

pub fn bot(server: &MockServer) -> Bot {
    Bot::new("123:fake").set_api_url(server.uri().parse().unwrap())
}

/// Telegram method name and JSON (or multipart-decoded) body of a request.
pub fn call(request: &Request) -> (String, String) {
    let method = request.url.path().rsplit('/').next().unwrap().to_owned();
    (method, String::from_utf8_lossy(&request.body).into_owned())
}

pub async fn calls(server: &MockServer) -> Vec<(String, String)> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(call)
        .collect()
}

/// Chat ids of requests to `method`, in order.
pub async fn chats(server: &MockServer, method: &str) -> Vec<i64> {
    calls(server)
        .await
        .iter()
        .filter(|(m, _)| m.eq_ignore_ascii_case(method))
        .map(|(_, body)| chat_of(body))
        .collect()
}

pub fn chat_of(body: &str) -> i64 {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        return value["chat_id"].as_i64().unwrap();
    }
    // multipart/form-data: the value follows the `name="chat_id"` part header.
    let part = body.split("name=\"chat_id\"").nth(1).unwrap();
    part.trim_start_matches(['\r', '\n'])
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}
