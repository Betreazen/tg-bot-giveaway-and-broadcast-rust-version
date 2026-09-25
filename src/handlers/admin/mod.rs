//! Admin panel. Callbacks reach this module only for `ADMIN_IDS` (checked by the caller);
//! messages are checked here — together the Python bot's AdminOnlyMiddleware.
mod announce;
mod broadcast;
mod giveaway;
mod keyboards;
mod menu;
mod suspicious;
mod winners;

use super::{App, Cb, send};
use crate::{
    db::{Audience, Content, MailingKind, NewMailing},
    state::{Dialogue, Media},
    text::t,
};
use anyhow::Result;
use teloxide::{prelude::*, types::InlineKeyboardMarkup};

/// `/admin` outside a private chat: non-admins are refused, admins are asked to use a DM.
pub async fn command_outside_private(bot: &Bot, app: &App, msg: &Message, user: i64) -> Result<()> {
    let key = if app.config.is_admin(user) {
        "admin.use_private_chat"
    } else {
        "admin.access_denied"
    };
    send(bot, msg.chat.id, &t(key, &[]), None).await?;
    Ok(())
}

pub async fn command(bot: &Bot, app: &App, msg: &Message, user: i64) -> Result<()> {
    if !app.config.is_admin(user) {
        tracing::warn!(user, "non-admin tried /admin");
        send(bot, msg.chat.id, &t("admin.access_denied", &[]), None).await?;
        return Ok(());
    }
    menu::show(bot, app, msg.chat.id).await
}

/// A message sent while an admin wizard waits for input.
pub struct Input<'a> {
    pub bot: &'a Bot,
    pub app: &'a App,
    pub msg: &'a Message,
    pub user: i64,
}

impl Input<'_> {
    pub async fn reply(&self, text: &str, markup: Option<InlineKeyboardMarkup>) -> Result<()> {
        send(self.bot, self.msg.chat.id, text, markup).await?;
        Ok(())
    }
}

pub async fn on_message(
    bot: &Bot,
    app: &App,
    msg: &Message,
    user: i64,
    dialogue: Dialogue,
) -> Result<()> {
    if !app.config.is_admin(user) {
        tracing::warn!(user, "non-admin blocked from admin wizard");
        send(bot, msg.chat.id, &t("admin.access_denied", &[]), None).await?;
        return Ok(());
    }
    let input = Input {
        bot,
        app,
        msg,
        user,
    };
    match dialogue {
        Dialogue::Giveaway { step, draft } => giveaway::on_message(&input, step, draft).await,
        Dialogue::Broadcast { step, draft } => broadcast::on_message(&input, step, draft).await,
        Dialogue::Suspicious { mark } => suspicious::on_message(&input, mark).await,
        Dialogue::Winners { .. } | Dialogue::Verify(_) => Ok(()),
    }
}

pub async fn on_callback(cb: &Cb<'_>, data: &str) -> Result<()> {
    let (prefix, arg) = data.split_once(':').unwrap_or((data, ""));
    match (prefix, arg) {
        ("admin", "close") => menu::close(cb).await,
        ("admin", "status") => menu::status(cb).await,
        ("admin", "sync_sheets") => menu::sync_sheets(cb).await,
        ("admin", "create_giveaway") => giveaway::start(cb).await,
        ("admin", "announce_giveaway") => announce::prompt(cb).await,
        ("admin", "complete_giveaway") => winners::start(cb).await,
        ("admin", "broadcast") => broadcast::start(cb).await,
        ("admin", "suspicious") => suspicious::menu(cb).await,
        ("nav", "main_menu") => menu::main_menu(cb).await,
        ("nav", "cancel") => menu::cancel(cb).await,
        ("announce_manual", target) => announce::send(cb, target).await,
        ("suspicious", action) => suspicious::on_callback(cb, action).await,
        _ => match cb.app.db.load_dialogue(cb.user).await? {
            Some(Dialogue::Giveaway { step, draft }) => {
                giveaway::on_callback(cb, (prefix, arg), step, draft).await
            }
            Some(Dialogue::Broadcast { step, draft }) => {
                broadcast::on_callback(cb, (prefix, arg), step, draft).await
            }
            Some(Dialogue::Winners { step, giveaway_id }) => {
                winners::on_callback(cb, (prefix, arg), step, giveaway_id).await
            }
            Some(Dialogue::Suspicious { .. }) if data == "nav:back" => suspicious::back(cb).await,
            _ => Ok(()),
        },
    }
}

/// Queues a mailing whose final report replaces the pressed message.
async fn enqueue(
    cb: &Cb<'_>,
    kind: MailingKind,
    content: Content,
    to_channel: bool,
    audience: Audience,
    rps: u32,
) -> Result<()> {
    let mailing = NewMailing {
        kind,
        content,
        to_channel,
        audience,
        rps,
        report_chat: cb.chat.0,
        report_message: cb.message.0,
    };
    cb.app.db.enqueue_mailing(&mailing).await?;
    cb.app.mailings.notify_one();
    Ok(())
}

/// Photo (largest size), video, GIF or document — the Python bot's order of checks.
fn media_of(msg: &Message) -> Option<Media> {
    let (kind, file) = if let Some(sizes) = msg.photo() {
        ("photo", &sizes.last()?.file)
    } else if let Some(video) = msg.video() {
        ("video", &video.file)
    } else if let Some(animation) = msg.animation() {
        ("animation", &animation.file)
    } else {
        ("document", &msg.document()?.file)
    };
    Some(Media {
        kind: kind.into(),
        file_id: file.id.0.clone(),
    })
}
