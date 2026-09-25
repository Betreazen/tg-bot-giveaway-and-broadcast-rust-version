use chrono::{Duration, TimeZone, Utc};
use std::{fs, path::Path};
use tg_bot_giveaway_and_broadcast::{
    db::{Database, NewGiveaway, Winner},
    import::{self, ImportReport},
    state::Media,
    time::Time,
};

const TABLES: [&str; 4] = ["users", "giveaways", "participants", "winners"];

async fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path()).await.unwrap();
    (dir, db)
}

/// A writable copy of `tests/fixtures/pg`.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pg");
    for table in TABLES {
        let name = format!("{table}.jsonl");
        fs::copy(source.join(&name), dir.path().join(&name)).unwrap();
    }
    dir
}

fn append(dir: &Path, table: &str, line: &str) {
    let path = dir.join(format!("{table}.jsonl"));
    let content = fs::read_to_string(&path).unwrap() + line + "\n";
    fs::write(path, content).unwrap();
}

fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32, micros: i64) -> Time {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap() + Duration::microseconds(micros)
}

async fn counts(db: &Database) -> Vec<i64> {
    let mut counts = Vec::new();
    for table in TABLES {
        let sql = format!("SELECT count(*) FROM {table}");
        counts.push(sqlx::query_scalar(&sql).fetch_one(db.pool()).await.unwrap());
    }
    counts
}

async fn import_err(db: &Database, dir: &Path) -> String {
    format!("{:#}", import::run(db, dir).await.unwrap_err())
}

#[tokio::test]
async fn imports_fixture_and_reports_counts() {
    let (_db_dir, db) = open().await;
    let source = fixture();
    let report = import::run(&db, source.path()).await.unwrap();
    assert_eq!(
        report,
        ImportReport {
            users: 4,
            suspicious: 1,
            giveaways: 2,
            participants: 4,
            winners: 2,
        }
    );
    assert_eq!(
        report.to_string(),
        "users=4\nsuspicious=1\ngiveaways=2\nparticipants=4\nwinners=2"
    );
    assert_eq!(db.user_count().await.unwrap(), 4);
    assert_eq!(
        db.suspicious_users().await.unwrap(),
        [(1003, Some("Борис_77".into()))]
    );
    assert_eq!(db.active_giveaway().await.unwrap(), None);
    assert_eq!(db.participant_count(3).await.unwrap(), 3);
    assert_eq!(db.participant_count(7).await.unwrap(), 1);
    assert!(db.is_participant(3, 1003).await.unwrap());
    db.check().await.unwrap();
}

#[tokio::test]
async fn imported_giveaways_and_winners_match_the_dump() {
    let (_db_dir, db) = open().await;
    import::run(&db, fixture().path()).await.unwrap();

    let first = db.giveaway(3).await.unwrap().unwrap();
    assert_eq!(
        first.description,
        "Розыгрыш \"iPhone\"\nУсловия: подписка на канал"
    );
    assert_eq!(first.start_at, utc(2025, 11, 10, 9, 0, 0, 0));
    assert_eq!(first.end_at, utc(2025, 11, 17, 9, 0, 0, 0));
    assert_eq!(first.ended_at, Some(utc(2025, 11, 17, 9, 0, 3, 100_000)));
    assert_eq!(first.num_winners, 2);
    assert!(!first.is_active);
    assert_eq!(
        first.media,
        Media {
            kind: "photo".into(),
            file_id: "AgACAgIAAxkBAAIB".into(),
        }
    );
    let second = db.giveaway(7).await.unwrap().unwrap();
    assert_eq!(second.start_at, utc(2025, 12, 1, 12, 0, 0, 1));
    assert_eq!(second.ended_at, None);

    let end = utc(2025, 11, 17, 9, 0, 3, 100_000);
    assert_eq!(
        db.winners(3).await.unwrap(),
        [
            Winner {
                user_id: 1001,
                username_snapshot: Some("alice".into()),
                giveaway_end_snapshot: end,
            },
            Winner {
                user_id: 1002,
                username_snapshot: None,
                giveaway_end_snapshot: end,
            },
        ]
    );
}

#[tokio::test]
async fn imported_rows_are_stored_normalized_with_original_ids() {
    let (_db_dir, db) = open().await;
    import::run(&db, fixture().path()).await.unwrap();

    let users: Vec<(i64, Option<String>, String, i64)> = sqlx::query_as(
        "SELECT user_id, username, joined_at, is_suspicious FROM users ORDER BY user_id",
    )
    .fetch_all(db.pool())
    .await
    .unwrap();
    assert_eq!(
        users,
        [
            (
                1001,
                Some("alice".into()),
                "2025-11-02T10:15:30.123456Z".into(),
                0
            ),
            (1002, None, "2025-11-03T08:00:00.000000Z".into(), 0),
            (
                1003,
                Some("Борис_77".into()),
                "2025-11-04T21:30:05.500000Z".into(),
                1
            ),
            (
                5_000_000_001,
                Some("o'neil \"quoted\"".into()),
                "2025-12-01T00:00:00.120000Z".into(),
                0
            ),
        ]
    );

    let giveaway: (Option<String>, i64, String, Option<String>) = sqlx::query_as(
        "SELECT announce_text, created_by_admin_id, created_at, ended_at FROM giveaways WHERE id = 7",
    )
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(
        giveaway,
        (
            Some("Итоги скоро".into()),
            42,
            "2025-11-30T20:00:00.000000Z".into(),
            None
        )
    );

    let participants: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, joined_at FROM participants ORDER BY id")
            .fetch_all(db.pool())
            .await
            .unwrap();
    assert_eq!(
        participants,
        [
            (10, "2025-11-10T09:05:00.250000Z".into()),
            (11, "2025-11-10T10:00:00.000000Z".into()),
            (12, "2025-11-11T00:00:00.999999Z".into()),
            (15, "2025-12-02T00:00:00.000000Z".into()),
        ]
    );

    let winners: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, created_at FROM winners ORDER BY id")
            .fetch_all(db.pool())
            .await
            .unwrap();
    assert_eq!(
        winners,
        [
            (4, "2025-11-17T09:00:04.500000Z".into()),
            (5, "2025-11-17T09:00:04.500000Z".into()),
        ]
    );
}

