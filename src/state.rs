//! Per-user dialogue state, stored as JSON in `dialogues` (Python kept it in Redis).
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Media {
    /// `photo`, `video`, `animation` or `document`, as in the Python bot.
    pub kind: String,
    pub file_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state")]
pub enum Dialogue {
    /// Waiting for the correct digit button; the snapshot mirrors Python's FSM data.
    Verify {
        correct: u8,
        numbers: Vec<u8>,
        created_at: i64,
        giveaway_id: i64,
        username: String,
        end_at: String,
        description: String,
        num_winners: i64,
    },
    Giveaway {
        step: GwStep,
        draft: GwDraft,
    },
    Broadcast {
        step: BcStep,
        draft: BcDraft,
    },
    Winners {
        step: WinStep,
        giveaway_id: i64,
    },
    Suspicious {
        mark: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GwStep {
    StartTime,
    Duration,
    Description,
    WinnerCount,
    Media,
    Preview,
    AnnounceTarget,
}

/// Wizard answers collected so far; "Назад" keeps them, like Python's FSM data.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GwDraft {
    pub start_option: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub description: Option<String>,
    pub num_winners: Option<i64>,
    pub media: Option<Media>,
    pub giveaway_id: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BcStep {
    Type,
    Text,
    Media,
    Confirm,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BcDraft {
    pub media_mode: bool,
    pub text: Option<String>,
    pub media: Option<Media>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WinStep {
    ConfirmEnd,
    Select,
    Publish,
}
