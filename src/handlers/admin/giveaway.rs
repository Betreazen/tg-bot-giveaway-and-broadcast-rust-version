//! Giveaway creation wizard and the announcement right after it (Python giveaway_wizard.py).
use super::{App, Cb, Input, announce, enqueue, keyboards, media_of, menu};
use crate::{
    db::{MailingKind, NewGiveaway},
    state::{Dialogue, GwDraft, GwStep},
    text::t,
    time::{calculate_dates, format_dates_display, from_db, now, to_db},
};
use anyhow::{Context, Result};

const START: &str = "🗓 <b>Создание розыгрыша</b>\n\nКогда начать розыгрыш?";
const DURATION: &str = "📅 <b>Длительность розыгрыша</b>\n\nСколько будет длиться розыгрыш?";
const DESCRIPTION: &str =
    "📝 <b>Описание розыгрыша</b>\n\nВведите описание розыгрыша (что разыгрываете):";
const WINNERS: &str =
    "🏆 <b>Количество победителей</b>\n\nВведите число победителей (например: 1, 3, 5):";
const MEDIA: &str = "📸 <b>Медиа для анонса</b>\n\nОтправьте одно фото, видео, GIF или документ для анонса розыгрыша:";
const EDIT: &str = "📝 <b>Редактирование розыгрыша</b>\n\nВведите новое описание розыгрыша:";
const MAX_DESCRIPTION: usize = 4096;

async fn save(app: &App, user: i64, step: GwStep, draft: GwDraft) -> Result<()> {
    app.db
        .save_dialogue(user, &Dialogue::Giveaway { step, draft })
        .await
}

pub async fn start(cb: &Cb<'_>) -> Result<()> {
    cb.edit(START, Some(keyboards::start_time())).await?;
    save(cb.app, cb.user, GwStep::StartTime, GwDraft::default()).await
}

pub async fn on_callback(
    cb: &Cb<'_>,
    data: (&str, &str),
    step: GwStep,
    mut draft: GwDraft,
) -> Result<()> {
    match (data, step) {
        (("start_time", option), GwStep::StartTime) => {
            draft.start_option = Some(option.to_owned());
            cb.edit(DURATION, Some(keyboards::duration())).await?;
            save(cb.app, cb.user, GwStep::Duration, draft).await
        }
        (("duration", days), GwStep::Duration) => {
            let Ok(days) = days.parse() else {
                return Ok(());
            };
            let option = draft.start_option.as_deref().unwrap_or("now");
            let (start, end) = calculate_dates(option, days, now());
            draft.start_at = Some(to_db(start));
            draft.end_at = Some(to_db(end));
            let nav = keyboards::navigation(true, true, true);
            cb.edit(DESCRIPTION, Some(nav)).await?;
            save(cb.app, cb.user, GwStep::Description, draft).await
        }
        (("nav", "back"), step) => back(cb, step, draft).await,
        (("preview", "confirm"), GwStep::Preview) => confirm(cb, draft).await,
        (("preview", "edit"), GwStep::Preview) => {
            cb.edit(EDIT, Some(keyboards::navigation(false, true, true)))
                .await?;
            save(cb.app, cb.user, GwStep::Description, draft).await
        }
        (("announce", target), GwStep::AnnounceTarget) => announce_new(cb, target, draft).await,
        _ => Ok(()),
    }
}

async fn back(cb: &Cb<'_>, step: GwStep, draft: GwDraft) -> Result<()> {
    let nav = || Some(keyboards::navigation(true, true, true));
    let (text, markup, previous) = match step {
        GwStep::StartTime => {
            cb.app.db.clear_dialogue(cb.user).await?;
            let markup = keyboards::main_menu(menu::has_active(cb.app).await?);
            return cb
                .edit("📋 <b>Админ-панель</b>\n\nВыберите действие:", Some(markup))
                .await;
        }
        GwStep::Duration => (START, Some(keyboards::start_time()), GwStep::StartTime),
        GwStep::Description => (DURATION, Some(keyboards::duration()), GwStep::Duration),
        GwStep::WinnerCount => (DESCRIPTION, nav(), GwStep::Description),
        GwStep::Media => (WINNERS, nav(), GwStep::WinnerCount),
        GwStep::Preview | GwStep::AnnounceTarget => return Ok(()),
    };
    cb.edit(text, markup).await?;
    save(cb.app, cb.user, previous, draft).await
}

