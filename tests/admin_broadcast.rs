//! Broadcast wizard and suspicious accounts: PARITY.md B1–B7, S1–S5.
mod common;

use common::*;
use serde_json::Value;
use std::sync::Arc;
use teloxide::{Bot, types::Message};
use tg_bot_giveaway_and_broadcast::{
    db::{Audience, MailingKind},
    handlers::{App, handle_callback, handle_message},
    state::{BcStep, Dialogue},
    text::{paginate, t},
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

    async fn press(&self, data: &str) {
        handle_callback(self.bot.clone(), callback(ADMIN, data), self.app.clone())
            .await
            .unwrap();
    }

    async fn post(&self, message: Message) {
        handle_message(self.bot.clone(), message, self.app.clone())
            .await
            .unwrap();
    }

    async fn say(&self, text: &str) {
        self.post(common::text(ADMIN, text)).await;
    }

    async fn last_text(&self) -> String {
        texts(&self.server).await.pop().unwrap()
    }

    async fn last_body(&self) -> Value {
        let calls = sent(&self.server).await;
        calls
            .into_iter()
            .rev()
            .find(|(m, _)| m == "sendmessage" || m == "editmessagetext")
            .unwrap()
            .1
    }

    async fn step(&self) -> Option<BcStep> {
        match self.app.db.load_dialogue(ADMIN).await.unwrap() {
            Some(Dialogue::Broadcast { step, .. }) => Some(step),
            _ => None,
        }
    }
}

const PREVIEW_BUTTONS: [&str; 3] = ["preview:confirm", "preview:edit", "nav:cancel"];

#[tokio::test]
async fn b1_b2_b5_text_broadcast_is_queued_for_all_users() {
    let admin = Admin::new().await;
    admin.app.db.upsert_user(1, None).await.unwrap();
    admin.press("admin:broadcast").await;
    let body = admin.last_body().await;
    assert_eq!(
        body["text"],
        "📢 <b>Рассылка сообщений</b>\n\nВыберите тип рассылки:"
    );
    assert_eq!(
        buttons(&body).concat(),
        ["broadcast:text", "broadcast:media", "nav:cancel"]
    );
    admin.press("broadcast:text").await;
    let body = admin.last_body().await;
    assert_eq!(
        body["text"],
        "✏️ <b>Текстовая рассылка</b>\n\nВведите текст сообщения:"
    );
    assert_eq!(
        buttons(&body),
        [vec!["nav:back", "nav:cancel"], vec!["nav:main_menu"]]
    );

    admin.say(&"я".repeat(4097)).await;
    assert_eq!(
        admin.last_text().await,
        "❌ Текст слишком длинный (максимум 4096 символов)"
    );
    admin.say("Привет, <b>мир</b>!").await;
    let body = admin.last_body().await;
    assert_eq!(
        body["text"],
        "👁️ <b>Предпросмотр рассылки</b>\n\nПривет, <b>мир</b>!\n\n📏 Символов: 19\n\nПодтвердить отправку?"
    );
    assert_eq!(buttons(&body).concat(), PREVIEW_BUTTONS);
    assert_eq!(admin.step().await, Some(BcStep::Confirm));

    admin.press("preview:confirm").await;
    assert_eq!(admin.last_text().await, "📤 Начинаю рассылку...");
    assert_eq!(admin.step().await, None);
    let mailing = admin.app.db.mailing(1).await.unwrap().unwrap();
    assert_eq!(mailing.kind, MailingKind::Broadcast);
    assert!(!mailing.to_channel);
    assert_eq!(mailing.audience, Audience::Users);
    assert_eq!(mailing.rps, 1000);
    assert_eq!(mailing.content.text.as_deref(), Some("Привет, <b>мир</b>!"));
    assert!(mailing.content.media.is_none() && !mailing.content.join_button);
}

