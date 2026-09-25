//! Participation and verification: PARITY.md U1–U17.
mod common;

use common::*;
use tg_bot_giveaway_and_broadcast::{
    handlers::{App, handle_callback, handle_message},
    state::Dialogue,
    text::t,
};

async fn start(bot: &teloxide::Bot, app: &std::sync::Arc<App>, user: i64, name: Option<&str>) {
    handle_message(
        bot.clone(),
        text_from(user, name, "/start join"),
        app.clone(),
    )
    .await
    .unwrap();
}

async fn press(bot: &teloxide::Bot, app: &std::sync::Arc<App>, user: i64, data: &str) {
    handle_callback(bot.clone(), callback(user, data), app.clone())
        .await
        .unwrap();
}

async fn verification(app: &App, user: i64) -> (u8, Vec<u8>) {
    match app.db.load_dialogue(user).await.unwrap() {
        Some(Dialogue::Verify(v)) => (v.correct, v.numbers),
        other => panic!("expected verification, got {other:?}"),
    }
}

#[tokio::test]
async fn u1_start_registers_the_user_and_keeps_known_username() {
    let server = server().await;
    membership(&server, "left").await;
    let (_dir, app, bot) = app(&server).await;
    start(&bot, &app, 5, Some("Bob_Name")).await;
    start(&bot, &app, 5, None).await;
    assert_eq!(app.db.user_count().await.unwrap(), 1);
    assert_eq!(
        app.db.set_suspicious("bob_name", false).await.unwrap(),
        Some(5)
    );
}

#[tokio::test]
async fn u2_not_subscribed_or_api_error_means_not_subscribed() {
    for status in ["left", "kicked", "restricted"] {
        let server = server().await;
        membership(&server, status).await;
        let (_dir, app, bot) = app(&server).await;
        active_giveaway(&app).await;
        start(&bot, &app, 5, Some("bob")).await;
        assert_eq!(
            texts(&server).await,
            [t("user.not_subscribed", &[])],
            "{status}"
        );
    }
    let server = server().await; // getChatMember answered by the generic mock: invalid result
    let (_dir, app, bot) = app(&server).await;
    start(&bot, &app, 5, Some("bob")).await;
    assert_eq!(texts(&server).await, [t("user.not_subscribed", &[])]);
}

#[tokio::test]
async fn u3_no_active_giveaway() {
    let server = server().await;
    membership(&server, "member").await;
    let (_dir, app, bot) = app(&server).await;
    start(&bot, &app, 5, Some("bob")).await;
    assert_eq!(texts(&server).await, [t("user.no_active_giveaway", &[])]);
}

#[tokio::test]
async fn u4_already_participating() {
    let server = server().await;
    membership(&server, "creator").await;
    let (_dir, app, bot) = app(&server).await;
    let id = active_giveaway(&app).await;
    app.db
        .add_participant(
            id,
            5,
            Some("bob"),
            tg_bot_giveaway_and_broadcast::time::now(),
        )
        .await
        .unwrap();
    start(&bot, &app, 5, Some("bob")).await;
    assert_eq!(texts(&server).await, [t("user.already_participating", &[])]);
}

#[tokio::test]
async fn u5_admin_joins_without_verification_or_username() {
    let server = server().await;
    membership(&server, "administrator").await;
    let (_dir, app, bot) = app(&server).await;
    let id = active_giveaway(&app).await;
    start(&bot, &app, ADMIN, None).await;
    assert!(app.db.is_participant(id, ADMIN).await.unwrap());
    assert_eq!(
        texts(&server).await,
        [t(
            "user.participation_confirmed",
            &[("description", &"Приз <b>iPhone</b>"), ("num_winners", &2)]
        )]
    );
}

#[tokio::test]
async fn u6_username_is_required() {
    let server = server().await;
    membership(&server, "member").await;
    let (_dir, app, bot) = app(&server).await;
    let id = active_giveaway(&app).await;
    start(&bot, &app, 5, None).await;
    assert_eq!(texts(&server).await, [t("user.no_username", &[])]);
    assert!(!app.db.is_participant(id, 5).await.unwrap());
}

#[tokio::test]
async fn u7_u9_blocked_user_is_told_so() {
    let server = server().await;
    membership(&server, "member").await;
    let (_dir, app, bot) = app(&server).await;
    let id = active_giveaway(&app).await;
    app.db.block(id, 5).await.unwrap();
    start(&bot, &app, 5, Some("bob")).await;
    assert_eq!(texts(&server).await, [t("user.verification_blocked", &[])]);
}

#[tokio::test]
async fn u8_u10_verification_prompt_then_in_progress_then_restart_after_timeout() {
    let server = server().await;
    membership(&server, "member").await;
    let (_dir, app, bot) = app(&server).await;
    active_giveaway(&app).await;
    start(&bot, &app, 5, Some("bob")).await;
    let (correct, numbers) = verification(&app, 5).await;
    let (_, body) = last(&server).await;
    assert_eq!(
        body["text"],
        t("user.verification_prompt", &[("number", &correct)])
    );
    let rows = buttons(&body);
    assert_eq!((rows.len(), rows[0].len(), rows[1].len()), (2, 3, 2));
    let data: Vec<String> = numbers.iter().map(|n| format!("verify:{n}")).collect();
    assert_eq!(rows.concat(), data);
    assert_eq!(numbers.len(), 5);
    assert!(numbers.contains(&correct) && numbers.iter().all(|n| *n <= 9));

    start(&bot, &app, 5, Some("bob")).await;
    assert_eq!(
        texts(&server).await.last().unwrap(),
        &t("user.verification_in_progress", &[])
    );

    expire(&app, 5).await;
    start(&bot, &app, 5, Some("bob")).await;
    let (_, body) = last(&server).await;
    assert!(body["text"].as_str().unwrap().starts_with("🔐"));
}

