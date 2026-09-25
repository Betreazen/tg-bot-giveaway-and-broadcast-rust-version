//! Mass mailings: one background worker, sequential sends with a `1/RPS` pause
//! (the Python bot's pace), progress persisted before every send.
use crate::{
    config::Config,
    db::{Audience, Content, Database, Mailing, MailingKind},
    network::redact,
    time::now,
};
use anyhow::{Context, Result, bail};
use std::{sync::Arc, time::Duration};
use teloxide::{
    RequestError,
    prelude::*,
    types::{FileId, InlineKeyboardButton, InlineKeyboardMarkup, InputFile, MessageId, ParseMode},
};
use tokio::sync::Notify;

const BATCH: i64 = 500;
const IDLE_POLL: Duration = Duration::from_secs(5);

/// Sends `content` to one chat: media with caption, otherwise text; HTML like the Python bot.
pub async fn send_content(bot: &Bot, chat: i64, content: &Content, join_url: &str) -> Result<()> {
    let chat = ChatId(chat);
    let markup = if content.join_button {
        let url = join_url.parse().context("JOIN_URL is not a valid URL")?;
        Some(InlineKeyboardMarkup::new([[InlineKeyboardButton::url(
            "🎁 Участвовать",
            url,
        )]]))
    } else {
        None
    };
    let Some(media) = &content.media else {
        let text = content.text.clone().context("empty mailing content")?;
        let mut request = bot.send_message(chat, text).parse_mode(ParseMode::Html);
        request.reply_markup = markup.map(Into::into);
        request.await?;
        return Ok(());
    };
    let file = InputFile::file_id(FileId(media.file_id.clone()));
    let caption = content.text.clone().filter(|t| !t.is_empty());
    macro_rules! send {
        ($method:ident) => {{
            let mut request = bot.$method(chat, file).parse_mode(ParseMode::Html);
            request.caption = caption;
            request.reply_markup = markup.map(Into::into);
            request.await?;
        }};
    }
    match media.kind.as_str() {
        "photo" => send!(send_photo),
        "video" => send!(send_video),
        "animation" => send!(send_animation),
        "document" => send!(send_document),
        other => bail!("unsupported media type {other}"),
    }
    Ok(())
}

/// Delivers to one recipient, waiting out `429 retry_after` up to `max_retries` times.
async fn deliver(bot: &Bot, chat: i64, content: &Content, config: &Config) -> bool {
    for attempt in 0..=config.max_retries {
        let Err(error) = send_content(bot, chat, content, &config.join_url).await else {
            return true;
        };
        match error.downcast_ref::<RequestError>() {
            Some(RequestError::RetryAfter(wait)) if attempt < config.max_retries => {
                tracing::warn!(chat, seconds = wait.seconds(), "flood control, waiting");
                tokio::time::sleep(wait.duration()).await;
            }
            Some(RequestError::Api(api)) => {
                tracing::debug!(chat, error = %api, "not delivered");
                return false;
            }
            _ => {
                tracing::warn!(chat, error = %redact(&format!("{error:#}"), &config.token), "not delivered");
                return false;
            }
        }
    }
    false
}

/// Runs the next pending mailing to completion; `false` when the queue is empty.
pub async fn run_pending(bot: &Bot, db: &Database, config: &Config) -> Result<bool> {
    let Some(mailing) = db.next_mailing().await? else {
        return Ok(false);
    };
    if mailing.to_channel && !mailing.channel_done {
        db.begin_channel(mailing.id).await?;
        let sent = deliver(bot, config.channel_id, &mailing.content, config).await;
        db.end_channel(mailing.id, sent).await?;
    }
    send_to_recipients(bot, db, config, &mailing).await?;
    // Report before marking done: a crash in between repeats the edit, never loses it.
    let counted = db
        .mailing(mailing.id)
        .await?
        .context("mailing disappeared")?;
    report(bot, &counted, config).await;
    let finished = db.finish_mailing(mailing.id).await?;
    tracing::info!(
        mailing = finished.id,
        sent = finished.sent,
        failed = finished.failed,
        "mailing finished"
    );
    Ok(true)
}

async fn send_to_recipients(
    bot: &Bot,
    db: &Database,
    config: &Config,
    mailing: &Mailing,
) -> Result<()> {
    let pause = Duration::from_secs_f64(1.0 / f64::from(mailing.rps));
    let (mut cursor, mut done) = (mailing.cursor, mailing.sent + mailing.failed);
    tracing::info!(
        mailing = mailing.id,
        total = mailing.total,
        done,
        "mailing started"
    );
    loop {
        let batch = recipients(db, mailing, cursor).await?;
        if batch.is_empty() {
            return Ok(());
        }
        for chat in batch {
            db.begin_send(mailing.id, chat).await?;
            let delivered = deliver(bot, chat, &mailing.content, config).await;
            db.end_send(mailing.id, delivered).await?;
            cursor = Some(chat);
            done += 1;
            if done % 100 == 0 {
                tracing::info!(
                    mailing = mailing.id,
                    done,
                    total = mailing.total,
                    "mailing progress"
                );
            }
            tokio::time::sleep(pause).await;
        }
    }
}

async fn recipients(db: &Database, mailing: &Mailing, after: Option<i64>) -> Result<Vec<i64>> {
    Ok(match &mailing.audience {
        Audience::Nobody => Vec::new(),
        Audience::Users => db.user_recipients(mailing, after, BATCH).await?,
        Audience::Admins(ids) => {
            let mut ids: Vec<i64> = ids.iter().copied().filter(|id| Some(*id) > after).collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        }
    })
}

/// Final texts are the Python bot's, word for word.
pub fn report_text(mailing: &Mailing) -> String {
    let delivered = mailing.sent + i64::from(mailing.channel_sent);
    match mailing.kind {
        MailingKind::Broadcast => {
            let seconds = mailing
                .started_at
                .map_or(0.0, |start| (now() - start).as_seconds_f64());
            format!(
                "✅ <b>Рассылка завершена!</b>\n\n📊 Всего пользователей: {}\n✉️ Отправлено: {}\n❌ Не доставлено: {}\n⏱️ Длительность: {seconds:.1}с",
                mailing.total, mailing.sent, mailing.failed
            )
        }
        MailingKind::Announce => format!("✅ Анонс отправлен!\n\n📊 Отправлено: {delivered}"),
        MailingKind::AnnounceNew => {
            format!("✅ Анонс отправлен!\n\n📊 Отправлено: {delivered}\n🎁 Розыгрыш активен!")
        }
        MailingKind::Results => {
            format!("✅ Результаты опубликованы!\n\n📊 Отправлено: {delivered}")
        }
    }
}

async fn report(bot: &Bot, mailing: &Mailing, config: &Config) {
    let result = bot
        .edit_message_text(
            ChatId(mailing.report_chat),
            MessageId(mailing.report_message),
            report_text(mailing),
        )
        .parse_mode(ParseMode::Html)
        .await;
    if let Err(error) = result {
        tracing::warn!(mailing = mailing.id, error = %redact(&error.to_string(), &config.token), "report edit failed");
    }
}

/// Background loop: drains the queue, then sleeps until notified or `IDLE_POLL` passes.
pub async fn mailing_worker(bot: Bot, db: Database, config: Arc<Config>, notify: Arc<Notify>) {
    loop {
        match run_pending(&bot, &db, &config).await {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => {
                tracing::error!(error = %redact(&format!("{error:#}"), &config.token), "mailing worker failed");
            }
        }
        let _ = tokio::time::timeout(IDLE_POLL, notify.notified()).await;
    }
}
