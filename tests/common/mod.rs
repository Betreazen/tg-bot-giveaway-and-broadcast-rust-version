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
                "BROADCAST_RPS" => "1000",
                "ANNOUNCE_RPS" => "999",
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
    Mock::given(wiremock::matchers::path_regex(
        "(?i)/(answercallbackquery|deletemessage)$",
    ))
    .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": true})))
    .mount(&server)
    .await;
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

// ---- handler-level helpers ---------------------------------------------------

use std::sync::Arc;
use teloxide::types::{CallbackQuery, Message};
use tg_bot_giveaway_and_broadcast::handlers::App;
use wiremock::matchers::path_regex;

pub async fn app(server: &MockServer) -> (tempfile::TempDir, Arc<App>, Bot) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::open(config(dir.path())).await.unwrap();
    (dir, app, bot(server))
}

fn user_json(user: i64, username: Option<&str>) -> Value {
    let mut user = json!({"id": user, "is_bot": false, "first_name": "Test"});
    if let Some(name) = username {
        user["username"] = json!(name);
    }
    user
}

fn message_json(user: i64, chat: Value, username: Option<&str>) -> Value {
    json!({"message_id": 1, "date": 0, "chat": chat, "from": user_json(user, username)})
}

pub fn private_chat(user: i64) -> Value {
    json!({"id": user, "type": "private", "first_name": "Test"})
}

pub fn text_from(user: i64, username: Option<&str>, text: &str) -> Message {
    let mut value = message_json(user, private_chat(user), username);
    value["text"] = json!(text);
    serde_json::from_value(value).unwrap()
}

pub fn text(user: i64, text: &str) -> Message {
    text_from(user, Some("tester"), text)
}

pub fn group_text(user: i64, text: &str) -> Message {
    let chat = json!({"id": -555, "type": "group", "title": "g"});
    let mut value = message_json(user, chat, Some("tester"));
    value["text"] = json!(text);
    serde_json::from_value(value).unwrap()
}

/// A private message carrying media of `kind` (photo, video, animation, document).
pub fn media(user: i64, kind: &str, caption: Option<&str>) -> Message {
    let mut value = message_json(user, private_chat(user), Some("tester"));
    let file = |id: &str| json!({"file_id": id, "file_unique_id": format!("u{id}")});
    match kind {
        "photo" => {
            let mut small = file("small");
            small["width"] = json!(90);
            small["height"] = json!(90);
            let mut big = file("big");
            big["width"] = json!(900);
            big["height"] = json!(900);
            value["photo"] = json!([small, big]);
        }
        "video" | "animation" => {
            let mut item = file(kind);
            item["width"] = json!(1);
            item["height"] = json!(1);
            item["duration"] = json!(1);
            // teloxide requires the field although the Bot API marks it optional.
            item["mime_type"] = json!("video/mp4");
            if kind == "animation" {
                value["document"] = file(kind);
            }
            value[kind] = item;
        }
        "document" => value["document"] = file("document"),
        "sticker" => {
            let mut item = file("sticker");
            item["width"] = json!(1);
            item["height"] = json!(1);
            item["type"] = json!("regular");
            item["is_animated"] = json!(false);
            item["is_video"] = json!(false);
            value["sticker"] = item;
        }
        other => panic!("unknown media {other}"),
    }
    if let Some(caption) = caption {
        value["caption"] = json!(caption);
    }
    serde_json::from_value(value).unwrap()
}

pub fn callback(user: i64, data: &str) -> CallbackQuery {
    let mut message = message_json(0, private_chat(user), None);
    message["from"] = json!({"id": 1, "is_bot": true, "first_name": "Bot"});
    message["message_id"] = json!(55);
    message["text"] = json!("menu");
    serde_json::from_value(json!({"id": "cb", "from": user_json(user, Some("tester")),
        "chat_instance": "ci", "data": data, "message": message}))
    .unwrap()
}

/// Every Telegram call as (method, JSON body); multipart bodies become `{"raw": ...}`.
pub async fn sent(server: &MockServer) -> Vec<(String, Value)> {
    calls(server)
        .await
        .into_iter()
        .map(|(m, body)| {
            let value = serde_json::from_str(&body).unwrap_or_else(|_| json!({"raw": body}));
            (m.to_ascii_lowercase(), value)
        })
        .filter(|(m, _)| m != "getchatmember")
        .collect()
}

/// Texts of sendMessage/editMessageText calls, in order.
pub async fn texts(server: &MockServer) -> Vec<String> {
    sent(server)
        .await
        .into_iter()
        .filter(|(m, _)| m == "sendmessage" || m == "editmessagetext")
        .map(|(_, body)| body["text"].as_str().unwrap().to_owned())
        .collect()
}

pub async fn last(server: &MockServer) -> (String, Value) {
    sent(server).await.pop().expect("no Telegram calls")
}

/// Callback data of the inline keyboard in a request body, row by row.
pub fn buttons(body: &Value) -> Vec<Vec<String>> {
    body["reply_markup"]["inline_keyboard"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    row.as_array()
                        .unwrap()
                        .iter()
                        .map(|b| {
                            b["callback_data"]
                                .as_str()
                                .or(b["url"].as_str())
                                .unwrap()
                                .to_owned()
                        })
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Answers getChatMember with `status` for every user.
pub async fn membership(server: &MockServer, status: &str) {
    Mock::given(path_regex("(?i)/getchatmember$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true,
            "result": member(status)})))
        .with_priority(1)
        .mount(server)
        .await;
}

pub async fn active_giveaway(app: &App) -> i64 {
    use chrono::Duration;
    use tg_bot_giveaway_and_broadcast::{db::NewGiveaway, state::Media, time::now};
    app.db
        .create_giveaway(&NewGiveaway {
            start_at: now(),
            end_at: now() + Duration::days(3),
            description: "Приз <b>iPhone</b>".into(),
            num_winners: 2,
            media: Media {
                kind: "photo".into(),
                file_id: "PHOTO".into(),
            },
            created_by: ADMIN,
        })
        .await
        .unwrap()
}

fn member(status: &str) -> Value {
    let mut member = json!({"status": status, "is_anonymous": false,
        "user": {"id": 1, "is_bot": false, "first_name": "T"}});
    if status == "administrator" {
        for right in [
            "can_be_edited",
            "can_manage_chat",
            "can_change_info",
            "can_delete_messages",
            "can_manage_video_chats",
            "can_invite_users",
            "can_restrict_members",
            "can_promote_members",
            "can_post_stories",
            "can_edit_stories",
            "can_delete_stories",
        ] {
            member[right] = json!(false);
        }
    }
    member
}

/// Body of the last call to `method` (case-insensitive).
pub async fn last_of(server: &MockServer, method: &str) -> Value {
    let method = method.to_ascii_lowercase();
    sent(server)
        .await
        .into_iter()
        .rev()
        .find(|(m, _)| *m == method)
        .unwrap_or_else(|| panic!("no {method} call"))
        .1
}
