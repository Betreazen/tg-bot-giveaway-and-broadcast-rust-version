//! Admin menu, giveaway wizard, announcements and winners: PARITY.md A1–A8, G1–G11, N1–N3, W1–W7.
mod common;

use common::*;
use serde_json::Value;
use std::sync::Arc;
use teloxide::Bot;
use tg_bot_giveaway_and_broadcast::{
    db::{Audience, MailingKind},
    handlers::{App, handle_callback, handle_message},
    state::{Dialogue, GwStep},
    text::t,
    time::now,
};
use wiremock::MockServer;

struct Admin {
    server: MockServer,
    app: Arc<App>,
    bot: Bot,
    _dir: tempfile::TempDir,
}

impl Admin {
    async fn new() -> Self {
        let server = server().await;
        let (dir, app, bot) = app(&server).await;
        Self {
            server,
            app,
            bot,
            _dir: dir,
        }
    }

    async fn press_as(&self, user: i64, data: &str) {
        handle_callback(self.bot.clone(), callback(user, data), self.app.clone())
            .await
            .unwrap();
    }

    async fn press(&self, data: &str) {
        self.press_as(ADMIN, data).await;
    }

    async fn say(&self, text: &str) {
        handle_message(
            self.bot.clone(),
            common::text(ADMIN, text),
            self.app.clone(),
        )
        .await
        .unwrap();
    }

    async fn send_media(&self, kind: &str) {
        handle_message(self.bot.clone(), media(ADMIN, kind, None), self.app.clone())
            .await
            .unwrap();
    }

    async fn last(&self) -> (String, Value) {
        last(&self.server).await
    }

    async fn last_text(&self) -> String {
        texts(&self.server).await.pop().unwrap()
    }

    async fn dialogue(&self) -> Option<Dialogue> {
        self.app.db.load_dialogue(ADMIN).await.unwrap()
    }

    async fn step(&self) -> GwStep {
        match self.dialogue().await {
            Some(Dialogue::Giveaway { step, .. }) => step,
            other => panic!("not in the giveaway wizard: {other:?}"),
        }
    }

    /// Walks the wizard up to the preview.
    async fn to_preview(&self) {
        self.press("admin:create_giveaway").await;
        self.press("start_time:now").await;
        self.press("duration:3").await;
        self.say("Розыгрыш <b>AirPods</b>").await;
        self.say("2").await;
        self.send_media("photo").await;
        assert_eq!(self.step().await, GwStep::Preview);
    }
}

fn menu_data(has_active: bool) -> Vec<Vec<String>> {
    let mut rows = vec!["admin:create_giveaway"];
    if has_active {
        rows.extend(["admin:announce_giveaway", "admin:complete_giveaway"]);
    }
    rows.extend([
        "admin:broadcast",
        "admin:status",
        "admin:suspicious",
        "admin:sync_sheets",
        "admin:close",
    ]);
    rows.into_iter().map(|d| vec![d.to_owned()]).collect()
}

#[tokio::test]
async fn a1_non_admins_are_refused_everywhere() {
    let admin = Admin::new().await;
    admin.press_as(5, "admin:status").await;
    let (method, body) = admin.last().await;
    assert_eq!(method, "answercallbackquery");
    assert_eq!(body["text"], t("admin.access_denied", &[]));
    assert_eq!(body["show_alert"], true);
    handle_message(admin.bot.clone(), text(5, "/admin"), admin.app.clone())
        .await
        .unwrap();
    assert_eq!(admin.last_text().await, t("admin.access_denied", &[]));
}

#[tokio::test]
async fn a2_admin_command_outside_private_chat() {
    let admin = Admin::new().await;
    for (user, key) in [
        (ADMIN, "admin.use_private_chat"),
        (5, "admin.access_denied"),
    ] {
        handle_message(
            admin.bot.clone(),
            group_text(user, "/admin"),
            admin.app.clone(),
        )
        .await
        .unwrap();
        assert_eq!(admin.last_text().await, t(key, &[]));
    }
}

#[tokio::test]
async fn a3_menu_shows_giveaway_buttons_only_when_active() {
    let admin = Admin::new().await;
    admin.say("/admin").await;
    let (_, body) = admin.last().await;
    assert_eq!(body["text"], t("admin.main_menu", &[]));
    assert_eq!(buttons(&body), menu_data(false));
    active_giveaway(&admin.app).await;
    admin.say("/admin").await;
    assert_eq!(buttons(&admin.last().await.1), menu_data(true));
}

