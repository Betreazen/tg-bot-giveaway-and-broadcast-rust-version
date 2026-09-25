//! Google Sheets sync against fake OAuth and Sheets endpoints.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};
use serde_json::{Value, json};
use std::collections::HashMap;
use tg_bot_giveaway_and_broadcast::{
    config::Config,
    db::Database,
    sheets::{ServiceAccount, SyncOutcome, jwt, sync_all, sync_with},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_string_contains, header, method, path, path_regex, query_param},
};

const SHEET: &str = "/v4/spreadsheets/sheet-id";
const TOKEN: &str = "test-access-token";
const BEARER: &str = "Bearer test-access-token";

fn account() -> ServiceAccount {
    serde_json::from_str(include_str!("fixtures/sheets_test_key.json")).unwrap()
}

fn decode(part: &str) -> Value {
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
}

#[test]
fn jwt_has_google_claims_and_valid_signature() {
    let token = jwt(&account(), 1_700_000_000).unwrap();
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(
        decode(parts[0]),
        json!({"alg": "RS256", "typ": "JWT", "kid": "test"})
    );
    assert_eq!(
        decode(parts[1]),
        json!({
            "iss": "test@example.iam.gserviceaccount.com",
            "scope": "https://www.googleapis.com/auth/spreadsheets https://www.googleapis.com/auth/drive",
            "aud": "https://oauth2.googleapis.com/token",
            "iat": 1_700_000_000,
            "exp": 1_700_003_600,
        })
    );
    let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
    let key = UnparsedPublicKey::new(
        &RSA_PKCS1_2048_8192_SHA256,
        include_bytes!("fixtures/sheets_test_pub.der"),
    );
    let signed = format!("{}.{}", parts[0], parts[1]);
    key.verify(signed.as_bytes(), &signature).unwrap();
}

fn config(overrides: &[(&str, &str)]) -> Config {
    let mut env: HashMap<&str, &str> = HashMap::from([
        ("BOT_TOKEN", "123:abc"),
        ("ADMIN_IDS", "1"),
        ("CHANNEL_ID", "-100"),
        ("JOIN_URL", "https://t.me/x"),
    ]);
    env.extend(overrides.iter().copied());
    Config::parse(|key| env.get(key).map(|v| v.to_string())).unwrap()
}

#[tokio::test]
async fn sync_all_skips_when_disabled_or_unconfigured() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path()).await.unwrap();
    let missing = "/nonexistent/service_account.json";
    for env in [
        vec![
            ("GOOGLE_CREDENTIALS_PATH", missing),
            ("SPREADSHEET_ID", "x"),
        ],
        vec![
            ("SHEETS_SYNC_ENABLED", "false"),
            ("GOOGLE_CREDENTIALS_PATH", missing),
            ("SPREADSHEET_ID", "x"),
        ],
        vec![("SHEETS_SYNC_ENABLED", "true"), ("SPREADSHEET_ID", "x")],
        vec![
            ("SHEETS_SYNC_ENABLED", "true"),
            ("GOOGLE_CREDENTIALS_PATH", missing),
        ],
    ] {
        let outcome = sync_all(&db, &config(&env)).await.unwrap();
        assert!(matches!(outcome, SyncOutcome::Skipped));
    }
}

#[tokio::test]
async fn sync_all_fails_when_key_file_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path()).await.unwrap();
    let env = [
        ("SHEETS_SYNC_ENABLED", "true"),
        (
            "GOOGLE_CREDENTIALS_PATH",
            "/nonexistent/service_account.json",
        ),
        ("SPREADSHEET_ID", "x"),
    ];
    assert!(sync_all(&db, &config(&env)).await.is_err());
}

async fn exec(db: &Database, sql: &str) {
    sqlx::raw_sql(sql).execute(db.pool()).await.unwrap();
}