async fn expire(app: &App, user: i64) {
    let Some(Dialogue::Verify(mut state)) = app.db.load_dialogue(user).await.unwrap() else {
        panic!("no verification")
    };
    state.created_at = chrono::Utc::now().timestamp() - 181;
    app.db
        .save_dialogue(user, &Dialogue::Verify(state))
        .await
        .unwrap();
}

#[tokio::test]
async fn u11_correct_button_registers_participant() {
    let server = server().await;
    membership(&server, "member").await;
    let (_dir, app, bot) = app(&server).await;
    let id = active_giveaway(&app).await;
    start(&bot, &app, 5, Some("bob")).await;
    let (correct, _) = verification(&app, 5).await;
    press(&bot, &app, 5, &format!("verify:{correct}")).await;
    assert!(app.db.is_participant(id, 5).await.unwrap());
    assert!(app.db.load_dialogue(5).await.unwrap().is_none());
    let edit = sent(&server)
        .await
        .into_iter()
        .find(|(m, _)| m == "editmessagetext")
        .unwrap()
        .1;
    assert_eq!(
        edit["text"],
        t(
            "user.participation_confirmed",
            &[("description", &"Приз <b>iPhone</b>"), ("num_winners", &2)]
        )
    );
    assert_eq!(edit["message_id"], 55);
}

#[tokio::test]
async fn u12_u13_wrong_answers_reshuffle_then_block_across_restarts() {
    let server = server().await;
    membership(&server, "member").await;
    let dir = tempfile::tempdir().unwrap();
    let bot = bot(&server);
    let app = App::open(config(dir.path())).await.unwrap();
    let id = active_giveaway(&app).await;
    start(&bot, &app, 5, Some("bob")).await;
    let (correct, numbers) = verification(&app, 5).await;
    let wrong = *numbers.iter().find(|n| **n != correct).unwrap();

    press(&bot, &app, 5, &format!("verify:{wrong}")).await;
    let body = last_of(&server, "editMessageText").await;
    assert_eq!(
        body["text"],
        t(
            "user.verification_wrong",
            &[("remaining", &2), ("number", &correct)]
        )
    );
    let mut shown: Vec<String> = buttons(&body).concat();
    shown.sort();
    let mut expected: Vec<String> = numbers.iter().map(|n| format!("verify:{n}")).collect();
    expected.sort();
    assert_eq!(shown, expected, "same digits, new order");

    // Attempts survive a new verification session and a restart.
    app.db.close().await;
    let app = App::open(config(dir.path())).await.unwrap();
    assert_eq!(app.db.attempts(id, 5).await.unwrap(), 1);
    press(&bot, &app, 5, &format!("verify:{wrong}")).await;
    press(&bot, &app, 5, &format!("verify:{wrong}")).await;
    assert_eq!(
        last_of(&server, "editMessageText").await["text"],
        t("user.verification_blocked", &[])
    );
    assert!(app.db.load_dialogue(5).await.unwrap().is_none());
    assert!(!app.db.is_participant(id, 5).await.unwrap());
    start(&bot, &app, 5, Some("bob")).await;
    assert_eq!(
        texts(&server).await.last().unwrap(),
        &t("user.verification_blocked", &[])
    );
}

#[tokio::test]
async fn u14_late_answer_times_out() {
    let server = server().await;
    membership(&server, "member").await;
    let (_dir, app, bot) = app(&server).await;
    let id = active_giveaway(&app).await;
    start(&bot, &app, 5, Some("bob")).await;
    let (correct, _) = verification(&app, 5).await;
    expire(&app, 5).await;
    press(&bot, &app, 5, &format!("verify:{correct}")).await;
    let (method, body) = last(&server).await;
    assert_eq!(method, "answercallbackquery");
    assert_eq!(body["text"], t("user.verification_timeout", &[]));
    assert_eq!(body["show_alert"], true);
    assert!(!app.db.is_participant(id, 5).await.unwrap());
    assert!(app.db.load_dialogue(5).await.unwrap().is_none());
}

#[tokio::test]
async fn u15_stray_verification_button_is_just_acknowledged() {
    let server = server().await;
    let (_dir, app, bot) = app(&server).await;
    press(&bot, &app, 5, "verify:3").await;
    press(&bot, &app, 5, "verify:x").await;
    let calls = sent(&server).await;
    assert_eq!(calls.len(), 2);
    assert!(
        calls
            .iter()
            .all(|(m, b)| m == "answercallbackquery" && b.get("text").is_none())
    );
}

#[tokio::test]
async fn u16_internal_error_gives_generic_reply() {
    let server = server().await;
    let (_dir, app, bot) = app(&server).await;
    app.db.close().await;
    start(&bot, &app, 5, Some("bob")).await;
    assert_eq!(texts(&server).await, [t("errors.generic", &[])]);
}

#[tokio::test]
async fn u17_other_messages_and_group_chats_are_ignored() {
    let server = server().await;
    let (_dir, app, bot) = app(&server).await;
    handle_message(bot.clone(), text(5, "hello"), app.clone())
        .await
        .unwrap();
    handle_message(bot.clone(), group_text(5, "/start"), app.clone())
        .await
        .unwrap();
    assert!(sent(&server).await.is_empty());
    assert_eq!(app.db.user_count().await.unwrap(), 0);
}
