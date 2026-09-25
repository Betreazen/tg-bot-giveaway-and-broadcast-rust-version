//! Admin inline keyboards, as in the Python bot's keyboards/admin.py, common.py, date_picker.py.
use crate::text::t;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

fn button(text: &str, data: &str) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(text, data)
}

/// Button text from the message catalogue.
fn b(key: &str, data: &str) -> InlineKeyboardButton {
    button(&t(key, &[]), data)
}

fn column(buttons: Vec<InlineKeyboardButton>) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(buttons.into_iter().map(|b| vec![b]))
}

pub fn main_menu(has_active_giveaway: bool) -> InlineKeyboardMarkup {
    let mut rows = vec![b("buttons.create_giveaway", "admin:create_giveaway")];
    if has_active_giveaway {
        rows.push(b("buttons.announce_giveaway", "admin:announce_giveaway"));
        rows.push(b("buttons.complete_giveaway", "admin:complete_giveaway"));
    }
    rows.extend([
        b("buttons.broadcast", "admin:broadcast"),
        b("buttons.view_status", "admin:status"),
        button("🚩 Подозрительные аккаунты", "admin:suspicious"),
        button("📊 Синхронизация Google Sheets", "admin:sync_sheets"),
        b("buttons.close", "admin:close"),
    ]);
    column(rows)
}

/// Back / cancel / main menu, two per row.
pub fn navigation(back: bool, cancel: bool, main_menu: bool) -> InlineKeyboardMarkup {
    let mut buttons = Vec::new();
    if back {
        buttons.push(b("buttons.back", "nav:back"));
    }
    if cancel {
        buttons.push(b("buttons.cancel", "nav:cancel"));
    }
    if main_menu {
        buttons.push(b("buttons.main_menu", "nav:main_menu"));
    }
    InlineKeyboardMarkup::new(buttons.chunks(2).map(<[_]>::to_vec))
}

fn nav_row() -> Vec<InlineKeyboardButton> {
    vec![
        button("⬅️ Назад", "nav:back"),
        button("❌ Отменить", "nav:cancel"),
    ]
}

pub fn start_time() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([
        vec![
            button("🕐 Сейчас", "start_time:now"),
            button("🕐 Через 1 час", "start_time:1h"),
        ],
        vec![
            button("🕐 Через 3 часа", "start_time:3h"),
            button("🕐 Через 6 часов", "start_time:6h"),
        ],
        vec![button("🕐 Завтра в 12:00", "start_time:tomorrow")],
        nav_row(),
    ])
}

pub fn duration() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([
        vec![
            button("📅 1 день", "duration:1"),
            button("📅 3 дня", "duration:3"),
        ],
        vec![
            button("📅 7 дней", "duration:7"),
            button("📅 14 дней", "duration:14"),
        ],
        vec![button("📅 30 дней", "duration:30")],
        nav_row(),
    ])
}

pub fn preview() -> InlineKeyboardMarkup {
    column(vec![
        b("buttons.confirm_send", "preview:confirm"),
        b("buttons.edit", "preview:edit"),
        b("buttons.cancel", "nav:cancel"),
    ])
}

/// Announcement targets; `prefix` is `announce` (after creation, with skip) or `announce_manual`.
pub fn announce_target(prefix: &str, with_skip: bool) -> InlineKeyboardMarkup {
    let mut rows = vec![
        b("buttons.to_channel", &format!("{prefix}:channel")),
        b("buttons.to_users", &format!("{prefix}:users")),
        b("buttons.everywhere", &format!("{prefix}:everywhere")),
    ];
    if with_skip {
        rows.push(b("buttons.skip", &format!("{prefix}:skip")));
    }
    rows.push(b("buttons.cancel", "nav:cancel"));
    column(rows)
}

pub fn results_target() -> InlineKeyboardMarkup {
    column(vec![
        b("buttons.to_channel", "results:channel"),
        b("buttons.to_admins", "results:admins"),
        b("buttons.to_users", "results:users"),
        b("buttons.everywhere", "results:everywhere"),
        b("buttons.cancel", "nav:cancel"),
    ])
}

pub fn end_confirm() -> InlineKeyboardMarkup {
    column(vec![
        b("buttons.yes_end_now", "giveaway:end_confirm"),
        b("buttons.no_continue", "giveaway:end_cancel"),
    ])
}

pub fn select_winners() -> InlineKeyboardMarkup {
    column(vec![
        b("buttons.select_winners", "winners:select"),
        b("buttons.back", "nav:back"),
    ])
}

pub fn broadcast_type() -> InlineKeyboardMarkup {
    column(vec![
        b("buttons.text_only", "broadcast:text"),
        b("buttons.media_caption", "broadcast:media"),
        b("buttons.cancel", "nav:cancel"),
    ])
}

pub fn suspicious_menu() -> InlineKeyboardMarkup {
    column(vec![
        button("🚩 Пометить подозрительным", "suspicious:mark"),
        button("✅ Снять метку", "suspicious:unmark"),
        button("📋 Список подозрительных", "suspicious:list"),
        b("buttons.main_menu", "nav:main_menu"),
    ])
}
