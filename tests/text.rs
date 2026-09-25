use tg_bot_giveaway_and_broadcast::text::{KEYS, t};

#[test]
fn messages_json_is_byte_identical_to_python() {
    let digest = ring::digest::digest(&ring::digest::SHA256, include_bytes!("../messages.json"));
    let hex: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex,
        "7d223e9f01092a21c0998688762c0f886dc0155948f802ff8d120ae1e53bb102"
    );
}

#[test]
fn every_key_used_by_the_bot_exists() {
    for key in KEYS {
        assert!(!t(key, &[]).is_empty(), "{key}");
    }
}

#[test]
fn placeholders_are_substituted() {
    assert_eq!(
        t(
            "admin.broadcast_completed",
            &[("sent", &1), ("failed", &2), ("duration", &"3.0")]
        ),
        "✅ Рассылка завершена!\n\n✉️ Отправлено: 1\n❌ Не доставлено: 2\n⏱️ Длительность: 3.0с"
    );
    assert_eq!(
        t("user.verification_prompt", &[("number", &7)]),
        "🔐 Для завершения регистрации нажмите на кнопку <b>7</b>"
    );
}