#[tokio::test]
async fn a4_close_deletes_the_menu() {
    let admin = Admin::new().await;
    admin.press("admin:close").await;
    assert!(
        sent(&admin.server)
            .await
            .iter()
            .any(|(m, b)| m == "deletemessage" && b["message_id"] == 55)
    );
}

#[tokio::test]
async fn a5_status_alerts_or_reports() {
    let admin = Admin::new().await;
    admin.press("admin:status").await;
    let (_, body) = admin.last().await;
    assert_eq!(body["text"], t("admin.status_no_active", &[]));
    assert_eq!(body["show_alert"], true);

    let id = active_giveaway(&admin.app).await;
    admin
        .app
        .db
        .add_participant(id, 7, None, now())
        .await
        .unwrap();
    admin.press("admin:status").await;
    let giveaway = admin.app.db.giveaway(id).await.unwrap().unwrap();
    let expected = t(
        "admin.status_active",
        &[
            ("description", &giveaway.description),
            ("participants", &1),
            (
                "end_at",
                &tg_bot_giveaway_and_broadcast::time::fmt_msk(giveaway.end_at, "%Y-%m-%d %H:%M"),
            ),
            ("num_winners", &2),
        ],
    );
    assert_eq!(
        last_of(&admin.server, "sendMessage").await["text"],
        expected
    );
}

#[tokio::test]
async fn a6_a7_main_menu_and_cancel_reset_the_wizard() {
    let admin = Admin::new().await;
    admin.press("admin:create_giveaway").await;
    admin.press("nav:main_menu").await;
    assert!(admin.dialogue().await.is_none());
    let body = last_of(&admin.server, "editMessageText").await;
    assert_eq!(body["text"], t("admin.main_menu", &[]));
    assert_eq!(buttons(&body), menu_data(false));

    admin.press("admin:create_giveaway").await;
    admin.press("nav:cancel").await;
    assert!(admin.dialogue().await.is_none());
    assert_eq!(admin.last_text().await, t("admin.operation_cancelled", &[]));
}

#[tokio::test]
async fn g1_to_g8_full_wizard_creates_an_active_giveaway() {
    let admin = Admin::new().await;
    let old = active_giveaway(&admin.app).await;
    admin.press("admin:create_giveaway").await;
    let body = last_of(&admin.server, "editMessageText").await;
    assert_eq!(
        body["text"],
        "🗓 <b>Создание розыгрыша</b>\n\nКогда начать розыгрыш?"
    );
    assert_eq!(
        buttons(&body),
        [
            vec!["start_time:now", "start_time:1h"],
            vec!["start_time:3h", "start_time:6h"],
            vec!["start_time:tomorrow"],
            vec!["nav:back", "nav:cancel"],
        ]
    );
    admin.press("start_time:tomorrow").await;
    let body = last_of(&admin.server, "editMessageText").await;
    assert_eq!(
        body["text"],
        "📅 <b>Длительность розыгрыша</b>\n\nСколько будет длиться розыгрыш?"
    );
    assert_eq!(buttons(&body)[0], ["duration:1", "duration:3"]);
    admin.press("duration:7").await;
    let body = last_of(&admin.server, "editMessageText").await;
    assert_eq!(
        body["text"],
        "📝 <b>Описание розыгрыша</b>\n\nВведите описание розыгрыша (что разыгрываете):"
    );
    assert_eq!(
        buttons(&body),
        [vec!["nav:back", "nav:cancel"], vec!["nav:main_menu"]]
    );

    admin.say(&"x".repeat(4097)).await;
    assert_eq!(
        admin.last_text().await,
        t("wizard.description_too_long", &[])
    );
    admin.say("Розыгрыш <b>AirPods</b>").await;
    assert_eq!(
        admin.last_text().await,
        "🏆 <b>Количество победителей</b>\n\nВведите число победителей (например: 1, 3, 5):"
    );
    for bad in ["0", "abc", "-2"] {
        admin.say(bad).await;
        assert_eq!(
            admin.last_text().await,
            t("wizard.invalid_winner_count", &[])
        );
    }
    admin.say("3").await;
    assert_eq!(
        admin.last_text().await,
        "📸 <b>Медиа для анонса</b>\n\nОтправьте одно фото, видео, GIF или документ для анонса розыгрыша:"
    );
    admin.send_media("sticker").await;
    assert_eq!(admin.last_text().await, t("wizard.invalid_media", &[]));
    admin.send_media("photo").await;
    let preview = admin.last_text().await;
    assert!(
        preview.starts_with("👁️ <b>Предпросмотр розыгрыша</b>\n\n🗓 Начало: "),
        "{preview}"
    );
    assert!(preview.contains(" 12:00 МСК\n⏰ Окончание: "), "{preview}");
    assert!(preview.ends_with(
        "📅 Длительность: 7 дн.\n🏆 Победителей: 3\n📝 Описание: Розыгрыш <b>AirPods</b>\n📎 Медиа: photo\n\nПодтвердить создание?"
    ), "{preview}");
    assert_eq!(
        buttons(&admin.last().await.1),
        [
            vec!["preview:confirm"],
            vec!["preview:edit"],
            vec!["nav:cancel"]
        ]
    );

    admin.press("preview:confirm").await;
    let giveaway = admin.app.db.active_giveaway().await.unwrap().unwrap();
    assert_ne!(giveaway.id, old);
    assert_eq!(giveaway.num_winners, 3);
    assert_eq!(giveaway.media.file_id, "big", "largest photo size");
    assert_eq!((giveaway.end_at - giveaway.start_at).num_days(), 7);
    let body = last_of(&admin.server, "editMessageText").await;
    assert_eq!(
        body["text"],
        "✅ Розыгрыш успешно создан!\n\n📣 Куда отправить анонс?"
    );
    assert_eq!(
        buttons(&body),
        [
            vec!["announce:channel"],
            vec!["announce:users"],
            vec!["announce:everywhere"],
            vec!["announce:skip"],
            vec!["nav:cancel"],
        ]
    );

    admin.press("announce:everywhere").await;
    assert_eq!(admin.last_text().await, "📤 Отправляю анонс...");
    assert!(admin.dialogue().await.is_none());
    let mailing = admin.app.db.mailing(1).await.unwrap().unwrap();
    assert_eq!(mailing.kind, MailingKind::AnnounceNew);
    assert!(mailing.to_channel);
    assert_eq!(mailing.audience, Audience::Users);
    assert_eq!(mailing.rps, 999);
    assert_eq!((mailing.report_chat, mailing.report_message), (ADMIN, 55));
    assert_eq!(
        mailing.content.text.as_deref(),
        Some(
            "🎉 <b>Новый розыгрыш!</b>\n\nРозыгрыш <b>AirPods</b>\n\n🏆 Победителей: 3\n\n👉 Нажми кнопку ниже для участия!"
        )
    );
    assert!(mailing.content.join_button);
    assert_eq!(mailing.content.media.unwrap().kind, "photo");
}

