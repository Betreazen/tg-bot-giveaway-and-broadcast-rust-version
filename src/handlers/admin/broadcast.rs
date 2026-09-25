//! Broadcast wizard (Python broadcast_wizard.py).
use super::{App, Cb, Input, enqueue, keyboards, media_of};
use crate::{
    db::{Audience, Content, MailingKind},
    state::{BcDraft, BcStep, Dialogue},
    text::t,
};
use anyhow::Result;

const TYPE: &str = "📢 <b>Рассылка сообщений</b>\n\nВыберите тип рассылки:";
const TEXT: &str = "✏️ <b>Текстовая рассылка</b>\n\nВведите текст сообщения:";
const MEDIA: &str = "📎 <b>Рассылка с медиа</b>\n\nОтправьте фото, видео, GIF или документ с подписью (необязательно):";
const MAX_TEXT: usize = 4096;

async fn save(app: &App, user: i64, step: BcStep, draft: BcDraft) -> Result<()> {
    let dialogue = Dialogue::Broadcast { step, draft };
    app.db.save_dialogue(user, &dialogue).await
}

fn nav() -> Option<teloxide::types::InlineKeyboardMarkup> {
    Some(keyboards::navigation(true, true, true))
}

pub async fn start(cb: &Cb<'_>) -> Result<()> {
    cb.edit(TYPE, Some(keyboards::broadcast_type())).await?;
    save(cb.app, cb.user, BcStep::Type, BcDraft::default()).await
}

pub async fn on_callback(
    cb: &Cb<'_>,
    data: (&str, &str),
    step: BcStep,
    mut draft: BcDraft,
) -> Result<()> {
    match (data, step) {
        (("broadcast", kind @ ("text" | "media")), BcStep::Type) => {
            draft.media_mode = kind == "media";
            let (text, next) = if draft.media_mode {
                (MEDIA, BcStep::Media)
            } else {
                (TEXT, BcStep::Text)
            };
            cb.edit(text, nav()).await?;
            save(cb.app, cb.user, next, draft).await
        }
        // Dead button in Python; here it returns to the type choice (SPEC Δ7).
        (("nav", "back"), BcStep::Text | BcStep::Media) => start(cb).await,
        (("preview", "edit"), BcStep::Confirm) => {
            let (text, next) = if draft.media_mode {
                ("📎 Отправьте новое медиа с подписью:", BcStep::Media)
            } else {
                ("✏️ Введите новый текст:", BcStep::Text)
            };
            cb.edit(text, nav()).await?;
            save(cb.app, cb.user, next, draft).await
        }
        (("preview", "confirm"), BcStep::Confirm) => confirm(cb, draft).await,
        _ => Ok(()),
    }
}

pub async fn on_message(input: &Input<'_>, step: BcStep, mut draft: BcDraft) -> Result<()> {
    let preview = match step {
        BcStep::Text => {
            let Some(text) = input.msg.text() else {
                return Ok(());
            };
            let length = text.chars().count();
            if length > MAX_TEXT {
                let too_long = "❌ Текст слишком длинный (максимум 4096 символов)";
                return input.reply(too_long, None).await;
            }
            draft.text = Some(text.to_owned());
            format!(
                "👁️ <b>Предпросмотр рассылки</b>\n\n{text}\n\n📏 Символов: {length}\n\nПодтвердить отправку?"
            )
        }
        BcStep::Media => {
            let Some(media) = media_of(input.msg) else {
                return input.reply(&t("wizard.invalid_media", &[]), None).await;
            };
            let caption = input.msg.caption().unwrap_or_default();
            let shown = if caption.is_empty() {
                "(нет)"
            } else {
                caption
            };
            let text = format!(
                "👁️ <b>Предпросмотр рассылки</b>\n\n📎 Медиа: {}\n📝 Подпись: {shown}\n📏 Символов: {}\n\nПодтвердить отправку?",
                media.kind,
                caption.chars().count()
            );
            draft.text = Some(caption.to_owned());
            draft.media = Some(media);
            text
        }
        BcStep::Type | BcStep::Confirm => return Ok(()),
    };
    input.reply(&preview, Some(keyboards::preview())).await?;
    save(input.app, input.user, BcStep::Confirm, draft).await
}

async fn confirm(cb: &Cb<'_>, draft: BcDraft) -> Result<()> {
    cb.app.db.clear_dialogue(cb.user).await?;
    cb.edit("📤 Начинаю рассылку...", None).await?;
    if cb.app.db.user_count().await? == 0 {
        return cb
            .edit("❌ В базе нет пользователей для рассылки", None)
            .await;
    }
    let content = Content {
        // Python sent `caption or None`: an empty caption is no caption.
        text: draft.text.filter(|t| !t.is_empty()),
        media: draft.media,
        join_button: false,
    };
    let rps = cb.app.config.broadcast_rps;
    let queued = enqueue(
        cb,
        MailingKind::Broadcast,
        content,
        false,
        Audience::Users,
        rps,
    );
    if let Err(error) = queued.await {
        tracing::error!(error = %cb.app.redact(&error), "broadcast failed");
        return cb.edit("❌ Ошибка при рассылке", None).await;
    }
    Ok(())
}
