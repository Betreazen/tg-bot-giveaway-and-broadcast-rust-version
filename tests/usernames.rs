use tg_bot_giveaway_and_broadcast::usernames::parse_username;

#[test]
fn valid_usernames() {
    for (raw, expected) in [
        ("john_doe", "john_doe"),
        ("@john_doe", "john_doe"),
        ("  @John_Doe  ", "john_doe"),
        ("https://t.me/john_doe", "john_doe"),
        ("http://t.me/john_doe", "john_doe"),
        ("t.me/john_doe", "john_doe"),
        ("telegram.me/john_doe", "john_doe"),
        ("https://t.me/john_doe?start=x", "john_doe"),
        ("t.me/john_doe/", "john_doe"),
        ("@@john_doe", "john_doe"),
        ("JohnDoe123", "johndoe123"),
        ("t.me/john_doe#frag", "john_doe"),
    ] {
        assert_eq!(parse_username(raw).as_deref(), Some(expected), "{raw}");
    }
}

#[test]
fn invalid_usernames() {
    for raw in [
        "",
        "   ",
        "@",
        "ab",
        "has spaces",
        "bad-char!",
        "with.dot",
        "https://example.com/",
        "a_name_that_is_way_too_long_for_telegram",
        "имя_пользователя",
    ] {
        assert_eq!(parse_username(raw), None, "{raw}");
    }
}
