mod common;

use common::*;
use serde_json::json;
use tg_bot_giveaway_and_broadcast::{
    db::{Audience, Content, Database, MailingKind, NewMailing},
    mailing::{run_pending, send_content},
    state::Media,
};
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{body_partial_json, method, path_regex},
};

fn text(text: &str) -> Content {
    Content {
        text: Some(text.into()),
        media: None,
        join_button: false,
    }
}

fn new(kind: MailingKind, content: Content, to_channel: bool, audience: Audience) -> NewMailing {
    NewMailing {
        kind,
        content,
        to_channel,
        audience,
        rps: 1000,
        report_chat: ADMIN,
        report_message: 77,
    }
}

async fn users(db: &Database, ids: impl IntoIterator<Item = i64>) {
    for id in ids {
        db.upsert_user(id, Some(&format!("user{id}")))
            .await
            .unwrap();
    }
}

async fn report(server: &wiremock::MockServer) -> String {
    let edits: Vec<_> = calls(server)
        .await
        .into_iter()
        .filter(|(m, _)| m == "EditMessageText" || m == "editMessageText")
        .collect();
    assert_eq!(edits.len(), 1, "exactly one report edit");
    let body: serde_json::Value = serde_json::from_str(&edits[0].1).unwrap();
    assert_eq!(body["chat_id"], ADMIN);
    assert_eq!(body["message_id"], 77);
    body["text"].as_str().unwrap().to_owned()
}

fn method_is(name: &'static str) -> wiremock::matchers::PathRegexMatcher {
    path_regex(format!("(?i)/{name}$"))
}

#[tokio::test]
async fn broadcast_reaches_every_user_and_reports() {
    let (dir, db) = db().await;
    let server = server().await;
    users(&db, [3, 1, 2]).await;
    let id = db
        .enqueue_mailing(&new(
            MailingKind::Broadcast,
            text("<b>hi</b>"),
            false,
            Audience::Users,
        ))
        .await
        .unwrap();
    assert!(
        run_pending(&bot(&server), &db, &config(dir.path()))
            .await
            .unwrap()
    );
    assert_eq!(chats(&server, "SendMessage").await, [1, 2, 3]);
    let mailing = db.mailing(id).await.unwrap().unwrap();
    assert_eq!((mailing.sent, mailing.failed, mailing.total), (3, 0, 3));
    let text = report(&server).await;
    assert!(text.starts_with(
        "✅ <b>Рассылка завершена!</b>\n\n📊 Всего пользователей: 3\n✉️ Отправлено: 3\n❌ Не доставлено: 0\n⏱️ Длительность: "
    ), "{text}");
    assert!(text.ends_with('с'));
    assert!(
        !run_pending(&bot(&server), &db, &config(dir.path()))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn blocked_users_count_as_failed() {
    let (dir, db) = db().await;
    let server = wiremock::MockServer::start().await;
    Mock::given(body_partial_json(json!({"chat_id": 2})))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(api_error(403, "Forbidden: bot was blocked by the user")),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_message(1)))
        .mount(&server)
        .await;
    users(&db, 1..=3).await;
    let id = db
        .enqueue_mailing(&new(
            MailingKind::Broadcast,
            text("x"),
            false,
            Audience::Users,
        ))
        .await
        .unwrap();
    run_pending(&bot(&server), &db, &config(dir.path()))
        .await
        .unwrap();
    let mailing = db.mailing(id).await.unwrap().unwrap();
    assert_eq!((mailing.sent, mailing.failed), (2, 1));
}

#[tokio::test]
async fn retry_after_is_honoured_then_delivered() {
    let (dir, db) = db().await;
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_json(retry_after(1)))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_message(1)))
        .mount(&server)
        .await;
    users(&db, [1]).await;
    let id = db
        .enqueue_mailing(&new(
            MailingKind::Broadcast,
            text("x"),
            false,
            Audience::Users,
        ))
        .await
        .unwrap();
    let started = std::time::Instant::now();
    run_pending(&bot(&server), &db, &config(dir.path()))
        .await
        .unwrap();
    assert!(started.elapsed() >= std::time::Duration::from_secs(1));
    assert_eq!(chats(&server, "SendMessage").await, [1, 1]);
    let mailing = db.mailing(id).await.unwrap().unwrap();
    assert_eq!((mailing.sent, mailing.failed), (1, 0));
}

#[tokio::test]
async fn retries_stop_after_max_retries() {
    let (dir, db) = db().await;
    let server = wiremock::MockServer::start().await;
    Mock::given(method_is("SendMessage"))
        .respond_with(ResponseTemplate::new(429).set_body_json(retry_after(1)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_message(1)))
        .mount(&server)
        .await;
    users(&db, [1]).await;
    let id = db
        .enqueue_mailing(&new(
            MailingKind::Broadcast,
            text("x"),
            false,
            Audience::Users,
        ))
        .await
        .unwrap();
    run_pending(&bot(&server), &db, &config(dir.path()))
        .await
        .unwrap();
    // MAX_RETRIES=2 in the test config: one attempt plus two retries.
    assert_eq!(chats(&server, "SendMessage").await.len(), 3);
    let mailing = db.mailing(id).await.unwrap().unwrap();
    assert_eq!((mailing.sent, mailing.failed), (0, 1));
}

