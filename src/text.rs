//! Interface texts. `messages.json` is a byte-for-byte copy of the Python bot's
//! catalogue; strings the Python handlers hard-coded live next to their handlers.
use serde_json::Value;
use std::{fmt::Display, sync::OnceLock};

/// Catalogue keys the bot uses; a test checks that each one exists.
pub const KEYS: &[&str] = &[
    "user.not_subscribed",
    "user.no_active_giveaway",
    "user.already_participating",
    "user.no_username",
    "user.participation_confirmed",
    "user.verification_prompt",
    "user.verification_wrong",
    "user.verification_blocked",
    "user.verification_timeout",
    "user.verification_in_progress",
    "admin.access_denied",
    "admin.use_private_chat",
    "admin.main_menu",
    "admin.no_participants",
    "admin.broadcast_completed",
    "admin.operation_cancelled",
    "admin.status_active",
    "admin.status_no_active",
    "wizard.invalid_winner_count",
    "wizard.description_too_long",
    "wizard.invalid_media",
    "buttons.create_giveaway",
    "buttons.announce_giveaway",
    "buttons.complete_giveaway",
    "buttons.broadcast",
    "buttons.view_status",
    "buttons.close",
    "buttons.back",
    "buttons.cancel",
    "buttons.main_menu",
    "buttons.edit",
    "buttons.to_channel",
    "buttons.to_users",
    "buttons.everywhere",
    "buttons.skip",
    "buttons.select_winners",
    "buttons.to_admins",
    "buttons.text_only",
    "buttons.media_caption",
    "buttons.confirm_send",
    "buttons.yes_end_now",
    "buttons.no_continue",
    "errors.generic",
    "errors.database",
];

fn catalogue() -> &'static Value {
    static CATALOGUE: OnceLock<Value> = OnceLock::new();
    CATALOGUE.get_or_init(|| {
        serde_json::from_str(include_str!("../messages.json")).expect("messages.json is valid")
    })
}

/// Python's `t(key, **kwargs)`: dotted lookup plus `{name}` substitution.
/// One pass over the template, like `str.format`: inserted values are never expanded.
pub fn t(key: &str, args: &[(&str, &dyn Display)]) -> String {
    let template = key
        .split('.')
        .try_fold(catalogue(), |node, part| node.get(part))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing message key {key}"));
    let mut text = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        text.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let value = after
            .find('}')
            .and_then(|close| Some((close, args.iter().find(|(n, _)| *n == &after[..close])?)));
        match value {
            Some((close, (_, value))) => {
                text.push_str(&value.to_string());
                rest = &after[close + 1..];
            }
            None => {
                text.push('{');
                rest = after;
            }
        }
    }
    text.push_str(rest);
    text
}

/// Header plus lines split into messages of at most `limit` characters (Python `_paginate`).
pub fn paginate(header: &str, lines: &[String], limit: usize) -> Vec<String> {
    let mut pages = Vec::new();
    let mut current = header.to_owned();
    for line in lines {
        if current.chars().count() + line.chars().count() + 1 > limit {
            pages.push(current.trim_end().to_owned());
            current.clear();
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        pages.push(current.trim_end().to_owned());
    }
    if pages.is_empty() {
        pages.push(header.trim_end().to_owned());
    }
    pages
}