#[tokio::test]
async fn ids_continue_after_the_imported_maximum() {
    let (_db_dir, db) = open().await;
    import::run(&db, fixture().path()).await.unwrap();
    let start = utc(2026, 1, 1, 12, 0, 0, 0);
    let id = db
        .create_giveaway(&NewGiveaway {
            start_at: start,
            end_at: start + Duration::days(1),
            description: "new".into(),
            num_winners: 1,
            media: Media {
                kind: "photo".into(),
                file_id: "FILE".into(),
            },
            created_by: 1,
        })
        .await
        .unwrap();
    assert_eq!(id, 8);
    db.add_participant(id, 1002, None, start).await.unwrap();
    let participant: i64 = sqlx::query_scalar("SELECT id FROM participants WHERE giveaway_id = ?")
        .bind(id)
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(participant, 16);
}

#[tokio::test]
async fn refuses_to_import_into_a_database_with_data() {
    let (_db_dir, db) = open().await;
    let source = fixture();
    import::run(&db, source.path()).await.unwrap();
    let before = counts(&db).await;
    let err = import_err(&db, source.path()).await;
    assert!(err.contains("already"), "{err}");
    assert_eq!(counts(&db).await, before);

    let (_db_dir, db) = open().await;
    db.upsert_user(1, Some("bob")).await.unwrap();
    import_err(&db, source.path()).await;
    assert_eq!(counts(&db).await, [1, 0, 0, 0]);
}

#[tokio::test]
async fn broken_line_rolls_back_everything_and_names_the_line() {
    let (_db_dir, db) = open().await;
    let source = fixture();
    append(source.path(), "participants", "{\"id\":16,\"giveaway_id\":");
    let err = import_err(&db, source.path()).await;
    assert!(err.contains("participants.jsonl:5"), "{err}");
    assert_eq!(counts(&db).await, [0, 0, 0, 0]);
}

#[tokio::test]
async fn errors_do_not_leak_row_content() {
    let (_db_dir, db) = open().await;
    let source = fixture();
    append(
        source.path(),
        "users",
        r#"{"user_id":"secret_name","username":"secret_name","joined_at":"2025-11-02T10:00:00+00:00","is_suspicious":false}"#,
    );
    let err = import_err(&db, source.path()).await;
    assert!(err.contains("users.jsonl:6"), "{err}");
    assert!(!err.contains("secret_name"), "{err}");
    assert_eq!(counts(&db).await, [0, 0, 0, 0]);
}

#[tokio::test]
async fn invalid_timestamp_is_rejected() {
    let (_db_dir, db) = open().await;
    let source = fixture();
    append(
        source.path(),
        "winners",
        r#"{"id":6,"giveaway_id":7,"user_id":1001,"username_snapshot":"alice","giveaway_end_snapshot":"08.12.2025 12:00","created_at":"2025-12-08T12:00:01+00:00"}"#,
    );
    let err = import_err(&db, source.path()).await;
    assert!(err.contains("winners.jsonl:3"), "{err}");
    assert!(!err.contains("08.12.2025"), "{err}");
    assert_eq!(counts(&db).await, [0, 0, 0, 0]);
}

#[tokio::test]
async fn constraint_violation_rolls_back() {
    let (_db_dir, db) = open().await;
    let source = fixture();
    append(
        source.path(),
        "participants",
        r#"{"id":16,"giveaway_id":3,"user_id":1001,"joined_at":"2025-11-10T09:05:00+00:00","username_snapshot":"alice","giveaway_end_snapshot":"2025-11-17T09:00:00+00:00"}"#,
    );
    let err = import_err(&db, source.path()).await;
    assert!(err.contains("participants.jsonl:5"), "{err}");
    assert_eq!(counts(&db).await, [0, 0, 0, 0]);
}

#[tokio::test]
async fn unexpected_column_is_rejected() {
    let (_db_dir, db) = open().await;
    let source = fixture();
    append(
        source.path(),
        "giveaways",
        r#"{"id":8,"start_at":"2026-01-01T00:00:00+00:00","end_at":"2026-01-02T00:00:00+00:00","description":"x","num_winners":1,"is_active":false,"announce_text":null,"announce_media_file_id":"F","announce_media_type":"photo","created_by_admin_id":42,"created_at":"2026-01-01T00:00:00+00:00","ended_at":null,"prize":"car"}"#,
    );
    let err = import_err(&db, source.path()).await;
    assert!(err.contains("giveaways.jsonl:3"), "{err}");
    assert!(err.contains("unknown field `prize`"), "{err}");
    assert_eq!(counts(&db).await, [0, 0, 0, 0]);
}

#[tokio::test]
async fn missing_file_is_rejected_before_any_insert() {
    let (_db_dir, db) = open().await;
    let source = fixture();
    fs::remove_file(source.path().join("winners.jsonl")).unwrap();
    let err = import_err(&db, source.path()).await;
    assert!(err.contains("winners.jsonl"), "{err}");
    assert_eq!(counts(&db).await, [0, 0, 0, 0]);
}
