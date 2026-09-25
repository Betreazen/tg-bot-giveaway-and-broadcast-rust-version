//! Announcing the active giveaway (Python announce.py); the content is shared with the wizard.
use super::{Cb, enqueue, keyboards};
use crate::{
    db::{Audience, Content, Giveaway, MailingKind},
    time::fmt_msk,
};
use anyhow::Result;

pub fn content(giveaway: &Giveaway) -> Content {
    Content {
        text: Some(format!(
            "🎉 <b>Новый розыгрыш!</b>\n\n{}\n\n🏆 Победителей: {}\n\n👉 Нажми кнопку ниже для участия!",
            giveaway.description, giveaway.num_winners
        )),
        media: Some(giveaway.media.clone()),
        join_button: true,
    }
}

/// `channel`, `users` or `everywhere` → post to the channel?, which users.
pub fn targets(target: &str) -> (bool, Audience) {
    let to_channel = matches!(target, "channel" | "everywhere");
    let audience = if matches!(target, "users" | "everywhere") {
        Audience::Users
    } else {
        Audience::Nobody
    };
    (to_channel, audience)
}

pub async fn prompt(cb: &Cb<'_>) -> Result<()> {
    let Some(giveaway) = cb.app.db.active_giveaway().await? else {
        return cb.alert("Нет активного розыгрыша для анонсирования").await;
    };
    let text = format!(
        "📣 <b>Анонсирование розыгрыша</b>\n\n📝 {}\n🏆 Победителей: {}\n⏰ До: {} МСК\n\nКуда отправить анонс?",
        giveaway.description,
        giveaway.num_winners,
        fmt_msk(giveaway.end_at, "%d.%m.%Y %H:%M")
    );
    cb.edit(
        &text,
        Some(keyboards::announce_target("announce_manual", false)),
    )
    .await
}

pub async fn send(cb: &Cb<'_>, target: &str) -> Result<()> {
    cb.edit("📤 Отправляю анонс...", None).await?;
    let Some(giveaway) = cb.app.db.active_giveaway().await? else {
        return cb.edit("❌ Активный розыгрыш не найден", None).await;
    };
    let (to_channel, audience) = targets(target);
    let rps = cb.app.config.announce_rps;
    let queued = enqueue(
        cb,
        MailingKind::Announce,
        content(&giveaway),
        to_channel,
        audience,
        rps,
    );
    if let Err(error) = queued.await {
        tracing::error!(error = %cb.app.redact(&error), "announcement failed");
        return cb.edit("❌ Ошибка отправки анонса", None).await;
    }
    Ok(())
}
