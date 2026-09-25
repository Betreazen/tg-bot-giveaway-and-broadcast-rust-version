//! Main menu, status, close, cancel and the Google Sheets button (Python entry.py, menu.py).
use super::{super::send, Cb, keyboards};
use crate::{
    handlers::App,
    network::redact,
    sheets::{SyncOutcome, sync_all},
    text::t,
    time::fmt_msk,
};
use anyhow::Result;
use std::sync::atomic::Ordering;
use teloxide::prelude::*;

pub async fn has_active(app: &App) -> Result<bool> {
    Ok(app.db.active_giveaway().await?.is_some())
}

pub async fn show(bot: &Bot, app: &App, chat: ChatId) -> Result<()> {
    let markup = keyboards::main_menu(has_active(app).await?);
    send(bot, chat, &t("admin.main_menu", &[]), Some(markup)).await?;
    Ok(())
}

pub async fn main_menu(cb: &Cb<'_>) -> Result<()> {
    cb.app.db.clear_dialogue(cb.user).await?;
    let markup = keyboards::main_menu(has_active(cb.app).await?);
    cb.edit(&t("admin.main_menu", &[]), Some(markup)).await
}

pub async fn cancel(cb: &Cb<'_>) -> Result<()> {
    cb.app.db.clear_dialogue(cb.user).await?;
    cb.edit(&t("admin.operation_cancelled", &[]), None).await
}

pub async fn close(cb: &Cb<'_>) -> Result<()> {
    cb.bot.delete_message(cb.chat, cb.message).await?;
    Ok(())
}

pub async fn status(cb: &Cb<'_>) -> Result<()> {
    let Some(giveaway) = cb.app.db.active_giveaway().await? else {
        return cb.alert(&t("admin.status_no_active", &[])).await;
    };
    let participants = cb.app.db.participant_count(giveaway.id).await?;
    let text = t(
        "admin.status_active",
        &[
            ("description", &giveaway.description),
            ("participants", &participants),
            ("end_at", &fmt_msk(giveaway.end_at, "%Y-%m-%d %H:%M")),
            ("num_winners", &giveaway.num_winners),
        ],
    );
    cb.send(&text, None).await?;
    Ok(())
}

const SHEETS_DONE: &str = "✅ Синхронизация с Google Sheets успешно завершена!";
const SHEETS_SKIPPED: &str =
    "⚠️ Синхронизация не выполнена (возможно, отключена или нет credentials)";

/// Runs in the background, one sync at a time; the result arrives as a new message.
/// Any failure reads as "not performed", as Python's sync_all swallowed its errors.
pub async fn sync_sheets(cb: &Cb<'_>) -> Result<()> {
    cb.toast("Синхронизация начата...").await?;
    if cb.app.sheets_busy.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let (bot, db, config) = (cb.bot.clone(), cb.app.db.clone(), cb.app.config.clone());
    let (busy, chat) = (cb.app.sheets_busy.clone(), cb.chat);
    tokio::spawn(async move {
        let outcome = sync_all(&db, &config).await;
        busy.store(false, Ordering::SeqCst);
        let text = match outcome {
            Ok(SyncOutcome::Done) => SHEETS_DONE,
            Ok(SyncOutcome::Skipped) => SHEETS_SKIPPED,
            Err(error) => {
                let error = redact(&format!("{error:#}"), &config.token);
                tracing::error!(%error, "Google Sheets sync failed");
                SHEETS_SKIPPED
            }
        };
        if let Err(error) = send(&bot, chat, text, None).await {
            tracing::warn!(error = %redact(&format!("{error:#}"), &config.token), "sheets report failed");
        }
    });
    Ok(())
}