async fn seeded_db(dir: &tempfile::TempDir) -> (Database, String) {
    let db = Database::open(dir.path()).await.unwrap();
    let long = "абвгдеёжзи".repeat(6);
    exec(
        &db,
        "INSERT INTO users VALUES
        (1001, 'alice', '2026-01-01T09:00:00.000000Z', 0),
        (1002, 'bob', '2026-01-02T09:00:00.000000Z', 1),
        (1003, NULL, '2025-12-31T21:30:00.000000Z', 0)",
    )
    .await;
    exec(&db, &format!("INSERT INTO giveaways (id, start_at, end_at, description, num_winners,
        is_active, announce_media_file_id, announce_media_type, created_by_admin_id, created_at) VALUES
        (1, '2026-01-01T09:00:00.000000Z', '2026-01-04T09:00:00.000000Z', 'Первый', 1, 0, 'F', 'photo', 42, '2026-01-01T08:00:00.000000Z'),
        (2, '2026-01-05T09:00:00.000000Z', '2026-01-12T08:00:00.000000Z', '{long}', 1, 1, 'F', 'photo', 42, '2026-01-05T08:00:00.000000Z')")).await;
    exec(&db, "INSERT INTO participants (id, giveaway_id, user_id, joined_at, username_snapshot, giveaway_end_snapshot) VALUES
        (1, 1, 1001, '2026-01-01T10:00:00.000000Z', 'alice', '2026-01-04T09:00:00.000000Z'),
        (2, 1, 1002, '2026-01-01T11:00:00.000000Z', 'bob', '2026-01-04T09:00:00.000000Z'),
        (3, 2, 1001, '2026-01-05T10:00:00.000000Z', 'alice', '2026-01-12T08:00:00.000000Z'),
        (4, 2, 1003, '2026-01-05T11:00:00.000000Z', NULL, '2026-01-12T08:00:00.000000Z'),
        (5, 99, 1002, '2026-01-06T10:00:00.000000Z', 'bob', '2026-01-12T08:00:00.000000Z')").await;
    exec(&db, "INSERT INTO winners (id, giveaway_id, user_id, username_snapshot, giveaway_end_snapshot, created_at) VALUES
        (1, 1, 1002, NULL, '2026-01-04T09:00:00.000000Z', '2026-01-04T10:00:00.000000Z')").await;
    let short: String = long.chars().take(47).collect();
    (db, format!("{short}..."))
}

fn grid(id: i64, title: &str, rows: i64, cols: i64) -> Value {
    json!({"properties": {"sheetId": id, "title": title, "index": id,
        "gridProperties": {"rowCount": rows, "columnCount": cols}}})
}

async fn mount_google(server: &MockServer, put_status: u16) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer",
        ))
        .and(body_string_contains("assertion="))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"access_token": TOKEN, "expires_in": 3599, "token_type": "Bearer"}),
        ))
        .expect(1)
        .mount(server)
        .await;
    let sheets = json!({"sheets": [
        grid(0, "Overview", 1000, 26),
        grid(7, "Users", 5, 2),
        grid(8, "Participants", 1000, 26),
        grid(9, "Winners", 1000, 26),
    ]});
    Mock::given(method("GET"))
        .and(path(SHEET))
        .and(query_param("fields", "sheets.properties"))
        .and(header("Authorization", BEARER))
        .respond_with(ResponseTemplate::new(200).set_body_json(sheets))
        .mount(server)
        .await;
    for (verb, route, status) in [
        ("POST", format!("^{SHEET}:batchUpdate$"), 200),
        ("POST", format!("^{SHEET}/values/'[^/]+':clear$"), 200),
        ("PUT", format!("^{SHEET}/values/'[^/]+'!A1$"), put_status),
    ] {
        Mock::given(method(verb))
            .and(path_regex(route))
            .and(header("Authorization", BEARER))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({})))
            .mount(server)
            .await;
    }
}

async fn run(server: &MockServer, db: &Database) -> anyhow::Result<()> {
    let mut account = account();
    account.token_uri = format!("{}/token", server.uri());
    sync_with(db, &account, "sheet-id", &server.uri()).await
}

fn body(request: &wiremock::Request) -> Value {
    serde_json::from_slice(&request.body).unwrap()
}

