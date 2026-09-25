//! `/start` participation flow and the verification buttons (Python start.py, verification.py).
use super::{App, Cb, send};
use crate::{
    db::Giveaway,
    state::{Dialogue, Verification},
    text::t,
    time::{from_db, to_db},
    verification::{self, MAX_ATTEMPTS, TIMEOUT},
};
use anyhow::Result;
use rand::seq::SliceRandom;
use teloxide::{prelude::*, types::ChatMemberKind};

pub async fn start(bot: &Bot, app: &App, msg: &Message, user: i64) -> Result<()> {
    let username = msg.from.as_ref().and_then(|u| u.username.as_deref());
    if let Err(error) = try_start(bot, app, msg.chat.id, user, username).await {
        tracing::error!(user, error = %app.redact(&error), "start failed");
        send(bot, msg.chat.id, &t("errors.generic", &[]), None).await?;
    }
    Ok(())
}

async fn try_start(
    bot: &Bot,
    app: &App,
    chat: ChatId,
    user: i64,
    username: Option<&str>,
) -> Result<()> {
    app.db.upsert_user(user, username).await?;
    let reply = async |key: &str| send(bot, chat, &t(key, &[]), None).await;
    if !subscribed(bot, app, user).await {
        reply("user.not_subscribed").await?;
        return Ok(());
    }
    let Some(giveaway) = app.db.active_giveaway().await? else {
        reply("user.no_active_giveaway").await?;
        return Ok(());
    };
    if app.db.is_participant(giveaway.id, user).await? {
        reply("user.already_participating").await?;
        return Ok(());
    }
    if app.config.is_admin(user) {
        app.db
            .add_participant(giveaway.id, user, username, giveaway.end_at)
            .await?;
        send(
            bot,
            chat,
            &confirmed(&giveaway.description, giveaway.num_winners),
            None,
        )
        .await?;
        return Ok(());
    }
    let Some(username) = username else {
        reply("user.no_username").await?;
        return Ok(());
    };
    if app.db.attempts(giveaway.id, user).await? >= MAX_ATTEMPTS {
        reply("user.verification_blocked").await?;
        return Ok(());
    }
    if let Some(Dialogue::Verify(Verification { created_at, .. })) =
        app.db.load_dialogue(user).await?
        && unix_now() - created_at <= TIMEOUT
    {
        reply("user.verification_in_progress").await?;
        return Ok(());
    }
    begin_verification(bot, app, chat, user, username, &giveaway).await
}

async fn begin_verification(
    bot: &Bot,
    app: &App,
    chat: ChatId,
    user: i64,
    username: &str,
    giveaway: &Giveaway,
) -> Result<()> {
    let (correct, numbers) = verification::numbers(&mut rand::rng());
    let keyboard = verification::keyboard(&numbers);
    let state = Verification {
        correct,
        numbers,
        created_at: unix_now(),
        giveaway_id: giveaway.id,
        username: username.to_owned(),
        end_at: to_db(giveaway.end_at),
        description: giveaway.description.clone(),
        num_winners: giveaway.num_winners,
    };
    app.db.save_dialogue(user, &Dialogue::Verify(state)).await?;
    let prompt = t("user.verification_prompt", &[("number", &correct)]);
    send(bot, chat, &prompt, Some(keyboard)).await?;
    Ok(())
}

/// Python: creator, administrator or member; any API error counts as not subscribed.
async fn subscribed(bot: &Bot, app: &App, user: i64) -> bool {
    let member = bot
        .get_chat_member(ChatId(app.config.channel_id), UserId(user as u64))
        .await;
    match member {
        Ok(member) => matches!(
            member.kind,
            ChatMemberKind::Owner(_) | ChatMemberKind::Administrator(_) | ChatMemberKind::Member(_)
        ),
        Err(error) => {
            let message = crate::network::redact(&error.to_string(), &app.config.token);
            tracing::warn!(user, error = %message, "subscription check failed");
            false
        }
    }
}

fn confirmed(description: &str, num_winners: i64) -> String {
    t(
        "user.participation_confirmed",
        &[("description", &description), ("num_winners", &num_winners)],
    )
}

fn unix_now() -> i64 {
    crate::time::now().timestamp()
}

pub async fn verify(cb: &Cb<'_>, digit: &str) -> Result<()> {
    let (Ok(pressed), Some(Dialogue::Verify(state))) =
        (digit.parse::<u8>(), cb.app.db.load_dialogue(cb.user).await?)
    else {
        return Ok(());
    };
    if unix_now() - state.created_at > TIMEOUT {
        cb.app.db.clear_dialogue(cb.user).await?;
        return cb.alert(&t("user.verification_timeout", &[])).await;
    }
    if pressed == state.correct {
        cb.app.db.clear_dialogue(cb.user).await?;
        return match join(cb, &state).await {
            Ok(()) => {
                cb.edit(&confirmed(&state.description, state.num_winners), None)
                    .await
            }
            Err(error) => {
                tracing::error!(user = cb.user, error = %cb.app.redact(&error), "join failed");
                cb.edit(&t("errors.generic", &[]), None).await
            }
        };
    }
    wrong_answer(cb, state).await
}

async fn join(cb: &Cb<'_>, state: &Verification) -> Result<()> {
    let end = from_db(&state.end_at)?;
    cb.app
        .db
        .add_participant(state.giveaway_id, cb.user, Some(&state.username), end)
        .await
}

async fn wrong_answer(cb: &Cb<'_>, mut state: Verification) -> Result<()> {
    let attempts = cb.app.db.add_attempt(state.giveaway_id, cb.user).await?;
    if attempts >= MAX_ATTEMPTS {
        cb.app.db.clear_dialogue(cb.user).await?;
        tracing::info!(
            user = cb.user,
            giveaway = state.giveaway_id,
            "blocked after failed verification"
        );
        return cb.edit(&t("user.verification_blocked", &[]), None).await;
    }
    state.numbers.shuffle(&mut rand::rng());
    let keyboard = verification::keyboard(&state.numbers);
    let text = t(
        "user.verification_wrong",
        &[
            ("remaining", &(MAX_ATTEMPTS - attempts)),
            ("number", &state.correct),
        ],
    );
    cb.app
        .db
        .save_dialogue(cb.user, &Dialogue::Verify(state))
        .await?;
    cb.edit(&text, Some(keyboard)).await
}
