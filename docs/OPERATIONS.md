# Эксплуатация

## Состояние

```bash
systemctl status tg-bot-giveaway-and-broadcast
journalctl -u tg-bot-giveaway-and-broadcast --since today
runuser -u tg-bot-giveaway-and-broadcast -- env DATA_DIR=/var/lib/tg-bot-giveaway-and-broadcast \
    /opt/tg-bot-giveaway-and-broadcast/bot --healthcheck && echo healthy
systemctl show tg-bot-giveaway-and-broadcast -p MemoryCurrent -p MemoryPeak -p CPUUsageNSec -p NRestarts
```

Healthcheck проверяет две вещи: процесс жив (heartbeat не старше 90 с) и SQLite цела.
Доступность Telegram он не проверяет. О сбоях связи говорит строка журнала
`Telegram polling failed; retrying with backoff`; бот сам повторяет запросы.

## Рассылки

- Идут по очереди, по одной. Прогресс пишется в журнал каждые 100 адресатов:
  `mailing progress done=… total=…`.
- По окончании сообщение админа «📤 …» заменяется итогом.
- После перезапуска незаконченная рассылка продолжается со следующего адресата.
  Сообщение, которое уходило в момент падения, считается недоставленным и повторно не
  отправляется.
- Скорость — не больше `BROADCAST_RPS`/`ANNOUNCE_RPS` сообщений в секунду. Фактически меньше:
  отправки последовательные.

## Бэкап

Все данные лежат в `/var/lib/tg-bot-giveaway-and-broadcast/bot.db` (режим WAL).
Согласованная копия без остановки бота:

```bash
sqlite3 /var/lib/tg-bot-giveaway-and-broadcast/bot.db ".backup /root/backups/giveaway-$(date +%F).db"
```

Без `sqlite3`: остановить сервис и скопировать `bot.db`, `bot.db-wal`, `bot.db-shm`.
Секреты лежат в `/etc/tg-bot-giveaway-and-broadcast/` (`bot.env`, `service_account.json`).

## Типичные ситуации

| Симптом | Причина и действие |
|---|---|
| Всем пишет «Вы не подписаны» | Бот не админ канала `CHANNEL_ID` или id канала неверный |
| Анонс в канал «Отправлено: 0» | Бот не может публиковать в канал; подпись к медиа длиннее 1024 символов |
| `409 Conflict` в журнале | Тем же токеном опрашивает второй процесс — остановите его |
| «⚠️ Синхронизация не выполнена» | `SHEETS_SYNC_ENABLED`, путь к ключу, доступ сервисного аккаунта к таблице; подробности — в журнале |
| `database schema is newer than this application` | Запущен старый бинарник на базе от новой версии — обновите бинарник |