#[tokio::test]
async fn g5_every_media_kind_is_accepted() {
    for kind in ["video", "animation", "document"] {
        let admin = Admin::new().await;
        admin.press("admin:create_giveaway").await;
        admin.press("start_time:now").await;
        admin.press("duration:1").await;
        admin.say("d").await;
        admin.say("1").await;
        admin.send_media(kind).await;
        let text = admin.last_text().await;
        assert!(
            text.contains(&format!("📎 Медиа: {kind}\n")),
            "{kind}: {text}"
        );
    }
}

#[tokio::test]
async fn g7_edit_returns_to_description_without_back() {
    let admin = Admin::new().await;
    admin.to_preview().await;
    admin.press("preview:edit").await;
    let body = last_of(&admin.server, "editMessageText").await;
    assert_eq!(
        body["text"],
        "📝 <b>Редактирование розыгрыша</b>\n\nВведите новое описание розыгрыша:"
    );
    assert_eq!(buttons(&body), [vec!["nav:cancel", "nav:main_menu"]]);
    assert_eq!(admin.step().await, GwStep::Description);
}

#[tokio::test]
async fn g9_skip_announcement() {
    let admin = Admin::new().await;
    admin.to_preview().await;
    admin.press("preview:confirm").await;
    admin.press("announce:skip").await;
    assert_eq!(admin.last_text().await, "✅ Розыгрыш создан без анонса!");
    assert!(admin.app.db.mailing(1).await.unwrap().is_none());
    assert!(admin.dialogue().await.is_none());
}