#[tokio::test]
async fn b3_media_broadcast_with_and_without_caption() {
    let admin = Admin::new().await;
    admin.app.db.upsert_user(1, None).await.unwrap();
    admin.press("admin:broadcast").await;
    admin.press("broadcast:media").await;
    assert_eq!(
        admin.last_text().await,
        "📎 <b>Рассылка с медиа</b>\n\nОтправьте фото, видео, GIF или документ с подписью (необязательно):"
    );
    admin.say("just text").await;
    assert_eq!(admin.last_text().await, t("wizard.invalid_media", &[]));
    admin.post(media(ADMIN, "video", Some("Смотри!"))).await;
    assert_eq!(
        admin.last_text().await,
        "👁️ <b>Предпросмотр рассылки</b>\n\n📎 Медиа: video\n📝 Подпись: Смотри!\n📏 Символов: 7\n\nПодтвердить отправку?"
    );
    admin.press("preview:edit").await;
    assert_eq!(
        admin.last_text().await,
        "📎 Отправьте новое медиа с подписью:"
    );
    assert_eq!(admin.step().await, Some(BcStep::Media));
    admin.post(media(ADMIN, "photo", None)).await;
    assert_eq!(
        admin.last_text().await,
        "👁️ <b>Предпросмотр рассылки</b>\n\n📎 Медиа: photo\n📝 Подпись: (нет)\n📏 Символов: 0\n\nПодтвердить отправку?"
    );
    admin.press("preview:confirm").await;
    let content = admin.app.db.mailing(1).await.unwrap().unwrap().content;
    assert_eq!(content.media.unwrap().file_id, "big");
    assert!(content.text.is_none(), "an empty caption is not sent");
}

#[tokio::test]
async fn b4_edit_text_and_b5_empty_audience() {
    let admin = Admin::new().await;
    admin.press("admin:broadcast").await;
    admin.press("broadcast:text").await;
    admin.say("old").await;
    admin.press("preview:edit").await;
    let body = admin.last_body().await;
    assert_eq!(body["text"], "✏️ Введите новый текст:");
    assert_eq!(
        buttons(&body),
        [vec!["nav:back", "nav:cancel"], vec!["nav:main_menu"]]
    );
    admin.say("new").await;
    admin.press("preview:confirm").await;
    assert_eq!(
        admin.last_text().await,
        "❌ В базе нет пользователей для рассылки"
    );
    assert!(admin.app.db.mailing(1).await.unwrap().is_none());
    assert_eq!(admin.step().await, None);
}

#[tokio::test]
async fn back_returns_to_type_selection() {
    let admin = Admin::new().await;
    for kind in ["broadcast:text", "broadcast:media"] {
        admin.press("admin:broadcast").await;
        admin.press(kind).await;
        admin.press("nav:back").await;
        assert_eq!(admin.step().await, Some(BcStep::Type));
        assert_eq!(
            admin.last_text().await,
            "📢 <b>Рассылка сообщений</b>\n\nВыберите тип рассылки:"
        );
    }
}

const MENU: &str = "🚩 <b>Подозрительные аккаунты</b>\n\nПомеченные аккаунты участвуют в розыгрышах, но <b>никогда не выигрывают</b>. Пользователь об этом не узнаёт.\n\nВыберите действие:";
const ENTER: &str = "Введите username пользователя в любом виде:\n<code>@username</code>, <code>https://t.me/username</code> или просто <code>username</code>.";
const MENU_BUTTONS: [&str; 4] = [
    "suspicious:mark",
    "suspicious:unmark",
    "suspicious:list",
    "nav:main_menu",
];

#[tokio::test]
async fn s1_s2_s3_mark_and_unmark_by_any_username_form() {
    let admin = Admin::new().await;
    admin
        .app
        .db
        .upsert_user(42, Some("Cheater_1"))
        .await
        .unwrap();
    admin.press("admin:suspicious").await;
    let body = admin.last_body().await;
    assert_eq!(body["text"], MENU);
    assert_eq!(buttons(&body).concat(), MENU_BUTTONS);

    admin.press("suspicious:mark").await;
    let body = admin.last_body().await;
    assert_eq!(
        body["text"],
        format!("🚩 <b>Пометить подозрительным</b>\n\n{ENTER}")
    );
    assert_eq!(
        buttons(&body),
        [vec!["nav:back", "nav:cancel"], vec!["nav:main_menu"]]
    );
    admin.say("https://t.me/cheater_1?start=x").await;
    let body = admin.last_body().await;
    assert_eq!(
        body["text"],
        "🚩 <code>@cheater_1</code> (ID <code>42</code>) помечен как подозрительный. Участвует, но не выигрывает."
    );
    assert_eq!(buttons(&body).concat(), MENU_BUTTONS);
    assert_eq!(admin.app.db.suspicious_users().await.unwrap().len(), 1);
    assert!(admin.app.db.load_dialogue(ADMIN).await.unwrap().is_none());

    admin.press("suspicious:unmark").await;
    assert_eq!(
        admin.last_text().await,
        format!("✅ <b>Снять метку подозрительного</b>\n\n{ENTER}")
    );
    admin.say("@CHEATER_1").await;
    assert_eq!(
        admin.last_text().await,
        "✅ С <code>@cheater_1</code> (ID <code>42</code>) снята метка «подозрительный»."
    );
    assert!(admin.app.db.suspicious_users().await.unwrap().is_empty());

    admin.press("suspicious:mark").await;
    admin.say("nobody_here").await;
    assert_eq!(
        admin.last_text().await,
        "⚠️ Пользователь <code>@nobody_here</code> не найден в базе (он должен был хотя бы раз запустить бота)."
    );
    admin.press("suspicious:mark").await;
    admin.say("not a name!").await;
    assert_eq!(
        admin.last_text().await,
        "❌ Не удалось распознать username. Пришлите его в виде <code>@username</code>, ссылки или просто <code>username</code>."
    );
    assert!(admin.app.db.load_dialogue(ADMIN).await.unwrap().is_none());
}

