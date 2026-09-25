-- Timestamps: TEXT, RFC 3339 UTC with microseconds (see src/time.rs).
-- The four data tables mirror the Python bot's Postgres schema; ids are preserved on import.
CREATE TABLE IF NOT EXISTS users (
    user_id INTEGER PRIMARY KEY,
    username TEXT,
    joined_at TEXT NOT NULL,
    is_suspicious INTEGER NOT NULL DEFAULT 0 CHECK (is_suspicious IN (0, 1))
);
CREATE INDEX IF NOT EXISTS ix_users_joined_at ON users(joined_at);

CREATE TABLE IF NOT EXISTS giveaways (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    start_at TEXT NOT NULL,
    end_at TEXT NOT NULL,
    description TEXT NOT NULL,
    num_winners INTEGER NOT NULL CHECK (num_winners > 0),
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    announce_text TEXT,
    announce_media_file_id TEXT NOT NULL,
    announce_media_type TEXT NOT NULL,
    created_by_admin_id INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    ended_at TEXT,
    CHECK (end_at > start_at)
);
-- Python only kept "one active giveaway" in code; here the database enforces it.
CREATE UNIQUE INDEX IF NOT EXISTS ix_giveaways_one_active ON giveaways(is_active) WHERE is_active = 1;

CREATE TABLE IF NOT EXISTS participants (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    giveaway_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    joined_at TEXT NOT NULL,
    username_snapshot TEXT,
    giveaway_end_snapshot TEXT NOT NULL,
    UNIQUE (giveaway_id, user_id)
);

CREATE TABLE IF NOT EXISTS winners (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    giveaway_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    username_snapshot TEXT,
    giveaway_end_snapshot TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (giveaway_id, user_id)
);

-- Wrong verification answers per giveaway; three or more means blocked for that giveaway.
CREATE TABLE IF NOT EXISTS verification_attempts (
    giveaway_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    attempts INTEGER NOT NULL,
    PRIMARY KEY (giveaway_id, user_id)
);

-- Wizard and verification state per user (replaces aiogram's Redis FSM).
CREATE TABLE IF NOT EXISTS dialogues (
    user_id INTEGER PRIMARY KEY,
    state TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Announcements, results and broadcasts. One worker runs them in id order; the cursor
-- and counters survive restarts. `in_flight` marks a send that may or may not have
-- reached Telegram: after a crash it is counted as failed and never repeated.
CREATE TABLE IF NOT EXISTS mailings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    content TEXT NOT NULL,
    to_channel INTEGER NOT NULL,
    audience TEXT NOT NULL,
    rps INTEGER NOT NULL CHECK (rps > 0),
    report_chat INTEGER NOT NULL,
    report_message INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'running', 'done')),
    channel_done INTEGER NOT NULL DEFAULT 0,
    channel_sent INTEGER NOT NULL DEFAULT 0,
    cursor INTEGER,
    in_flight INTEGER NOT NULL DEFAULT 0,
    total INTEGER NOT NULL,
    sent INTEGER NOT NULL DEFAULT 0,
    failed INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT
);

PRAGMA user_version = 1;