pub async fn on_message(input: &Input<'_>, step: GwStep, mut draft: GwDraft) -> Result<()> {
    let text = input.msg.text();
    let nav = || Some(keyboards::navigation(true, true, true));
    let next = match (step, text) {
        (GwStep::Description, Some(text)) => {
            if text.chars().count() > MAX_DESCRIPTION {
                return input
                    .reply(&t("wizard.description_too_long", &[]), None)
                    .await;
            }
            draft.description = Some(text.to_owned());
            input.reply(WINNERS, nav()).await?;
            GwStep::WinnerCount
        }
        (GwStep::WinnerCount, Some(text)) => {
            let Some(count) = text.trim().parse::<i64>().ok().filter(|n| *n >= 1) else {
                return input
                    .reply(&t("wizard.invalid_winner_count", &[]), None)
                    .await;
            };
            draft.num_winners = Some(count);
            input.reply(MEDIA, nav()).await?;
            GwStep::Media
        }
        (GwStep::Media, _) => {
            let Some(media) = media_of(input.msg) else {
                return input.reply(&t("wizard.invalid_media", &[]), None).await;
            };
            draft.media = Some(media);
            input
                .reply(&preview(&draft)?, Some(keyboards::preview()))
                .await?;
            GwStep::Preview
        }
        _ => return Ok(()),
    };
    save(input.app, input.user, next, draft).await
}

fn preview(draft: &GwDraft) -> Result<String> {
    let dates = format_dates_display(
        from_db(draft.start_at.as_deref().context("no start")?)?,
        from_db(draft.end_at.as_deref().context("no end")?)?,
    );
    Ok(format!(
        "👁️ <b>Предпросмотр розыгрыша</b>\n\n{dates}\n🏆 Победителей: {}\n📝 Описание: {}\n📎 Медиа: {}\n\nПодтвердить создание?",
        draft.num_winners.unwrap_or_default(),
        draft.description.as_deref().unwrap_or_default(),
        draft.media.as_ref().map_or("", |m| m.kind.as_str()),
    ))
}

fn new_giveaway(draft: &GwDraft, admin: i64) -> Result<NewGiveaway> {
    Ok(NewGiveaway {
        start_at: from_db(draft.start_at.as_deref().context("no start")?)?,
        end_at: from_db(draft.end_at.as_deref().context("no end")?)?,
        description: draft.description.clone().context("no description")?,
        num_winners: draft.num_winners.context("no winner count")?,
        media: draft.media.clone().context("no media")?,
        created_by: admin,
    })
}

async fn confirm(cb: &Cb<'_>, mut draft: GwDraft) -> Result<()> {
    let created = match new_giveaway(&draft, cb.user) {
        Ok(new) => cb.app.db.create_giveaway(&new).await,
        Err(error) => Err(error),
    };
    let id = match created {
        Ok(id) => id,
        Err(error) => {
            tracing::error!(error = %cb.app.redact(&error), "giveaway creation failed");
            cb.app.db.clear_dialogue(cb.user).await?;
            return cb.edit(&t("errors.database", &[]), None).await;
        }
    };
    tracing::info!(giveaway = id, admin = cb.user, "giveaway created");
    let markup = keyboards::announce_target("announce", true);
    cb.edit(
        "✅ Розыгрыш успешно создан!\n\n📣 Куда отправить анонс?",
        Some(markup),
    )
    .await?;
    draft.giveaway_id = Some(id);
    save(cb.app, cb.user, GwStep::AnnounceTarget, draft).await
}

async fn announce_new(cb: &Cb<'_>, target: &str, draft: GwDraft) -> Result<()> {
    cb.app.db.clear_dialogue(cb.user).await?;
    if target == "skip" {
        return cb.edit("✅ Розыгрыш создан без анонса!", None).await;
    }
    cb.edit("📤 Отправляю анонс...", None).await?;
    let giveaway = match draft.giveaway_id {
        Some(id) => cb.app.db.giveaway(id).await?,
        None => None,
    };
    let Some(giveaway) = giveaway else {
        return cb.edit("❌ Розыгрыш не найден", None).await;
    };
    let (to_channel, audience) = announce::targets(target);
    let queued = enqueue(
        cb,
        MailingKind::AnnounceNew,
        announce::content(&giveaway),
        to_channel,
        audience,
        cb.app.config.announce_rps,
    )
    .await;
    if let Err(error) = queued {
        tracing::error!(error = %cb.app.redact(&error), "announcement failed");
        return cb
            .edit("❌ Ошибка отправки анонса, но розыгрыш создан", None)
            .await;
    }
    Ok(())
}