#[tokio::test]
async fn s4_list_is_sorted_and_paginated() {
    let admin = Admin::new().await;
    admin.press("suspicious:list").await;
    let body = admin.last_body().await;
    assert_eq!(body["text"], "📋 Список подозрительных пуст.");
    assert_eq!(buttons(&body).concat(), MENU_BUTTONS);

    for (id, name) in [(3, "zed_user"), (1, "alice_1"), (2, "bob_22")] {
        admin.app.db.upsert_user(id, Some(name)).await.unwrap();
        admin.app.db.set_suspicious(name, true).await.unwrap();
    }
    admin.press("suspicious:list").await;
    assert_eq!(
        admin.last_text().await,
        "📋 <b>Подозрительные аккаунты (3)</b>\n\n1. @alice_1 (ID <code>1</code>)\n2. @bob_22 (ID <code>2</code>)\n3. @zed_user (ID <code>3</code>)"
    );

    for id in 100..400 {
        let name = format!("long_suspicious_name_{id}");
        admin.app.db.upsert_user(id, Some(&name)).await.unwrap();
        admin.app.db.set_suspicious(&name, true).await.unwrap();
    }
    let before = texts(&admin.server).await.len();
    admin.press("suspicious:list").await;
    let pages = &texts(&admin.server).await[before..];
    assert!(pages.len() > 1);
    assert!(pages.iter().all(|p| p.chars().count() <= 3800));
    assert!(pages[0].starts_with("📋 <b>Подозрительные аккаунты (303)</b>"));
    let first = sent(&admin.server).await[before..]
        .iter()
        .find(|(m, _)| m == "editmessagetext")
        .unwrap()
        .1
        .clone();
    assert_eq!(
        buttons(&first).concat(),
        MENU_BUTTONS,
        "menu stays under page one"
    );
}

#[tokio::test]
async fn s_back_returns_to_the_suspicious_menu() {
    let admin = Admin::new().await;
    admin.press("suspicious:mark").await;
    admin.press("nav:back").await;
    assert_eq!(admin.last_text().await, MENU);
    assert!(admin.app.db.load_dialogue(ADMIN).await.unwrap().is_none());
}

// Port of the Python bot's tests/test_suspicious_list.py.
#[test]
fn paginate_single_page_when_short() {
    let lines: Vec<String> = (0..5).map(|i| format!("{i}. @user{i}")).collect();
    let pages = paginate("HEAD\n\n", &lines, 3800);
    assert_eq!(pages.len(), 1);
    assert!(lines.iter().all(|l| pages[0].contains(l.as_str())));
}

#[test]
fn paginate_empty_returns_header() {
    assert_eq!(paginate("HEAD", &[], 3800), ["HEAD"]);
}

#[test]
fn paginate_splits_under_limit_and_keeps_every_line() {
    let lines: Vec<String> = (0..1000)
        .map(|i| format!("{i}. @some_long_username_{i}"))
        .collect();
    let pages = paginate("HEAD\n\n", &lines, 1000);
    assert!(pages.len() > 1);
    assert!(pages.iter().all(|p| p.chars().count() <= 1000));
    let joined = pages.join("\n");
    assert!(lines.iter().all(|l| joined.contains(l.as_str())));
}