#[tokio::test]
async fn g10_back_goes_one_step_back() {
    let admin = Admin::new().await;
    admin.to_preview().await;
    admin.press("nav:cancel").await;
    admin.press("admin:create_giveaway").await;
    admin.press("start_time:1h").await;
    admin.press("duration:3").await;
    admin.say("d").await;
    admin.say("2").await;
    let back = [
        (GwStep::WinnerCount, "🏆 <b>Количество победителей</b>"),
        (GwStep::Description, "📝 <b>Описание розыгрыша</b>"),
        (GwStep::Duration, "📅 <b>Длительность розыгрыша</b>"),
        (GwStep::StartTime, "🗓 <b>Создание розыгрыша</b>"),
    ];
    for (step, heading) in back {
        admin.press("nav:back").await;
        assert_eq!(admin.step().await, step);
        assert!(admin.last_text().await.starts_with(heading));
    }
    admin.press("nav:back").await;
    assert!(admin.dialogue().await.is_none());
    let body = last_of(&admin.server, "editMessageText").await;
    assert_eq!(body["text"], "📋 <b>Админ-панель</b>\n\nВыберите действие:");
    assert_eq!(buttons(&body), menu_data(false));
}

#[tokio::test]
async fn stale_buttons_do_nothing() {
    let admin = Admin::new().await;
    admin.to_preview().await;
    admin.press("preview:confirm").await;
    admin.press("preview:confirm").await; // double tap: the wizard already moved on
    admin.press("duration:3").await;
    admin.press("winners:select").await;
    let giveaways: i64 = sqlx::query_scalar("SELECT count(*) FROM giveaways")
        .fetch_one(admin.app.db.pool())
        .await
        .unwrap();
    assert_eq!(giveaways, 1);
    let (method, body) = admin.last().await;
    assert_eq!(method, "answercallbackquery");
    assert!(body.get("text").is_none());
}

#[tokio::test]
async fn n1_to_n3_manual_announcement() {
    let admin = Admin::new().await;
    admin.press("admin:announce_giveaway").await;
    let (_, body) = admin.last().await;
    assert_eq!(body["text"], "Нет активного розыгрыша для анонсирования");
    assert_eq!(body["show_alert"], true);

    let id = active_giveaway(&admin.app).await;
    admin.press("admin:announce_giveaway").await;
    let giveaway = admin.app.db.giveaway(id).await.unwrap().unwrap();
    let body = last_of(&admin.server, "editMessageText").await;
    let end = tg_bot_giveaway_and_broadcast::time::fmt_msk(giveaway.end_at, "%d.%m.%Y %H:%M");
    assert_eq!(
        body["text"],
        format!(
            "📣 <b>Анонсирование розыгрыша</b>\n\n📝 Приз <b>iPhone</b>\n🏆 Победителей: 2\n⏰ До: {end} МСК\n\nКуда отправить анонс?"
        )
    );
    assert_eq!(
        buttons(&body).concat(),
        [
            "announce_manual:channel",
            "announce_manual:users",
            "announce_manual:everywhere",
            "nav:cancel"
        ]
    );
    admin.press("announce_manual:channel").await;
    assert_eq!(admin.last_text().await, "📤 Отправляю анонс...");
    let mailing = admin.app.db.mailing(1).await.unwrap().unwrap();
    assert_eq!(mailing.kind, MailingKind::Announce);
    assert!(mailing.to_channel);
    assert_eq!(mailing.audience, Audience::Nobody);
}

