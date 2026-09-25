# Паритет с Python-версией

Исходник: [tg-bot-giveaway-and-broadcast](https://github.com/Betreazen/tg-bot-giveaway-and-broadcast),
коммит `9a4c9c6` (версия, работавшая в production). Ссылки ниже — `файл::функция` в том
репозитории; Rust-тесты — `tests/<файл>.rs::<тест>`.

**=** — поведение то же самое; **≈** — то же для пользователя, но устроено иначе;
**Δ** — осознанное отличие (таблица в конце и [SPEC.md](SPEC.md) §10).

## 1. Участник: `/start` и верификация

| # | Поведение | Python | Py-тест | | Rust-тест |
|---|---|---|---|---|---|
| U1 | `/start` (с любым payload) сохраняет пользователя; username перезаписывается только новым не-NULL значением | `handlers/start.py::start_handler`, `db/repo/user_repo.py::create_or_update_user` | — | = | `user_flow::u1_*` |
| U2 | Не подписан на `CHANNEL_ID` (статус не creator/administrator/member или ошибка API) → `user.not_subscribed` | `services/subscription.py::check_subscription` | — | = | `user_flow::u2_*` |
| U3 | Нет активного розыгрыша → `user.no_active_giveaway` | `start_handler` | — | = | `user_flow::u3_*` |
| U4 | Уже участник → `user.already_participating` | `start_handler` | — | = | `user_flow::u4_*` |
| U5 | Админ участвует сразу, без верификации и username | `start_handler` | — | = | `user_flow::u5_*` |
| U6 | Нет `@username` → `user.no_username` | `start_handler` | — | = | `user_flow::u6_*` |
| U7 | Заблокирован для этого розыгрыша → `user.verification_blocked` | `start_handler` | — | ≈ | `user_flow::u7_u9_*` |
| U8 | Верификация идёт (< 180 с) → `user.verification_in_progress`; истекла → заново | `start_handler` | — | = | `user_flow::u8_u10_*` |
| U9 | Попыток ≥ 3 → блок | `start_handler` | — | = | `user_flow::u7_u9_*` |
| U10 | 5 разных цифр 0–9, одна верная, клавиатура 3+2, `verify:<n>` | `handlers/verification.py::generate_verification_*` | `test_verification.py` | = | `user_flow::u8_u10_*` |
| U11 | Верная кнопка → участие (idempotent), сообщение → `user.participation_confirmed` | `verification.py::verification_callback` | — | = | `user_flow::u11_*` |
| U12 | Неверная → попытка +1 (per розыгрыш+пользователь), кнопки перемешаны, `user.verification_wrong` | `verification_callback` | — | = | `user_flow::u12_u13_*` |
| U13 | 3-я неверная → блок на этот розыгрыш; счётчик переживает рестарт | `verification_callback` | — | = (Δ14) | `user_flow::u12_u13_*` |
| U14 | Нажатие после 180 с → alert `user.verification_timeout` | `verification_callback` | — | = | `user_flow::u14_*` |
| U15 | `verify:` без активной верификации | фильтр состояния | — | ≈ (Δ6) | `user_flow::u15_*` |
| U16 | Внутренняя ошибка → `errors.generic` | `start_handler` | — | = | `user_flow::u16_*` |
| U17 | Прочие сообщения — без ответа; группы (кроме `/admin`) | aiogram | — | = / Δ5 | `user_flow::u17_*` |

## 2. Админ: доступ и меню

| # | Поведение | Python | Py-тест | | Rust-тест |
|---|---|---|---|---|---|
| A1 | Админский хендлер от не-админа → `admin.access_denied` (у callback — alert) | `middlewares/admin.py` | `test_admin_middleware.py` | = (Δ15) | `admin_giveaway::a1_*` |
| A2 | `/admin` не в личке → `admin.use_private_chat`; `/admin@ДругойБот` — не нам | `handlers/admin/entry.py::admin_command_handler` | — | = | `admin_giveaway::a2_*` |
| A3 | Меню: «Создать», [«Объявить», «Завершить» при активном], «Рассылка», «Статус», «Подозрительные», «Sheets», «Закрыть» | `keyboards/admin.py::get_admin_main_menu` | — | = | `admin_giveaway::a3_*` |
| A4 | «Закрыть» удаляет сообщение | `entry.py::close_admin_panel` | — | = | `admin_giveaway::a4_*` |
| A5 | «Статус»: alert или новое сообщение `admin.status_active` | `menu.py::show_status` | — | = | `admin_giveaway::a5_*` |
| A6 | «Главное меню» сбрасывает состояние | `giveaway_wizard.py::return_to_main_menu` | — | = | `admin_giveaway::a6_a7_*` |
| A7 | «Отменить» → `admin.operation_cancelled` | `giveaway_wizard.py::cancel_wizard` | — | = | `admin_giveaway::a6_a7_*` |
| A8 | Хранение UTC, показ МСК (UTC+3) | `utils/datetimes.py` | `test_datetimes.py` | = | `time::*` |

## 3. Мастер создания розыгрыша

| # | Поведение | Python | Py-тест | | Rust-тест |
|---|---|---|---|---|---|
| G1 | Старт: Сейчас / +1 ч / +3 ч / +6 ч / Завтра 12:00 МСК | `date_picker.py::get_start_time_keyboard` | — | = | `admin_giveaway::g1_to_g8_*` |
| G2 | Длительность 1/3/7/14/30 дней, даты в МСК → UTC | `date_picker.py::calculate_dates` | — | = | `time::start_options_*`, `g1_to_g8_*` |
| G3 | Описание > 4096 символов → `wizard.description_too_long` | `giveaway_wizard.py::process_description` | — | = | `g1_to_g8_*` |
| G4 | Победителей — целое ≥ 1 | `process_winner_count` | — | = | `g1_to_g8_*` |
| G5 | Медиа: фото (наибольшее), видео, GIF, документ | `process_media` | — | = | `g1_to_g8_*`, `g5_*` |
| G6 | Предпросмотр с датами, победителями, описанием, типом медиа | `process_media`, `format_dates_display` | — | = | `g1_to_g8_*`, `time::dates_display_*` |
| G7 | «Редактировать» → описание без «Назад» | `edit_giveaway` | — | = | `g7_*` |
| G8 | «Подтвердить»: прочие деактивируются, новый активен | `confirm_creation` | — | = (Δ4) | `g1_to_g8_*`, `database::creating_*` |
| G9 | Анонс после создания: канал / пользователи / везде / пропустить | `handle_announce_target` | — | ≈ (Δ2) | `g1_to_g8_*`, `g9_*`, `mailing::announce_*` |
| G10 | «Назад» на каждом шаге; с первого — в меню | `back_from_*` | — | = (Δ11) | `g10_*` |
| G11 | Неизвестный вариант старта → «сейчас» | `calculate_dates` | — | = | `time::start_options_*` |

## 4. Анонс активного розыгрыша

| # | Поведение | Python | Py-тест | | Rust-тест |
|---|---|---|---|---|---|
| N1 | Нет активного → alert | `announce.py::announce_giveaway` | — | = | `n1_to_n3_*` |
| N2 | Экран «Анонсирование», «⏰ До: … МСК» | `announce_giveaway` | — | = | `n1_to_n3_*` |
| N3 | Отправка, итог «Отправлено: N» | `handle_manual_announce` | — | ≈ (Δ2) | `n1_to_n3_*`, `mailing::channel_only_*` |

## 5. Завершение и победители

| # | Поведение | Python | Py-тест | | Rust-тест |
|---|---|---|---|---|---|
| W1 | Нет активного → alert | `winners.py::start_complete_giveaway` | — | = | `w1_to_w7_*` |
| W2 | Подтверждение → `ended_at`, неактивен | `confirm_end_giveaway` | — | = | `w1_to_w7_*` |
| W3 | «Нет, продолжить» → «❌ Отменено» | `cancel_end_giveaway` | — | = | `w1_to_w7_*` |
| W4 | Случайно среди участников без подозрительных, min(N, легитимных) | `giveaway_service.py::select_winners` | `test_giveaway_service.py` | ≈ (Δ3) | `winners::*`, `w4_*` |
| W5 | Снимки username и `ended_at`/`end_at` | `select_winners` | `test_select_winners_uses_ended_at_snapshot` | = | `winners::snapshot_*` |
| W6 | `N. @username` / `N. ID: <id>` / `No winners` | `format_winner_list` | `test_format_winner_list_*` | = | `winners::winner_list_*`, `empty_username_*` |
| W7 | Публикация: канал / админам / пользователям / везде | `publish_results` | — | ≈ (Δ2, Δ13) | `w1_to_w7_*`, `w7_*`, `mailing::results_*` |

## 6. Рассылка

| # | Поведение | Python | Py-тест | | Rust-тест |
|---|---|---|---|---|---|
| B1 | Тип: текст / медиа + подпись | `broadcast_wizard.py::start_broadcast` | — | = | `admin_broadcast::b1_b2_b5_*` |
| B2 | Текст > 4096 → отказ | `process_broadcast_text` | — | = | `b1_b2_b5_*` |
| B3 | Медиа + подпись, предпросмотр | `process_broadcast_media` | — | = | `b3_*` |
| B4 | «Редактировать» | `edit_broadcast` | — | = | `b3_*`, `b4_*` |
| B5 | Всем пользователям; пусто → «❌ В базе нет пользователей…»; итог | `confirm_broadcast` | — | ≈ (Δ2) | `b1_b2_b5_*`, `b4_*`, `mailing::broadcast_*` |
| B6 | Пауза `1/RPS`; 429 → ждать и повторить; 403 → не доставлено | `services/mailing.py::send_mass_message` | `test_mailing.py` | ≈ (Δ2, Δ9) | `mailing::*` |
| B7 | `parse_mode=HTML`, текст админа как есть | `main.py` | — | = | `mailing::media_is_sent_*` |

## 7. Подозрительные аккаунты

| # | Поведение | Python | Py-тест | | Rust-тест |
|---|---|---|---|---|---|
| S1 | Меню: пометить / снять / список / главное меню | `suspicious.py::suspicious_menu` | — | = | `admin_broadcast::s1_s2_s3_*` |
| S2 | Username в любом виде | `utils/usernames.py::parse_username` | `test_usernames.py` | = | `usernames::*` |
| S3 | Не распознан / не найден | `suspicious.py::_apply` | — | = | `s1_s2_s3_*` |
| S4 | Список по username, страницы ≤ 3800 | `list_suspicious`, `_paginate` | `test_suspicious_list.py` | = (Δ12) | `s4_*`, `paginate_*` |
| S5 | Пользователь пометку не видит | — | — | = | — |

## 8. Google Sheets

| # | Поведение | Python | Py-тест | | Rust-тест |
|---|---|---|---|---|---|
| X1 | Кнопка всегда; toast «Синхронизация начата...», итог новым сообщением | `menu.py::sync_google_sheets` | `test_settings.py` | = (Δ16) | `admin_giveaway::x1_*`, `sheets::sync_all_skips_*` |
| X2 | 5 листов полностью перезаписываются | `services/sheets_sync.py` | `test_sheets_format.py` | = (Δ16) | `sheets::sync_rewrites_all_five_sheets` |

## 9. Конфигурация

| # | Поведение | Python | Py-тест | | Rust-тест |
|---|---|---|---|---|---|
| C1 | `ADMIN_IDS` через запятую с пробелами | `config/settings.py` | `test_settings.py` | = | `config::admin_ids_*` |
| C2 | `CHANNEL_ID` — целое | `settings.py` | `test_settings.py` | = | `config::channel_id_*` |
| C3 | Значения по умолчанию RPS 20/20, `MAX_RETRIES` 5 | `settings.py` | `test_settings.py` | = | `config::defaults_*` |
| C4 | `DATABASE_URL`, `REDIS_*`, `DB_POOL_*` | `settings.py` | — | Δ | — |
| C5 | `admin_drafts` — мёртвый код, пустая таблица | `db/repo/draft_repo.py` | — | Δ | — |

## Осознанные отличия

Полный список с причинами — [SPEC.md](SPEC.md) §10. Кратко:

| # | Отличие |
|---|---|
| Δ1 | Состояния мастеров и верификации в SQLite, переживают рестарт; Redis-сессии при переключении теряются |
| Δ2 | Рассылки — фоновая очередь с прогрессом в БД, без дублей после рестарта; 429 повторяется до `MAX_RETRIES` |
| Δ3 | Победители — CSPRNG; повторный выбор возвращает уже выбранных |
| Δ4 | Два активных розыгрыша запрещены на уровне БД |
| Δ5 | `/start` и мастера только в личке |
| Δ6 | Callback без обработчика гасится без текста |
| Δ7 | «Назад» в мастере рассылки и при вводе username возвращает на экран назад |
| Δ8 | Лог — journald, `ADMIN_IDS`/`CHANNEL_ID` не логируются |
| Δ9 | Пауза `1/RPS` после каждой попытки, в том числе неудачной и последней |
| Δ10 | `/start` и `/admin` во время мастера — команды, а не ввод мастера |
| Δ11 | «Назад» с первого шага мастера показывает меню с учётом активного розыгрыша |
| Δ12 | Список подозрительных сортируется бинарно (SQLite), Postgres — по collation |
| Δ13 | Повторяющиеся id в `ADMIN_IDS` получают результаты один раз |
| Δ14 | Блок верификации не истекает через 30 дней (привязан к розыгрышу) |
| Δ15 | Не-админ, нажавший устаревшую админскую кнопку, видит «доступ запрещён» |
| Δ16 | Sheets: сбой любого листа → «⚠️ не выполнена»; повторное нажатие во время синхронизации ждёт её итога |
