//! Anti-bot check before joining: press the button with the shown digit.
use rand::Rng;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

/// Seconds a verification stays valid.
pub const TIMEOUT: i64 = 180;
/// Wrong answers allowed per giveaway before the user is blocked for it.
pub const MAX_ATTEMPTS: i64 = 3;

/// Five distinct digits 0–9 and the correct one among them.
pub fn numbers(rng: &mut impl Rng) -> (u8, Vec<u8>) {
    let numbers: Vec<u8> = rand::seq::index::sample(rng, 10, 5)
        .into_iter()
        .map(|d| d as u8)
        .collect();
    let correct = numbers[rng.random_range(0..numbers.len())];
    (correct, numbers)
}

/// Row of three and row of two, `verify:<digit>`.
pub fn keyboard(numbers: &[u8]) -> InlineKeyboardMarkup {
    let buttons: Vec<InlineKeyboardButton> = numbers
        .iter()
        .map(|n| InlineKeyboardButton::callback(n.to_string(), format!("verify:{n}")))
        .collect();
    InlineKeyboardMarkup::new(buttons.chunks(3).map(<[_]>::to_vec))
}