#[tokio::test]
async fn media_is_sent_with_the_matching_method() {
    let server = server().await;
    for (kind, method) in [
        ("photo", "SendPhoto"),
        ("video", "SendVideo"),
        ("animation", "SendAnimation"),
        ("document", "SendDocument"),
    ] {
        let content = Content {
            text: Some("caption".into()),
            media: Some(Media {
                kind: kind.into(),
                file_id: format!("{kind}-id"),
            }),
            join_button: true,
        };
        send_content(&bot(&server), 5, &content, JOIN_URL)
            .await
            .unwrap();
        let (last_method, body) = calls(&server).await.pop().unwrap();
        assert!(last_method.eq_ignore_ascii_case(method), "{last_method}");
        for expected in [
            &format!("{kind}-id"),
            "caption",
            JOIN_URL,
            "🎁 Участвовать",
            "HTML",
        ] {
            assert!(
                body.contains(expected),
                "{kind}: {expected} missing in {body}"
            );
        }
    }
    let unknown = Content {
        text: None,
        media: Some(Media {
            kind: "sticker".into(),
            file_id: "x".into(),
        }),
        join_button: false,
    };
    assert!(
        send_content(&bot(&server), 5, &unknown, JOIN_URL)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn announce_goes_to_channel_first_and_counts_it_once() {
    let (dir, db) = db().await;
    let server = server().await;
    users(&db, [1, 2]).await;
    let content = Content {
        text: Some("🎉 <b>Новый розыгрыш!</b>".into()),
        media: Some(Media {
            kind: "photo".into(),
            file_id: "P".into(),
        }),
        join_button: true,
    };
    db.enqueue_mailing(&new(
        MailingKind::AnnounceNew,
        content,
        true,
        Audience::Users,
    ))
    .await
    .unwrap();
    run_pending(&bot(&server), &db, &config(dir.path()))
        .await
        .unwrap();
    assert_eq!(chats(&server, "SendPhoto").await, [CHANNEL, 1, 2]);
    assert_eq!(
        report(&server).await,
        "✅ Анонс отправлен!\n\n📊 Отправлено: 3\n🎁 Розыгрыш активен!"
    );
}

#[tokio::test]
async fn channel_only_announce_reports_one() {
    let (dir, db) = db().await;
    let server = server().await;
    users(&db, [1]).await;
    db.enqueue_mailing(&new(
        MailingKind::Announce,
        text("a"),
        true,
        Audience::Nobody,
    ))
    .await
    .unwrap();
    run_pending(&bot(&server), &db, &config(dir.path()))
        .await
        .unwrap();
    assert_eq!(chats(&server, "SendMessage").await, [CHANNEL]);
    assert_eq!(
        report(&server).await,
        "✅ Анонс отправлен!\n\n📊 Отправлено: 1"
    );
}

#[tokio::test]
async fn resumes_after_a_crash_without_duplicates() {
    let (dir, db) = db().await;
    let server = server().await;
    users(&db, 1..=5).await;
    let id = db
        .enqueue_mailing(&new(
            MailingKind::Broadcast,
            text("x"),
            false,
            Audience::Users,
        ))
        .await
        .unwrap();
    // The previous process delivered to user 1 and died while sending to user 2.
    let running = db.next_mailing().await.unwrap().unwrap();
    assert_eq!(running.id, id);
    db.begin_send(id, 1).await.unwrap();
    db.end_send(id, true).await.unwrap();
    db.begin_send(id, 2).await.unwrap();

    run_pending(&bot(&server), &db, &config(dir.path()))
        .await
        .unwrap();
    assert_eq!(chats(&server, "SendMessage").await, [3, 4, 5]);
    let mailing = db.mailing(id).await.unwrap().unwrap();
    assert_eq!((mailing.sent, mailing.failed, mailing.total), (4, 1, 5));
}

#[tokio::test]
async fn recipients_are_a_snapshot_taken_at_enqueue() {
    let (dir, db) = db().await;
    let server = server().await;
    users(&db, [1, 2]).await;
    db.enqueue_mailing(&new(
        MailingKind::Broadcast,
        text("x"),
        false,
        Audience::Users,
    ))
    .await
    .unwrap();
    users(&db, [0, 10]).await;
    run_pending(&bot(&server), &db, &config(dir.path()))
        .await
        .unwrap();
    assert_eq!(chats(&server, "SendMessage").await, [1, 2]);
}

#[tokio::test]
async fn results_to_admins_use_the_stored_admin_list() {
    let (dir, db) = db().await;
    let server = server().await;
    users(&db, [1, 2]).await;
    db.enqueue_mailing(&new(
        MailingKind::Results,
        text("🏆"),
        false,
        Audience::Admins(vec![99, 98]),
    ))
    .await
    .unwrap();
    run_pending(&bot(&server), &db, &config(dir.path()))
        .await
        .unwrap();
    assert_eq!(chats(&server, "SendMessage").await, [98, 99]);
    assert_eq!(
        report(&server).await,
        "✅ Результаты опубликованы!\n\n📊 Отправлено: 2"
    );
}

#[tokio::test]
async fn mailings_run_one_at_a_time_in_order() {
    let (dir, db) = db().await;
    let server = server().await;
    users(&db, [1]).await;
    let first = db
        .enqueue_mailing(&new(
            MailingKind::Broadcast,
            text("first"),
            false,
            Audience::Users,
        ))
        .await
        .unwrap();
    let second = db
        .enqueue_mailing(&new(
            MailingKind::Broadcast,
            text("second"),
            false,
            Audience::Users,
        ))
        .await
        .unwrap();
    let config = config(dir.path());
    run_pending(&bot(&server), &db, &config).await.unwrap();
    assert!(db.mailing(first).await.unwrap().unwrap().finished);
    assert!(!db.mailing(second).await.unwrap().unwrap().finished);
    run_pending(&bot(&server), &db, &config).await.unwrap();
    assert!(db.mailing(second).await.unwrap().unwrap().finished);
}
