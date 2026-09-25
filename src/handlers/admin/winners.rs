//! Ending the giveaway, drawing winners and publishing results (Python winners.py).
use super::{Cb, enqueue, keyboards};
use crate::{
    db::{Audience, Content, Draw, MailingKind, format_winner_list},
    state::{Dialogue, WinStep},
    text::t,
    time::fmt_msk,
};
use anyhow::Result;

async fn save(cb: &Cb<'_>, step: WinStep, giveaway_id: i64) -> Result<()> {
    let dialogue = Dialogue::Winners { step, giveaway_id };
    cb.app.db.save_dialogue(cb.user, &dialogue).await
}

async fn stop(cb: &Cb<'_>, text: &str) -> Result<()> {
    cb.app.db.clear_dialogue(cb.user).await?;
    cb.edit(text, None).await
}

pub async fn start(cb: &Cb<'_>) -> Result<()> {
    let Some(giveaway) = cb.app.db.active_giveaway().await? else {
        return cb.alert("Нет активного розыгрыша").await;
    };
    let text = format!(
        "🏁 <b>Завершение розыгрыша</b>\n\n📝 {}\n⏰ Окончание: {} МСК\n\nЗавершить розыгрыш сейчас?",
        giveaway.description,
        fmt_msk(giveaway.end_at, "%d.%m.%Y %H:%M")
    );
    cb.edit(&text, Some(keyboards::end_confirm())).await?;
    save(cb, WinStep::ConfirmEnd, giveaway.id).await
}

pub async fn on_callback(
    cb: &Cb<'_>,
    data: (&str, &str),
    step: WinStep,
    giveaway_id: i64,
) -> Result<()> {
    match (data, step) {
        (("giveaway", "end_confirm"), WinStep::ConfirmEnd) => end(cb, giveaway_id).await,
        (("giveaway", "end_cancel"), WinStep::ConfirmEnd) => stop(cb, "❌ Отменено").await,
        (("winners", "select"), WinStep::Select) => draw(cb, giveaway_id).await,
        (("results", target), WinStep::Publish) => publish(cb, giveaway_id, target).await,
        _ => Ok(()),
    }
}

async fn end(cb: &Cb<'_>, giveaway_id: i64) -> Result<()> {
    match cb.app.db.end_giveaway(giveaway_id).await {
        Ok(Some(_)) => {
            let text = "✅ Розыгрыш завершен!\n\n🎲 Выбрать победителей?";
            cb.edit(text, Some(keyboards::select_winners())).await?;
            save(cb, WinStep::Select, giveaway_id).await
        }
        Ok(None) => stop(cb, "❌ Розыгрыш не найден").await,
        Err(error) => {
            tracing::error!(error = %cb.app.redact(&error), "ending the giveaway failed");
            stop(cb, &t("errors.database", &[])).await
        }
    }
}

async fn draw(cb: &Cb<'_>, giveaway_id: i64) -> Result<()> {
    cb.edit("🎲 Выбираю победителей...", None).await?;
    let Some(giveaway) = cb.app.db.giveaway(giveaway_id).await? else {
        return stop(cb, "❌ Розыгрыш не найден").await;
    };
    let winners = match cb.app.db.draw_winners(&giveaway, &mut rand::rng()).await {
        Ok(Draw::Winners(winners)) => winners,
        Ok(Draw::NoParticipants) => return stop(cb, &t("admin.no_participants", &[])).await,
        Err(error) => {
            tracing::error!(error = %cb.app.redact(&error), "winner draw failed");
            return stop(cb, &t("errors.generic", &[])).await;
        }
    };
    tracing::info!(
        giveaway = giveaway.id,
        winners = winners.len(),
        "winners drawn"
    );
    let text = format!(
        "🎉 <b>Победители выбраны!</b>\n\n📝 {}\n🏆 Победителей: {}\n\n<b>Победители:</b>\n{}\n\n📣 Куда опубликовать результаты?",
        giveaway.description,
        winners.len(),
        format_winner_list(&winners)
    );
    cb.edit(&text, Some(keyboards::results_target())).await?;
    save(cb, WinStep::Publish, giveaway_id).await
}

async fn publish(cb: &Cb<'_>, giveaway_id: i64, target: &str) -> Result<()> {
    cb.app.db.clear_dialogue(cb.user).await?;
    cb.edit("📤 Публикую результаты...", None).await?;
    let giveaway = cb.app.db.giveaway(giveaway_id).await?;
    let winners = cb.app.db.winners(giveaway_id).await?;
    let (Some(giveaway), false) = (giveaway, winners.is_empty()) else {
        return cb.edit("❌ Данные не найдены", None).await;
    };
    let text = format!(
        "🏆 <b>Результаты розыгрыша!</b>\n\n📝 {}\n\n<b>Победители:</b>\n{}\n\nПоздравляем! 🎊\n\n📞 С победителями свяжутся в течение суток после объявления результатов.",
        giveaway.description,
        format_winner_list(&winners)
    );
    let config = &cb.app.config;
    let (audience, rps) = match target {
        "admins" => (
            Audience::Admins(config.admin_ids.clone()),
            config.announce_rps,
        ),
        "users" | "everywhere" => (Audience::Users, config.broadcast_rps),
        _ => (Audience::Nobody, config.broadcast_rps),
    };
    let to_channel = matches!(target, "channel" | "everywhere");
    let content = Content {
        text: Some(text),
        media: None,
        join_button: false,
    };
    if let Err(error) = enqueue(cb, MailingKind::Results, content, to_channel, audience, rps).await
    {
        tracing::error!(error = %cb.app.redact(&error), "publishing results failed");
        return cb.edit("❌ Ошибка публикации", None).await;
    }
    Ok(())
}
