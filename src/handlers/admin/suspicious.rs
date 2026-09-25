//! Suspicious accounts: they take part but never win; users are not told (Python suspicious.py).
use super::{Cb, Input, keyboards};
use crate::{state::Dialogue, text::paginate, usernames::parse_username};
use anyhow::Result;

const MENU: &str = "🚩 <b>Подозрительные аккаунты</b>\n\nПомеченные аккаунты участвуют в розыгрышах, но <b>никогда не выигрывают</b>. Пользователь об этом не узнаёт.\n\nВыберите действие:";
const ENTER: &str = "Введите username пользователя в любом виде:\n<code>@username</code>, <code>https://t.me/username</code> или просто <code>username</code>.";
const PAGE_LIMIT: usize = 3800;

pub async fn menu(cb: &Cb<'_>) -> Result<()> {
    cb.app.db.clear_dialogue(cb.user).await?;
    cb.edit(MENU, Some(keyboards::suspicious_menu())).await
}

pub async fn on_callback(cb: &Cb<'_>, action: &str) -> Result<()> {
    let (heading, mark) = match action {
        "list" => return list(cb).await,
        "mark" => ("🚩 <b>Пометить подозрительным</b>", true),
        "unmark" => ("✅ <b>Снять метку подозрительного</b>", false),
        _ => return Ok(()),
    };
    let nav = keyboards::navigation(true, true, true);
    cb.edit(&format!("{heading}\n\n{ENTER}"), Some(nav)).await?;
    cb.app
        .db
        .save_dialogue(cb.user, &Dialogue::Suspicious { mark })
        .await
}

/// "Назад" while typing a username (a dead button in Python) returns to this menu.
pub async fn back(cb: &Cb<'_>) -> Result<()> {
    menu(cb).await
}

async fn list(cb: &Cb<'_>) -> Result<()> {
    cb.app.db.clear_dialogue(cb.user).await?;
    let users = cb.app.db.suspicious_users().await?;
    if users.is_empty() {
        let markup = Some(keyboards::suspicious_menu());
        return cb.edit("📋 Список подозрительных пуст.", markup).await;
    }
    let lines: Vec<String> = users
        .iter()
        .enumerate()
        .map(|(i, (id, name))| {
            let name = name
                .as_deref()
                .filter(|n| !n.is_empty())
                .map_or("без username".into(), |n| format!("@{n}"));
            format!("{}. {name} (ID <code>{id}</code>)", i + 1)
        })
        .collect();
    let header = format!("📋 <b>Подозрительные аккаунты ({})</b>\n\n", users.len());
    let pages = paginate(&header, &lines, PAGE_LIMIT);
    cb.edit(&pages[0], Some(keyboards::suspicious_menu()))
        .await?;
    for page in &pages[1..] {
        cb.send(page, None).await?;
    }
    Ok(())
}

pub async fn on_message(input: &Input<'_>, mark: bool) -> Result<()> {
    let Some(text) = input.msg.text() else {
        return Ok(());
    };
    input.app.db.clear_dialogue(input.user).await?;
    let menu = || Some(keyboards::suspicious_menu());
    let Some(username) = parse_username(text) else {
        let error = "❌ Не удалось распознать username. Пришлите его в виде <code>@username</code>, ссылки или просто <code>username</code>.";
        return input.reply(error, menu()).await;
    };
    let reply = match input.app.db.set_suspicious(&username, mark).await? {
        None => format!(
            "⚠️ Пользователь <code>@{username}</code> не найден в базе (он должен был хотя бы раз запустить бота)."
        ),
        Some(id) if mark => {
            tracing::info!(user = id, "admin marked user as suspicious");
            format!(
                "🚩 <code>@{username}</code> (ID <code>{id}</code>) помечен как подозрительный. Участвует, но не выигрывает."
            )
        }
        Some(id) => {
            tracing::info!(user = id, "admin cleared suspicious flag");
            format!(
                "✅ С <code>@{username}</code> (ID <code>{id}</code>) снята метка «подозрительный»."
            )
        }
    };
    input.reply(&reply, menu()).await
}
