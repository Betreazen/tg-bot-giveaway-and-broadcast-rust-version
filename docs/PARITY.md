# Паритет с Python-версией

Исходник: [tg-bot-giveaway-and-broadcast](https://github.com/Betreazen/tg-bot-giveaway-and-broadcast),
коммит `9a4c9c6` (то, что работает в production). Ссылки ниже — пути в том репозитории.
Колонка «Rust-тест» заполняется по мере реализации.

Условные обозначения: **=** — поведение повторяется один в один; **≈** — то же для
пользователя, но иначе внутри; **Δ** — осознанное отличие (см. раздел в конце).

## 1. Участник: `/start` и верификация

| # | Поведение | Python | Py-тест | Паритет | Rust-тест |
|---|---|---|---|---|---|
| U1 | `/start` (с любым payload, напр. `?start=join`) сохраняет пользователя: upsert `user_id`, username перезаписывается только новым не-NULL значением | `handlers/start.py:59`, `db/repo/user_repo.py:417` | — | = | |
| U2 | Не подписан на `CHANNEL_ID` (`getChatMember` не `creator/administrator/member`, либо любая ошибка API) → `user.not_subscribed` | `services/subscription.py:376` | — | = | |
| U3 | Нет розыгрыша с `is_active=true` → `user.no_active_giveaway` | `handlers/start.py:72` | — | = | |
| U4 | Уже участник → `user.already_participating` | `handlers/start.py:80` | — | = | |
| U5 | Админ (из `ADMIN_IDS`) участвует сразу, без верификации и без проверки username → `user.participation_confirmed` | `handlers/start.py:88` | — | = | |
| U6 | Нет `@username` → `user.no_username`, участие не создаётся | `handlers/start.py:108` | — | = | |
| U7 | Заблокирован для этого розыгрыша → `user.verification_blocked` | `handlers/start.py:115` | — | = | |
| U8 | Уже идёт верификация (< 180 с) → `user.verification_in_progress`; истекла → начать заново | `handlers/start.py:124` | — | = | |
| U9 | Попыток ≥ 3 → блок и `user.verification_blocked` | `handlers/start.py:138` | — | = | |
| U10 | Верификация: 5 разных цифр 0–9, одна верная, клавиатура 3+2, `callback_data=verify:<n>`, текст `user.verification_prompt` | `handlers/verification.py:203-248` | `test_verification.py` | = | |
| U11 | Верная кнопка → участие (idempotent `ON CONFLICT DO NOTHING`), сообщение редактируется в `user.participation_confirmed` | `handlers/verification.py:296` | — | = | |
| U12 | Неверная → попытка +1 (счётчик per giveaway+user переживает новые сессии); кнопки перемешиваются; `user.verification_wrong` с остатком | `handlers/verification.py:331-360` | — | = | |
| U13 | 3-я неверная → блок до конца этого розыгрыша (новый розыгрыш — новые попытки), сообщение `user.verification_blocked` | `handlers/verification.py:337` | — | = | |
| U14 | Нажатие после 180 с → alert `user.verification_timeout`, состояние сброшено | `handlers/verification.py:289` | — | = | |
| U15 | Нажатие `verify:` без активной верификации — игнор | router filter | — | ≈ (callback гасится без текста) | |
| U16 | Ошибка в обработчике → `errors.generic` | `handlers/start.py:169` | — | = | |
| U17 | Прочие сообщения пользователя вне мастеров — без ответа | aiogram default | — | = | |

## 2. Админ: доступ и меню

| # | Поведение | Python | Py-тест | Паритет | Rust-тест |
|---|---|---|---|---|---|
| A1 | Любой админский хендлер от не-админа → `admin.access_denied` (сообщение, у callback — alert) | `middlewares/admin.py` | `test_admin_middleware.py` | = | |
| A2 | `/admin` не в личке → `admin.use_private_chat` | `handlers/admin/entry.py:99` | — | = | |
| A3 | Главное меню: «Создать», [«Объявить», «Завершить/Победители» — только при активном], «Рассылка», «Статус», «Подозрительные», «Синхронизация Google Sheets», «Закрыть» | `keyboards/admin.py:600` | — | = | |
| A4 | «Закрыть» удаляет сообщение меню | `entry.py:118` | — | = | |
| A5 | «Статус»: нет активного → alert `admin.status_no_active`; иначе новое сообщение `admin.status_active` (конец — `%Y-%m-%d %H:%M` МСК, без метки) | `menu.py:146` | — | = | |
| A6 | «Главное меню» из любого мастера сбрасывает состояние и показывает меню | `giveaway_wizard.py:745` | — | = | |
| A7 | «Отменить» (`nav:cancel`) в любом состоянии → `admin.operation_cancelled`, состояние сброшено | `giveaway_wizard.py:735` | — | = | |
| A8 | Время всегда хранится в UTC, показывается по МСК (UTC+3, без перехода на летнее) | `utils/datetimes.py` | `test_datetimes.py` | = | |

## 3. Мастер создания розыгрыша

| # | Поведение | Python | Py-тест | Паритет | Rust-тест |
|---|---|---|---|---|---|
| G1 | Старт: «Когда начать?» — Сейчас / +1 ч / +3 ч / +6 ч / Завтра 12:00 МСК | `date_picker.py:248`, `giveaway_wizard.py:354` | — | = | |
| G2 | Длительность: 1/3/7/14/30 дней; даты считаются в МСК и сохраняются в UTC в момент выбора длительности | `date_picker.py:225,271` | — | = | |
| G3 | Описание: только текст, > 4096 символов → `wizard.description_too_long` | `giveaway_wizard.py:419` | — | = | |
| G4 | Число победителей: целое ≥ 1, иначе `wizard.invalid_winner_count` | `giveaway_wizard.py:439` | — | = | |
| G5 | Медиа обязательно: фото (наибольшее), видео, GIF (animation), документ; иначе `wizard.invalid_media` | `giveaway_wizard.py:464` | — | = | |
| G6 | Предпросмотр с датами (`%d.%m.%Y %H:%M МСК`, длительность в днях), победителями, описанием, типом медиа | `date_picker.py:305`, `giveaway_wizard.py:499` | — | = | |
| G7 | «Редактировать» → снова ввод описания (без «Назад») | `giveaway_wizard.py:558` | — | = | |
| G8 | «Подтвердить»: все активные розыгрыши деактивируются, создаётся новый активный | `giveaway_wizard.py:512` | — | = | |
| G9 | Анонс после создания: канал / всем пользователям / везде / пропустить; кнопка «🎁 Участвовать» → `JOIN_URL`; итог «Отправлено: N» (канал считается за 1) | `giveaway_wizard.py:573` | — | ≈ (рассылка через очередь, см. Δ2) | |
| G10 | «Назад» на каждом шаге до медиа включительно ведёт на предыдущий шаг; с первого шага — в меню | `giveaway_wizard.py:654-732` | — | = | |
| G11 | Неизвестный `start_time` → «сейчас» | `date_picker.py:295` | — | = | |

## 4. Анонс активного розыгрыша

| # | Поведение | Python | Py-тест | Паритет | Rust-тест |
|---|---|---|---|---|---|
| N1 | Нет активного → alert «Нет активного розыгрыша для анонсирования» | `announce.py:799` | — | = | |
| N2 | Экран с описанием, победителями, «⏰ До: … МСК», выбор канал/пользователи/везде/отмена | `announce.py:806` | — | = | |
| N3 | Отправка: тот же текст и медиа, что при создании; итог «✅ Анонс отправлен! Отправлено: N» | `announce.py:827` | — | ≈ (Δ2) | |

## 5. Завершение и победители

| # | Поведение | Python | Py-тест | Паритет | Rust-тест |
|---|---|---|---|---|---|
| W1 | Нет активного → alert «Нет активного розыгрыша» | `winners.py:38` | — | = | |
| W2 | Подтверждение «Завершить сейчас?» → `ended_at=now`, `is_active=false` | `winners.py:57`, `giveaway_repo.py:232` | — | = | |
| W3 | «Нет, продолжить» → «❌ Отменено» | `winners.py:88` | — | = | |
| W4 | Выбор: случайно среди участников, **подозрительные исключены**, число = min(N, легитимных); без легитимных → `admin.no_participants` | `services/giveaway_service.py:427` | `test_giveaway_service.py` | ≈ (Δ3: криптостойкий RNG, повторный выбор невозможен) | |
| W5 | Снимки победителя: username участника, `giveaway_end_snapshot = ended_at или end_at` | `giveaway_service.py:472-485` | `test_select_winners_uses_ended_at_snapshot` | = | |
| W6 | Список: `N. @username` или `N. ID: <id>`; пустой → `No winners` | `giveaway_service.py:492` | `test_format_winner_list_*` | = | |
| W7 | Публикация: канал / только админам (с `ANNOUNCE_RPS`) / пользователям (с `BROADCAST_RPS`) / везде; текст результатов с «📞 С победителями свяжутся…» | `winners.py:146` | — | ≈ (Δ2) | |

## 6. Рассылка

| # | Поведение | Python | Py-тест | Паритет | Rust-тест |
|---|---|---|---|---|---|
| B1 | Тип: только текст / медиа + подпись | `broadcast_wizard.py:235-276` | — | = | |
| B2 | Текст > 4096 → «❌ Текст слишком длинный (максимум 4096 символов)» | `broadcast_wizard.py:285` | — | = | |
| B3 | Медиа: фото/видео/GIF/документ + необязательная подпись; предпросмотр с типом, подписью или «(нет)», числом символов | `broadcast_wizard.py:303` | — | = | |
| B4 | «Редактировать» → повтор ввода по типу | `broadcast_wizard.py:398` | — | = | |
| B5 | Подтверждение → всем пользователям из `users`; пусто → «❌ В базе нет пользователей для рассылки»; итог: всего/отправлено/не доставлено/длительность | `broadcast_wizard.py:345` | — | ≈ (Δ2) | |
| B6 | Отправка с паузой `1/RPS`; 429 → ждать `retry_after` и повторить; 403 → «не доставлено» (blocked); прочие ошибки → «не доставлено» | `services/mailing.py:548` | `test_mailing.py` | ≈ (Δ2: повторы до `MAX_RETRIES`) | |
| B7 | Текст рассылок, описаний и анонсов отправляется с `parse_mode=HTML` как есть (админ может писать `<b>`) | `main.py:73` | — | = | |

## 7. Подозрительные аккаунты

| # | Поведение | Python | Py-тест | Паритет | Rust-тест |
|---|---|---|---|---|---|
| S1 | Меню: пометить / снять / список / главное меню | `suspicious.py:460`, `keyboards/admin.py:642` | — | = | |
| S2 | Ввод username в любом виде: `@name`, `t.me/name`, `https://t.me/name?x`, `name`; 4–32 символа `[A-Za-z0-9_]`, регистр не важен | `utils/usernames.py` | `test_usernames.py` | = | |
| S3 | Не распознан → сообщение об ошибке + меню; не найден в `users` → «⚠️ … не найден в базе» | `suspicious.py:541` | — | = | |
| S4 | Список: сортировка по username, затем id; `@name` или «без username», ID в `<code>`; страницы ≤ 3800 символов, первая — редактированием меню | `suspicious.py:470-512` | `test_suspicious_list.py` | = | |
| S5 | Пользователю пометка не видна никак | — | — | = | |

## 8. Google Sheets

| # | Поведение | Python | Py-тест | Паритет | Rust-тест |
|---|---|---|---|---|---|
| X1 | Кнопка в меню всегда; при `SHEETS_SYNC_ENABLED=false` → alert «Синхронизация начата...» и «⚠️ Синхронизация не выполнена (возможно, отключена или нет credentials)» | `menu.py:185`, `services/sheets_sync.py:276` | `test_settings.py` | см. решение в спецификации | |
| X2 | При включении: листы Overview, Users, Participants, Winners, Giveaways Summary полностью перезаписываются | `sheets_sync.py:85-273` | `test_sheets_format.py` | см. решение в спецификации | |

## 9. Конфигурация и эксплуатация

| # | Поведение | Python | Py-тест | Паритет | Rust-тест |
|---|---|---|---|---|---|
| C1 | `ADMIN_IDS` через запятую с пробелами | `config/settings.py:62` | `test_settings.py` | = | |
| C2 | `CHANNEL_ID` — целое (`-100…`) | `settings.py:27` | `test_settings.py` | = | |
| C3 | Значения по умолчанию: `BROADCAST_RPS=20`, `ANNOUNCE_RPS=20`, `MAX_RETRIES=5`, `LOG_LEVEL=INFO` | `settings.py:43-53` | `test_settings.py` | = | |
| C4 | `DATABASE_URL`, `REDIS_URL`, `REDIS_FSM_PREFIX`, `DB_POOL_SIZE`, `DB_MAX_OVERFLOW` | `settings.py` | — | Δ (не нужны: SQLite в `DATA_DIR`) | |
| C5 | `admin_drafts` и `draft_repo` — мёртвый код (нигде не вызывается), таблица пуста | `db/repo/draft_repo.py` | — | Δ (таблица не переносится) | |

## Осознанные отличия (Δ)

Заполняется по итогам утверждённой спецификации — см. `docs/SPEC.md`.