#[tokio::test]
async fn sync_rewrites_all_five_sheets() {
    let dir = tempfile::tempdir().unwrap();
    let (db, short) = seeded_db(&dir).await;
    let server = MockServer::start().await;
    mount_google(&server, 200).await;
    run(&server, &db).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let calls: Vec<String> = requests
        .iter()
        .map(|r| format!("{} {}", r.method, r.url.path()))
        .collect();
    let v = format!("POST {SHEET}/values/");
    let p = format!("PUT {SHEET}/values/");
    let batch = format!("POST {SHEET}:batchUpdate");
    assert_eq!(
        calls,
        [
            "POST /token".to_owned(),
            format!("GET {SHEET}"),
            format!("{v}'Overview':clear"),
            format!("{p}'Overview'!A1"),
            batch.clone(),
            format!("{v}'Users':clear"),
            format!("{p}'Users'!A1"),
            format!("{v}'Participants':clear"),
            format!("{p}'Participants'!A1"),
            format!("{v}'Winners':clear"),
            format!("{p}'Winners'!A1"),
            batch,
            format!("{v}'Giveaways%20Summary':clear"),
            format!("{p}'Giveaways%20Summary'!A1"),
        ]
    );
    assert_eq!(
        body(&requests[4]),
        json!({"requests": [{"updateSheetProperties": {
            "properties": {"sheetId": 7, "gridProperties": {"rowCount": 14, "columnCount": 4}},
            "fields": "gridProperties.rowCount,gridProperties.columnCount"}}]})
    );
    assert_eq!(
        body(&requests[11]),
        json!({"requests": [{"addSheet": {"properties": {"title": "Giveaways Summary",
            "gridProperties": {"rowCount": 13, "columnCount": 11}}}}]})
    );
    for put in requests.iter().filter(|r| r.method.as_str() == "PUT") {
        assert_eq!(put.url.query(), Some("valueInputOption=RAW"));
    }

    let overview = body(&requests[3]);
    let rows = overview["values"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    assert_eq!(
        rows[..7],
        json!([
            ["Показатель", "Значение"],
            ["Уникальных пользователей (= строк в Users)", 3],
            ["Всего участий (= строк в Participants)", 5],
            ["Уникальных участников", 3],
            ["Подозрительных аккаунтов", 1],
            ["Всего розыгрышей", 2],
            ["Всего победителей (= строк в Winners)", 1],
        ])
        .as_array()
        .unwrap()[..]
    );
    assert_eq!(rows[7][0], "Обновлено (МСК)");
    let updated = rows[7][1].as_str().unwrap();
    assert_eq!(updated.len(), "2026-01-01 12:00".len());
    assert!(chrono::NaiveDateTime::parse_from_str(updated, "%Y-%m-%d %H:%M").is_ok());

    assert_eq!(
        body(&requests[6]),
        json!({"values": [
            ["User ID", "Username", "Joined At (MSK)", "Suspicious"],
            [1003, "", "2026-01-01 00:30", ""],
            [1001, "alice", "2026-01-01 12:00", ""],
            [1002, "bob", "2026-01-02 12:00", "Да"],
        ]})
    );
    assert_eq!(
        body(&requests[8]),
        json!({"values": [
            ["Giveaway ID", "User ID", "Username", "Joined At (MSK)", "Giveaway Start (MSK)", "Giveaway End (MSK)"],
            [1, 1001, "alice", "2026-01-01 13:00", "2026-01-01 12:00", "2026-01-04 12:00"],
            [1, 1002, "bob", "2026-01-01 14:00", "2026-01-01 12:00", "2026-01-04 12:00"],
            [2, 1001, "alice", "2026-01-05 13:00", "2026-01-05 12:00", "2026-01-12 11:00"],
            [2, 1003, "", "2026-01-05 14:00", "2026-01-05 12:00", "2026-01-12 11:00"],
            [99, 1002, "bob", "2026-01-06 13:00", "", ""],
        ]})
    );
    assert_eq!(
        body(&requests[10]),
        json!({"values": [
            ["Giveaway ID", "User ID", "Username", "Selected At (MSK)"],
            [1, 1002, "", "2026-01-04 13:00"],
        ]})
    );
    assert_eq!(
        body(&requests[13]),
        json!({"values": [
            ["ID", "Description", "Start (MSK)", "End (MSK)", "Duration (days)",
             "Total Participants", "Winners Count", "New Participants", "Status",
             "Created At (MSK)", "Created By Admin"],
            [1, "Первый", "2026-01-01 12:00", "2026-01-04 12:00", 3, 2, 1, 2, "Завершен", "2026-01-01 11:00", 42],
            [2, short, "2026-01-05 12:00", "2026-01-12 11:00", 6, 2, 0, 1, "Активен", "2026-01-05 11:00", 42],
        ]})
    );
}

#[tokio::test]
async fn sync_fails_on_server_error_without_leaking_token() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = seeded_db(&dir).await;
    let server = MockServer::start().await;
    mount_google(&server, 500).await;
    let error = format!("{:#}", run(&server, &db).await.unwrap_err());
    assert!(error.contains("500"), "{error}");
    assert!(!error.contains(TOKEN), "{error}");
}
