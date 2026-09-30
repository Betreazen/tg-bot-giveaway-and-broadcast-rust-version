# tg-bot-giveaway-and-broadcast-rust-version

Telegram-бот розыгрышей и рассылок для канала: участие через `/start` с проверкой
подписки и антибот-верификацией, админ-панель на inline-кнопках, честный выбор
победителей, анонсы и массовые рассылки, выгрузка в Google Sheets.

Это переписанная на Rust версия Python-бота
[Betreazen/tg-bot-giveaway-and-broadcast](https://github.com/Betreazen/tg-bot-giveaway-and-broadcast).
Вместо aiogram + PostgreSQL + Redis (три контейнера) — один процесс на teloxide и SQLite.
Тексты, кнопки и сценарии те же. Данные Python-версии переносятся командой `bot import`.

## Возможности

**Участникам**
- `/start` регистрирует пользователя и проверяет подписку на канал (`getChatMember`).
- Верификация перед участием: нажать кнопку с показанной цифрой. На ответ 3 минуты, на розыгрыш 3 попытки; после них участие в этом розыгрыше блокируется.
- Для участия нужен публичный `@username`: по нему связываются с победителями.

**Администраторам** (`ADMIN_IDS`, команда `/admin` в личке)
- Мастер розыгрыша:
  - начало: сейчас, через 1/3/6 часов или завтра в 12:00 МСК;
  - длительность 1–30 дней;
  - описание, число победителей и обязательное медиа для анонса;
  - предпросмотр и «Назад» на каждом шаге.
- Анонс: в канал, всем пользователям или везде, с кнопкой «🎁 Участвовать».
- Завершение и выбор победителей. Генератор криптостойкий, подозрительные аккаунты исключаются, повторный выбор возвращает тех же победителей. Публикация результатов в канал, админам или всем.
- Рассылка текста или медиа с подписью всем пользователям.
- Подозрительные аккаунты: участвуют, но не выигрывают. Username вводится в любом виде: `@name`, `t.me/name`, `name`.
- Статус активного розыгрыша и синхронизация с Google Sheets (5 листов).

**Надёжность**
- Состояния мастеров и верификации хранятся в SQLite и переживают перезапуск.
- Рассылки идут через очередь в базе. После перезапуска рассылка продолжается с того же адресата, никто не получает сообщение дважды.
- Скорость ограничена: пауза `1/RPS` после каждой отправки, ответ 429 повторяется через `retry_after`.
- Время хранится в UTC, показывается всегда по Москве (UTC+3).

## Запуск: бинарник + systemd

```bash
# на машине сборки (нужен Docker)
docker build --target build -t tg-giveaway:build .
id=$(docker create tg-giveaway:build) && docker cp "$id":/usr/local/bin/bot ./bot && docker rm "$id"

# на сервере (root), рядом с bot, .env.example и deploy/
bash deploy/install.sh .
nano /etc/tg-bot-giveaway-and-broadcast/bot.env   # BOT_TOKEN, ADMIN_IDS, CHANNEL_ID, JOIN_URL
systemctl enable --now tg-bot-giveaway-and-broadcast
journalctl -u tg-bot-giveaway-and-broadcast -f
```

Бот должен быть администратором канала из `CHANNEL_ID`: иначе он не сможет
проверять подписку и публиковать анонсы.

Подробно, с переходом с Python-версии и переносом данных, — [docs/DEPLOY.md](docs/DEPLOY.md).
Эксплуатация, бэкап и диагностика — [docs/OPERATIONS.md](docs/OPERATIONS.md).

## Настройки

| Переменная | По умолчанию | Назначение |
|---|---|---|
| `BOT_TOKEN` | — | токен бота |
| `ADMIN_IDS` | — | id администраторов через запятую |
| `CHANNEL_ID` | — | канал, `-100…` |
| `JOIN_URL` | — | ссылка кнопки «Участвовать», обычно `https://t.me/<бот>?start=join` |
| `BROADCAST_RPS` / `ANNOUNCE_RPS` | 20 / 20 | не больше стольких сообщений в секунду |
| `MAX_RETRIES` | 5 | повторы одного сообщения после 429 |
| `SHEETS_SYNC_ENABLED` | false | включить кнопку синхронизации с Google Sheets |
| `GOOGLE_CREDENTIALS_PATH`, `SPREADSHEET_ID` | — | ключ сервисного аккаунта и таблица |
| `DATA_DIR` | `/var/lib/tg-bot-giveaway-and-broadcast` | где лежит `bot.db` |
| `LOG_LEVEL` | INFO | уровень логов; `RUST_LOG` имеет приоритет |

Значения обрезаются от пробелов и `\r`, поэтому `.env`, сохранённый в Windows, тоже работает.

## Команды бинарника

| Команда | Что делает |
|---|---|
| `bot` | запуск бота |
| `bot --check` | конфигурация, миграции, проверка целостности БД — без обращения к Telegram |
| `bot --healthcheck` | свежесть heartbeat и проверка SQLite (для мониторинга) |
| `bot import <dir>` | перенос данных Python-версии из выгрузки PostgreSQL |
| `bot --version` | версия |

## Разработка

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test        # Telegram и Google подменяются wiremock, токен не нужен
cargo audit
```

Документы:
- [docs/SPEC.md](docs/SPEC.md) — устройство и осознанные отличия от Python-версии.
- [docs/PARITY.md](docs/PARITY.md) — карта поведения: функция → Python-код → Rust-тест.
- [docs/REVIEW.md](docs/REVIEW.md) — итоги ревью.

## Потребление ресурсов

Один production-сервер, те же данные, простой; 2026-09-30 (cgroup v2):

| | Python + PostgreSQL + Redis (Docker) | Rust + SQLite (systemd) |
|---|---|---|
| Память (cgroup) | 57.4 MiB, плюс 167 MiB в swap | 1.4 MiB, swap 0 |
| Пик памяти | 203 + 34.5 + 18.3 MiB | 2.1 MiB |
| Средний CPU | 1.34 % ядра, из них Redis 0.91 %, PostgreSQL 0.42 % | после суточного замера |
| Процессы | 3 контейнера, 18 PID | 1 процесс, 3 потока |
| На диске | ≈ 2.6 GB образов + 74 MB томов | бинарник 11 MB + база 3.6 MB |

Памяти по cgroup уходит примерно в 40 раз меньше. Главная экономия CPU — отказ от Redis
и PostgreSQL. Методика и подробности — [docs/BENCHMARK.md](docs/BENCHMARK.md).
