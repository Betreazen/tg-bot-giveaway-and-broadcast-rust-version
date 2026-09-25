//! Port of the Python bot's tests/test_giveaway_service.py.
use chrono::{Duration, TimeZone, Utc};
use rand::{SeedableRng, rngs::StdRng};
use std::collections::HashSet;
use tg_bot_giveaway_and_broadcast::{
    db::{Database, Draw, NewGiveaway, Winner, format_winner_list},
    state::Media,
    time::Time,
};

fn t0() -> Time {
    Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap()
}

async fn setup(num_winners: i64, participants: i64) -> (tempfile::TempDir, Database, i64) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path()).await.unwrap();
    let id = db
        .create_giveaway(&NewGiveaway {
            start_at: t0(),
            end_at: t0() + Duration::days(1),
            description: "d".into(),
            num_winners,
            media: Media {
                kind: "photo".into(),
                file_id: "F".into(),
            },
            created_by: 1,
        })
        .await
        .unwrap();
    for user in 1..=participants {
        let name = format!("user{user}");
        db.upsert_user(user, Some(&name)).await.unwrap();
        db.add_participant(id, user, Some(&name), t0() + Duration::days(1))
            .await
            .unwrap();
    }
    (dir, db, id)
}

async fn draw(db: &Database, id: i64, seed: u64) -> Draw {
    let giveaway = db.giveaway(id).await.unwrap().unwrap();
    db.draw_winners(&giveaway, &mut StdRng::seed_from_u64(seed))
        .await
        .unwrap()
}

fn ids(draw: &Draw) -> Vec<i64> {
    match draw {
        Draw::Winners(w) => w.iter().map(|w| w.user_id).collect(),
        Draw::NoParticipants => panic!("expected winners"),
    }
}

#[tokio::test]
async fn picks_requested_count_without_duplicates() {
    let (_dir, db, id) = setup(3, 10).await;
    let winners = ids(&draw(&db, id, 1).await);
    assert_eq!(winners.len(), 3);
    assert_eq!(winners.iter().collect::<HashSet<_>>().len(), 3);
}

#[tokio::test]
async fn caps_at_participant_count() {
    let (_dir, db, id) = setup(5, 2).await;
    assert_eq!(ids(&draw(&db, id, 1).await).len(), 2);
}

#[tokio::test]
async fn snapshot_uses_ended_at_when_present() {
    let (_dir, db, id) = setup(1, 3).await;
    let ended = db.end_giveaway(id).await.unwrap().unwrap();
    let Draw::Winners(winners) = draw(&db, id, 1).await else {
        panic!("expected winners")
    };
    assert_eq!(winners[0].giveaway_end_snapshot, ended.ended_at.unwrap());
    assert_eq!(
        winners[0].username_snapshot.as_deref().map(|s| &s[..4]),
        Some("user")
    );

    let (_dir2, db2, id2) = setup(1, 3).await;
    let Draw::Winners(winners) = draw(&db2, id2, 1).await else {
        panic!("expected winners")
    };
    assert_eq!(winners[0].giveaway_end_snapshot, t0() + Duration::days(1));
}

#[tokio::test]
async fn no_participants_is_reported() {
    let (_dir, db, id) = setup(1, 0).await;
    assert!(matches!(draw(&db, id, 1).await, Draw::NoParticipants));
}

#[tokio::test]
async fn suspicious_participants_never_win() {
    let (_dir, db, id) = setup(3, 10).await;
    for user in [1, 2, 3, 4] {
        db.set_suspicious(&format!("user{user}"), true)
            .await
            .unwrap();
    }
    for seed in 0..200 {
        exec(&db, "DELETE FROM winners WHERE giveaway_id=?", id)
            .await
            .unwrap();
        let winners = ids(&draw(&db, id, seed).await);
        assert_eq!(winners.len(), 3);
        assert!(winners.iter().all(|w| *w > 4), "seed {seed}: {winners:?}");
    }
}

#[tokio::test]
async fn all_suspicious_means_no_participants() {
    let (_dir, db, id) = setup(1, 2).await;
    for user in [1, 2] {
        db.set_suspicious(&format!("user{user}"), true)
            .await
            .unwrap();
    }
    assert!(matches!(draw(&db, id, 1).await, Draw::NoParticipants));
}

#[tokio::test]
async fn every_participant_can_win() {
    let (_dir, db, id) = setup(1, 5).await;
    let mut seen = HashSet::new();
    for seed in 0..200 {
        exec(&db, "DELETE FROM winners WHERE giveaway_id=?", id)
            .await
            .unwrap();
        seen.extend(ids(&draw(&db, id, seed).await));
    }
    assert_eq!(seen.len(), 5);
}

#[tokio::test]
async fn second_draw_returns_the_first_result() {
    let (_dir, db, id) = setup(2, 10).await;
    let first = ids(&draw(&db, id, 1).await);
    let second = ids(&draw(&db, id, 999).await);
    assert_eq!(first, second);
    assert_eq!(db.winners(id).await.unwrap().len(), 2);
}

#[test]
fn winner_list_format_matches_python() {
    let winner = |user_id, name: Option<&str>| Winner {
        user_id,
        username_snapshot: name.map(Into::into),
        giveaway_end_snapshot: t0(),
    };
    let out = format_winner_list(&[winner(1, Some("alice")), winner(2, None)]);
    assert_eq!(out, "1. @alice\n2. ID: 2");
    assert_eq!(format_winner_list(&[]), "No winners");
}

async fn exec(db: &Database, sql: &str, value: i64) -> Result<(), sqlx::Error> {
    sqlx::query(sql)
        .bind(value)
        .execute(db.pool())
        .await
        .map(|_| ())
}

#[test]
fn empty_username_is_shown_as_id_like_python() {
    let winner = Winner {
        user_id: 7,
        username_snapshot: Some(String::new()),
        giveaway_end_snapshot: t0(),
    };
    assert_eq!(format_winner_list(&[winner]), "1. ID: 7");
}
