//! Admin panel. Every entry point here runs only for `ADMIN_IDS` (checked by the caller
//! for callbacks, here for messages), like the Python bot's AdminOnlyMiddleware.
use super::{App, Cb, send};
use crate::{state::Dialogue, text::t};
use anyhow::Result;
use teloxide::prelude::*;

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

pub async fn command(_bot: &Bot, _app: &App, _msg: &Message, _user: i64) -> Result<()> {
    Ok(())
}

pub async fn on_message(
    _bot: &Bot,
    _app: &App,
    _msg: &Message,
    _user: i64,
    _dialogue: Dialogue,
) -> Result<()> {
    Ok(())
}

pub async fn on_callback(_cb: &Cb<'_>, _data: &str) -> Result<()> {
    Ok(())
}
