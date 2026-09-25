//! Update routing. Updates of one user are handled one at a time; dialogue state lives in SQLite.
mod admin;
mod user;

use crate::{config::Config, db::Database, network::redact, state::Dialogue, text::t};
use anyhow::{Result, anyhow};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use teloxide::{
    prelude::*,
    types::{CallbackQueryId, InlineKeyboardMarkup, MessageId, ParseMode},
};
use tokio::sync::{Mutex as AsyncMutex, Notify, OwnedMutexGuard};

pub struct App {
    pub config: Arc<Config>,
    pub db: Database,
    /// Wakes the mailing worker when something is queued.
    pub mailings: Arc<Notify>,
    /// Set while a Google Sheets sync runs; one at a time.
    pub sheets_busy: Arc<AtomicBool>,
    locks: UserLocks,
}

impl App {
    pub async fn open(config: Config) -> Result<Arc<Self>> {
        let db = Database::open(&config.data_dir).await?;
        Ok(Arc::new(Self {
            config: Arc::new(config),
            db,
            mailings: Arc::default(),
            sheets_busy: Arc::default(),
            locks: UserLocks::default(),
        }))
    }

    pub fn redact(&self, error: &anyhow::Error) -> String {
        redact(&format!("{error:#}"), &self.config.token)
    }
}

/// Serialises updates per user so a double tap cannot race itself. teloxide already
/// serialises updates per chat, but updates without a chat (a callback whose message is
/// no longer accessible) go to its concurrent default worker; this lock covers those too.
#[derive(Default)]
struct UserLocks {
    entries: Mutex<HashMap<i64, Weak<AsyncMutex<()>>>>,
}

impl UserLocks {
    async fn lock(&self, id: i64) -> Result<OwnedMutexGuard<()>> {
        let lock = {
            let mut entries = self.entries.lock().map_err(|_| anyhow!("lock poisoned"))?;
            entries.retain(|_, lock| lock.strong_count() > 0);
            match entries.get(&id).and_then(Weak::upgrade) {
                Some(lock) => lock,
                None => {
                    let lock = Arc::new(AsyncMutex::new(()));
                    entries.insert(id, Arc::downgrade(&lock));
                    lock
                }
            }
        };
        Ok(lock.lock_owned().await)
    }
}

/// `/start join` → (`start`, None), `/admin@bot` → (`admin`, Some("bot")).
fn command(text: Option<&str>) -> Option<(&str, Option<&str>)> {
    let word = text?.split_whitespace().next()?.strip_prefix('/')?;
    Some(match word.split_once('@') {
        Some((name, bot)) => (name, Some(bot)),
        None => (word, None),
    })
}

/// In groups `/admin@OtherBot` belongs to another bot, as aiogram's Command filter decides.
async fn addressed_to_us(bot: &Bot, mention: Option<&str>) -> Result<bool> {
    let Some(mention) = mention else {
        return Ok(true);
    };
    let me = bot.get_me().await?;
    Ok(me
        .username
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case(mention)))
}

fn user_id(user: &teloxide::types::User) -> Result<i64> {
    Ok(i64::try_from(user.id.0)?)
}

pub async fn handle_message(bot: Bot, msg: Message, app: Arc<App>) -> Result<()> {
    let Some(from) = msg.from.as_ref() else {
        return Ok(());
    };
    let user = user_id(from)?;
    let _guard = app.locks.lock(user).await?;
    let command = command(msg.text());
    if !msg.chat.is_private() {
        // Python answered /admin anywhere; everything else in groups is ignored (SPEC Δ5).
        if let Some(("admin", mention)) = command
            && addressed_to_us(&bot, mention).await?
        {
            admin::command_outside_private(&bot, &app, &msg, user).await?;
        }
        return Ok(());
    }
    match command.map(|(name, _)| name) {
        Some("start") => user::start(&bot, &app, &msg, user).await,
        Some("admin") => admin::command(&bot, &app, &msg, user).await,
        _ => match app.db.load_dialogue(user).await? {
            Some(Dialogue::Verify { .. }) | None => Ok(()),
            Some(dialogue) => admin::on_message(&bot, &app, &msg, user, dialogue).await,
        },
    }
}

pub async fn handle_callback(bot: Bot, query: CallbackQuery, app: Arc<App>) -> Result<()> {
    let user = user_id(&query.from)?;
    let _guard = app.locks.lock(user).await?;
    let (Some(data), Some(message)) = (query.data.as_deref(), query.message.as_ref()) else {
        return bot
            .answer_callback_query(query.id)
            .await
            .map(drop)
            .map_err(Into::into);
    };
    let cb = Cb {
        bot: &bot,
        app: &app,
        user,
        chat: message.chat().id,
        message: message.id(),
        id: query.id.clone(),
        answered: AtomicBool::new(false),
    };
    let result = if let Some(digit) = data.strip_prefix("verify:") {
        user::verify(&cb, digit).await
    } else if !app.config.is_admin(user) {
        tracing::warn!(user, "non-admin blocked from admin callback");
        cb.alert(&t("admin.access_denied", &[])).await
    } else {
        admin::on_callback(&cb, data).await
    };
    if !cb.answered.load(Ordering::Relaxed) {
        let _ = bot.answer_callback_query(query.id).await;
    }
    result
}

/// A button press being handled.
pub struct Cb<'a> {
    pub bot: &'a Bot,
    pub app: &'a App,
    pub user: i64,
    pub chat: ChatId,
    pub message: MessageId,
    id: CallbackQueryId,
    answered: AtomicBool,
}

impl Cb<'_> {
    /// Replaces the pressed message's text; no markup removes the keyboard, as in aiogram.
    pub async fn edit(&self, text: &str, markup: Option<InlineKeyboardMarkup>) -> Result<()> {
        let mut request = self
            .bot
            .edit_message_text(self.chat, self.message, text)
            .parse_mode(ParseMode::Html);
        request.reply_markup = markup;
        request.await?;
        Ok(())
    }

    pub async fn send(&self, text: &str, markup: Option<InlineKeyboardMarkup>) -> Result<Message> {
        send(self.bot, self.chat, text, markup).await
    }

    pub async fn alert(&self, text: &str) -> Result<()> {
        self.answered.store(true, Ordering::Relaxed);
        self.bot
            .answer_callback_query(self.id.clone())
            .text(text)
            .show_alert(true)
            .await?;
        Ok(())
    }

    /// A small notification without an alert box (aiogram `callback.answer(text)`).
    pub async fn toast(&self, text: &str) -> Result<()> {
        self.answered.store(true, Ordering::Relaxed);
        self.bot
            .answer_callback_query(self.id.clone())
            .text(text)
            .await?;
        Ok(())
    }
}

pub async fn send(
    bot: &Bot,
    chat: ChatId,
    text: &str,
    markup: Option<InlineKeyboardMarkup>,
) -> Result<Message> {
    let mut request = bot.send_message(chat, text).parse_mode(ParseMode::Html);
    request.reply_markup = markup.map(Into::into);
    Ok(request.await?)
}