#[tokio::test]
async fn w1_to_w7_complete_draw_and_publish() {
    let admin = Admin::new().await;
    admin.press("admin:complete_giveaway").await;
    assert_eq!(admin.last().await.1["text"], "Нет активного розыгрыша");

    let id = active_giveaway(&admin.app).await;
    for user in [11, 12, 13] {
        admin
            .app
            .db
            .upsert_user(user, Some(&format!("u{user}")))
            .await
            .unwrap();
        admin
            .app
            .db
            .add_participant(id, user, Some(&format!("u{user}")), now())
            .await
            .unwrap();
    }
    admin.press("admin:complete_giveaway").await;
    let body = last_of(&admin.server, "editMessageText").await;
    assert!(
        body["text"]
            .as_str()
            .unwrap()
            .starts_with("🏁 <b>Завершение розыгрыша</b>\n\n📝 Приз <b>iPhone</b>\n⏰ Окончание: ")
    );
    assert!(
        body["text"]
            .as_str()
            .unwrap()
            .ends_with(" МСК\n\nЗавершить розыгрыш сейчас?")
    );
    assert_eq!(
        buttons(&body).concat(),
        ["giveaway:end_confirm", "giveaway:end_cancel"]
    );

    admin.press("giveaway:end_cancel").await;
    assert_eq!(admin.last_text().await, "❌ Отменено");
    assert!(admin.app.db.active_giveaway().await.unwrap().is_some());

    admin.press("admin:complete_giveaway").await;
    admin.press("giveaway:end_confirm").await;
    assert!(admin.app.db.active_giveaway().await.unwrap().is_none());
    let body = last_of(&admin.server, "editMessageText").await;
    assert_eq!(
        body["text"],
        "✅ Розыгрыш завершен!\n\n🎲 Выбрать победителей?"
    );
    assert_eq!(buttons(&body).concat(), ["winners:select", "nav:back"]);

    admin.press("winners:select").await;
    let winners = admin.app.db.winners(id).await.unwrap();
    assert_eq!(winners.len(), 2);
    let list = tg_bot_giveaway_and_broadcast::db::format_winner_list(&winners);
    let body = last_of(&admin.server, "editMessageText").await;
    assert_eq!(
        body["text"],
        format!(
            "🎉 <b>Победители выбраны!</b>\n\n📝 Приз <b>iPhone</b>\n🏆 Победителей: 2\n\n<b>Победители:</b>\n{list}\n\n📣 Куда опубликовать результаты?"
        )
    );
    assert_eq!(
        buttons(&body).concat(),
        [
            "results:channel",
            "results:admins",
            "results:users",
            "results:everywhere",
            "nav:cancel"
        ]
    );

    admin.press("results:admins").await;
    assert_eq!(admin.last_text().await, "📤 Публикую результаты...");
    let mailing = admin.app.db.mailing(1).await.unwrap().unwrap();
    assert_eq!(mailing.kind, MailingKind::Results);
    assert!(!mailing.to_channel);
    assert_eq!(mailing.audience, Audience::Admins(vec![99, 98]));
    assert_eq!(mailing.rps, 999);
    assert_eq!(
        mailing.content.text.unwrap(),
        format!(
            "🏆 <b>Результаты розыгрыша!</b>\n\n📝 Приз <b>iPhone</b>\n\n<b>Победители:</b>\n{list}\n\nПоздравляем! 🎊\n\n📞 С победителями свяжутся в течение суток после объявления результатов."
        )
    );
    assert!(admin.dialogue().await.is_none());
}

#[tokio::test]
async fn w7_results_to_everyone_use_broadcast_rate() {
    let admin = Admin::new().await;
    let id = active_giveaway(&admin.app).await;
    admin
        .app
        .db
        .add_participant(id, 11, Some("u11"), now())
        .await
        .unwrap();
    admin.press("admin:complete_giveaway").await;
    admin.press("giveaway:end_confirm").await;
    admin.press("winners:select").await;
    admin.press("results:everywhere").await;
    let mailing = admin.app.db.mailing(1).await.unwrap().unwrap();
    assert!(mailing.to_channel);
    assert_eq!(mailing.audience, Audience::Users);
    assert_eq!(mailing.rps, 1000);
}

#[tokio::test]
async fn w4_no_participants() {
    let admin = Admin::new().await;
    active_giveaway(&admin.app).await;
    admin.press("admin:complete_giveaway").await;
    admin.press("giveaway:end_confirm").await;
    admin.press("winners:select").await;
    assert_eq!(admin.last_text().await, t("admin.no_participants", &[]));
    assert!(admin.dialogue().await.is_none());
}

#[tokio::test]
async fn x1_sheets_button_reports_when_disabled() {
    let admin = Admin::new().await;
    admin.press("admin:sync_sheets").await;
    let toast = sent(&admin.server)
        .await
        .into_iter()
        .find(|(m, _)| m == "answercallbackquery")
        .unwrap()
        .1;
    assert_eq!(toast["text"], "Синхронизация начата...");
    for _ in 0..50 {
        if texts(&admin.server).await.len() == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        texts(&admin.server).await,
        ["⚠️ Синхронизация не выполнена (возможно, отключена или нет credentials)"]
    );
}

#[tokio::test]
async fn a2_admin_command_addressed_to_another_bot_is_ignored() {
    let admin = Admin::new().await;
    wiremock::Mock::given(wiremock::matchers::path_regex("(?i)/getme$"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true, "result": {"id": 1, "is_bot": true, "first_name": "Bot",
            "username": "test_bot", "can_join_groups": true,
            "can_read_all_group_messages": false, "supports_inline_queries": false,
            "can_connect_to_business": false, "has_main_web_app": false}})),
        )
        .with_priority(1)
        .mount(&admin.server)
        .await;
    for text in ["/admin@OtherBot", "/admin@test_bot"] {
        handle_message(
            admin.bot.clone(),
            group_text(ADMIN, text),
            admin.app.clone(),
        )
        .await
        .unwrap();
    }
    assert_eq!(
        texts(&admin.server).await,
        [t("admin.use_private_chat", &[])]
    );
}
