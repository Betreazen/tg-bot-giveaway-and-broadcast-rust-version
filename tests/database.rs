use chrono::{Duration, TimeZone, Utc};
use tg_bot_giveaway_and_broadcast::{
    db::{Database, NewGiveaway},
    state::{BcDraft, BcStep, Dialogue, GwDraft, GwStep, Media, WinStep},
    time::Time,
};

pub async fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path()).await.unwrap();
    (dir, db)
}

fn t0() -> Time {
    Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap()
}

fn new_giveaway(description: &str) -> NewGiveaway {
    NewGiveaway {
        start_at: t0(),
        end_at: t0() + Duration::days(1),
        description: description.into(),
        num_winners: 2,
        media: Media {
            kind: "photo".into(),
            file_id: "FILE".into(),
        },
        created_by: 99,
    }
}

#[tokio::test]
async fn reopening_keeps_data_and_schema() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path()).await.unwrap();
    db.upsert_user(1, Some("alice")).await.unwrap();
    db.close().await;
    let db = Database::open(dir.path()).await.unwrap();
    assert_eq!(db.user_count().await.unwrap(), 1);
    db.check().await.unwrap();
}

#[tokio::test]
async fn upsert_keeps_username_when_new_one_is_missing() {
    let (_dir, db) = open().await;
    db.upsert_user(1, Some("alice")).await.unwrap();
    db.upsert_user(1, None).await.unwrap();
    assert_eq!(
        db.set_suspicious("alice", true).await.unwrap(),
        Some(1),
        "username survived the NULL update"
    );
    db.upsert_user(1, Some("alice2")).await.unwrap();
    assert_eq!(db.set_suspicious("alice", true).await.unwrap(), None);
    assert_eq!(db.user_count().await.unwrap(), 1);
}

#[tokio::test]
async fn suspicious_lookup_is_case_insensitive_and_listed_in_python_order() {
    let (_dir, db) = open().await;
    db.upsert_user(3, Some("Zed_user")).await.unwrap();
    db.upsert_user(2, None).await.unwrap();
    db.upsert_user(1, Some("Alice")).await.unwrap();
    assert_eq!(db.set_suspicious("zed_user", true).await.unwrap(), Some(3));
    assert_eq!(db.set_suspicious("alice", true).await.unwrap(), Some(1));
    assert_eq!(db.set_suspicious("nobody", true).await.unwrap(), None);
    sqlx_mark_null_username_suspicious(&db, 2).await;
    assert_eq!(
        db.suspicious_users().await.unwrap(),
        [
            (1, Some("Alice".into())),
            (3, Some("Zed_user".into())),
            (2, None)
        ]
    );
    assert_eq!(db.set_suspicious("ALICE", false).await.unwrap(), Some(1));
    assert_eq!(db.suspicious_users().await.unwrap().len(), 2);
}

// A user without username can only become suspicious through imported data.
async fn sqlx_mark_null_username_suspicious(db: &Database, user_id: i64) {
    exec(
        &db,
        "UPDATE users SET is_suspicious=1 WHERE user_id=?",
        user_id,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn creating_a_giveaway_deactivates_the_previous_one() {
    let (_dir, db) = open().await;
    assert!(db.active_giveaway().await.unwrap().is_none());
    let first = db.create_giveaway(&new_giveaway("first")).await.unwrap();
    let second = db.create_giveaway(&new_giveaway("second")).await.unwrap();
    let active = db.active_giveaway().await.unwrap().unwrap();
    assert_eq!(active.id, second);
    assert_eq!(active.description, "second");
    assert_eq!(active.num_winners, 2);
    assert_eq!(active.media.kind, "photo");
    assert!(!db.giveaway(first).await.unwrap().unwrap().is_active);
}

#[tokio::test]
async fn database_refuses_two_active_giveaways() {
    let (_dir, db) = open().await;
    let first = db.create_giveaway(&new_giveaway("first")).await.unwrap();
    db.create_giveaway(&new_giveaway("second")).await.unwrap();
    let reactivate = exec(&db, "UPDATE giveaways SET is_active=1 WHERE id=?", first).await;
    assert!(
        reactivate.is_err(),
        "unique index must reject a second active row"
    );
}

#[tokio::test]
async fn ending_sets_ended_at_and_deactivates() {
    let (_dir, db) = open().await;
    let id = db.create_giveaway(&new_giveaway("g")).await.unwrap();
    let ended = db.end_giveaway(id).await.unwrap().unwrap();
    assert!(!ended.is_active);
    assert!(ended.ended_at.is_some());
    assert!(db.active_giveaway().await.unwrap().is_none());
    assert!(db.end_giveaway(12345).await.unwrap().is_none());
}

#[tokio::test]
async fn participation_is_idempotent_and_counted() {
    let (_dir, db) = open().await;
    let id = db.create_giveaway(&new_giveaway("g")).await.unwrap();
    assert!(!db.is_participant(id, 5).await.unwrap());
    for _ in 0..2 {
        db.add_participant(id, 5, Some("bob"), t0()).await.unwrap();
    }
    db.add_participant(id, 6, None, t0()).await.unwrap();
    assert!(db.is_participant(id, 5).await.unwrap());
    assert_eq!(db.participant_count(id).await.unwrap(), 2);
}

#[tokio::test]
async fn verification_attempts_are_per_giveaway_and_persistent() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path()).await.unwrap();
    assert_eq!(db.attempts(1, 5).await.unwrap(), 0);
    assert_eq!(db.add_attempt(1, 5).await.unwrap(), 1);
    assert_eq!(db.add_attempt(1, 5).await.unwrap(), 2);
    db.close().await;
    let db = Database::open(dir.path()).await.unwrap();
    assert_eq!(db.attempts(1, 5).await.unwrap(), 2);
    assert_eq!(db.attempts(2, 5).await.unwrap(), 0);
    db.block(2, 5).await.unwrap();
    assert_eq!(db.attempts(2, 5).await.unwrap(), 3);
}

#[tokio::test]
async fn dialogues_round_trip_and_clear() {
    let (_dir, db) = open().await;
    let states = [
        Dialogue::Verify {
            correct: 3,
            numbers: vec![1, 2, 3, 4, 5],
            created_at: 100,
            giveaway_id: 1,
            username: "bob".into(),
            end_at: "2026-01-01T00:00:00.000000Z".into(),
            description: "desc".into(),
            num_winners: 2,
        },
        Dialogue::Giveaway {
            step: GwStep::Preview,
            draft: GwDraft {
                start_option: Some("now".into()),
                media: Some(Media {
                    kind: "video".into(),
                    file_id: "F".into(),
                }),
                ..GwDraft::default()
            },
        },
        Dialogue::Broadcast {
            step: BcStep::Confirm,
            draft: BcDraft {
                media_mode: true,
                text: Some("hi".into()),
                media: None,
            },
        },
        Dialogue::Winners {
            step: WinStep::Publish,
            giveaway_id: 4,
        },
        Dialogue::Suspicious { mark: false },
    ];
    for state in states {
        db.save_dialogue(7, &state).await.unwrap();
        assert_eq!(db.load_dialogue(7).await.unwrap(), Some(state));
    }
    db.clear_dialogue(7).await.unwrap();
    assert_eq!(db.load_dialogue(7).await.unwrap(), None);
}

async fn exec(db: &Database, sql: &str, value: i64) -> Result<(), sqlx::Error> {
    sqlx::query(sql)
        .bind(value)
        .execute(db.pool())
        .await
        .map(|_| ())
}
