# Развёртывание

Бот — один исполняемый файл для Linux x86_64 (glibc ≥ 2.36) и файл SQLite. Docker
на сервере не нужен.

## 1. Сборка

Компилировать на сервере не нужно: сборка съест память у соседних сервисов. Собирайте на
своей машине с Docker, образ сборки — Debian bookworm:

```bash
docker build --target build -t tg-giveaway:build .
id=$(docker create tg-giveaway:build)
docker cp "$id":/usr/local/bin/bot ./bot
docker rm "$id"
```

Заодно можно прогнать тесты на Linux: `docker build --target test .`

## 2. Установка

Скопируйте на сервер `bot`, `.env.example` и каталог `deploy/`, затем от root:

```bash
bash deploy/install.sh .
```

Скрипт:

- создаёт системного пользователя `tg-bot-giveaway-and-broadcast`;
- кладёт бинарник в `/opt/tg-bot-giveaway-and-broadcast/`;
- создаёт каталог данных `/var/lib/tg-bot-giveaway-and-broadcast` (0700);
- создаёт каталог настроек `/etc/tg-bot-giveaway-and-broadcast` (0750 root:<сервис>);
- ставит unit systemd.

Существующий `bot.env` скрипт не перезаписывает и сервис не запускает.

Заполните `/etc/tg-bot-giveaway-and-broadcast/bot.env` (0600, root):

```ini
BOT_TOKEN=...
ADMIN_IDS=111,222
CHANNEL_ID=-100...
JOIN_URL=https://t.me/<бот>?start=join
```

Для Google Sheets:

```bash
install -m 0640 -o root -g tg-bot-giveaway-and-broadcast service_account.json \
    /etc/tg-bot-giveaway-and-broadcast/service_account.json
```

Затем добавьте в `bot.env` строки `SHEETS_SYNC_ENABLED=true`,
`GOOGLE_CREDENTIALS_PATH=/etc/tg-bot-giveaway-and-broadcast/service_account.json` и
`SPREADSHEET_ID=...`. Сервисному аккаунту нужен доступ редактора к таблице.

## 3. Проверка и запуск

```bash
set -a; . /etc/tg-bot-giveaway-and-broadcast/bot.env; set +a
DATA_DIR=/var/lib/tg-bot-giveaway-and-broadcast \
    runuser -u tg-bot-giveaway-and-broadcast -p -- /opt/tg-bot-giveaway-and-broadcast/bot --check
systemctl enable --now tg-bot-giveaway-and-broadcast
systemctl is-active tg-bot-giveaway-and-broadcast
journalctl -u tg-bot-giveaway-and-broadcast -n 50
```

Одним токеном может опрашивать Telegram только один процесс. Второй получит `409 Conflict`.
Перед запуском остановите прежнюю версию бота.

## 4. Переход с Python-версии

Данные PostgreSQL переносятся в пустую базу SQLite, id сохраняются. Незаконченные
админские мастера и верификации из Redis не переносятся: админам придётся начать заново.

1. **Бэкап**, пока Python-версия работает:
   - `.env`, `service_account.json`, `docker-compose.override.yml`, `logs/`;
   - `docker exec <postgres> pg_dump -Fc -U <user> <db> > giveaway.dump`.
2. **Остановка бота** (PostgreSQL остаётся запущенным): `docker compose stop bot`.
3. **Выгрузка** четырёх таблиц в JSON Lines, время — в UTC:

   ```bash
   for t in users:user_id giveaways:id participants:id winners:id; do
     docker exec <postgres> psql -U <user> -d <db> -At \
       -c "SET TIME ZONE 'UTC'" \
       -c "SELECT row_to_json(x) FROM ${t%%:*} x ORDER BY ${t##*:}" > "dump/${t%%:*}.jsonl"
   done
   ```

4. **Импорт и сверка:**

   ```bash
   DATA_DIR=/var/lib/tg-bot-giveaway-and-broadcast /opt/tg-bot-giveaway-and-broadcast/bot import dump
   chown -R tg-bot-giveaway-and-broadcast: /var/lib/tg-bot-giveaway-and-broadcast
   ```

   Импорт работает одной транзакцией и отказывается писать в непустую базу. Он печатает
   `users=…`, `suspicious=…`, `giveaways=…`, `participants=…`, `winners=…`. Сравните эти
   числа с `SELECT count(*)` в PostgreSQL.
5. `--check`, затем `systemctl enable --now` (раздел 3).
6. **Откат:** `systemctl disable --now tg-bot-giveaway-and-broadcast`, затем
   `docker compose start bot`. Данные PostgreSQL за время работы Rust-версии не менялись.

Контейнеры и тома Python-версии удаляйте только после нескольких дней работы новой
версии и после проверенного бэкапа.

## 5. Обновление

```bash
bash deploy/install.sh .     # заменяет бинарник атомарно
systemctl restart tg-bot-giveaway-and-broadcast
```

Если рассылка шла во время перезапуска, она продолжится после старта.

## Docker (необязательно)

Образ на `distroless`, работает не от root. Данные хранятся в `/app/data`, healthcheck встроен:

```bash
docker build -t tg-giveaway .
docker run -d --env-file .env -v giveaway-data:/app/data tg-giveaway
```
