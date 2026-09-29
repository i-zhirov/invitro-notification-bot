# Invitro appointment notification bot

Telegram bot that watches the Invitro.ru online appointment system and notifies
you when new slots appear for a doctor.

Target doctor (configurable):
<https://www.invitro.ru/kurgan/vrachi/ginekolog/19143/> — Хохлова Ольга Евгеньевна,
гинеколог, МО Курган-4 (Гоголя 133).

Written in Rust: a single static binary (~10 MB), no runtime dependencies —
suitable for a small NixOS VPS. Polls the public JSON API of invitro.ru every
`INVITRO_POLL_INTERVAL_SECS` (default 120 s) with a browser-like User-Agent; no
authentication or cookies are required.

## How it works

The Invitro.ru frontend is a SPA backed by a public API at `https://www.invitro.ru/golk`.
The bot reproduces the flow the web page uses:

1. `GET /doctors/api/v1/mapper/doctors/bitrix/{id}` — map the numeric doctor id
   from the page URL (`19143`) to an internal UUID.
2. `GET /addresses/api/v1/cities?q={slug}` — resolve the city slug (`kurgan`).
3. `GET /appointments/api/v1/doctors/{uuid}/specialties/{sid}/?cityID=` — offices
   and services of the doctor for the watched specialty. Offices are filtered to
   the target city (a doctor can have offices in several cities).
4. `GET /appointments/api/v1/intervals/doctors?officeID=&doctorID=&serviceID=&slotDate=` —
   availability per date for the next ~30 days.
5. `GET /appointments/api/v1/slots/doctors?...&slotDate=` — concrete times for
   each available date.

New slots (office + service + date + time never seen before) trigger a Telegram
message grouped by office/service. The set of already-notified slots is persisted
in a state file, so restarts never re-notify.

## Configuration (environment variables)

| Variable | Default | Description |
|---|---|---|
| `INVITRO_TELEGRAM_BOT_TOKEN` | — (required) | Bot token from [@BotFather](https://t.me/BotFather) |
| `INVITRO_TELEGRAM_CHAT_ID` | — (required) | Chat/group id to send notifications to |
| `INVITRO_DRY_RUN` | `0` | `1` = print notifications to stdout instead of sending |
| `INVITRO_DOCTOR_BITRIX_ID` | `19143` | Numeric doctor id from the page URL |
| `INVITRO_CITY_SLUG` | `kurgan` | City slug from the page URL |
| `INVITRO_SPECIALTY_SLUG` | (primary) | Specialty slug to watch, e.g. `ginekolog` |
| `INVITRO_SERVICE_IDS` | (all consultations) | Comma-separated service UUIDs; if empty, all `is_consultation` services are watched |
| `INVITRO_POLL_INTERVAL_SECS` | `120` | Seconds between polls |
| `INVITRO_STATE_FILE` | `state.json` | Path of the de-duplication state file |
| `INVITRO_NOTIFY_ON_FIRST_RUN` | `1` | `1` = notify about currently open slots on the first poll; `0` = record them as baseline silently |

## Local usage

```sh
cargo build --release

# Inspect what the bot will watch (no Telegram credentials required):
./target/release/invitro-bot --check

# Run a single poll cycle (needs the two Telegram vars; use DRY_RUN=1 to test):
INVITRO_DRY_RUN=1 \
INVITRO_TELEGRAM_BOT_TOKEN=123:abc \
INVITRO_TELEGRAM_CHAT_ID=456 \
./target/release/invitro-bot --once

# Run the poll loop:
INVITRO_TELEGRAM_BOT_TOKEN=123:abc \
INVITRO_TELEGRAM_CHAT_ID=456 \
./target/release/invitro-bot
```

> macOS note: if the default `cc` in your PATH is not the Apple clang (e.g. a Nix
> clang), linking needs the SDK path:
> `RUSTFLAGS="-C link-arg=-Wl,-syslibroot,$(xcrun --show-sdk-path)" cargo build`
> On Linux/NixOS this is not needed.

## Telegram setup

1. Create a bot with [@BotFather](https://t.me/BotFather) and copy the token.
2. Find your chat id: message your bot, then run
   `curl "https://api.telegram.org/bot<TOKEN>/getUpdates"` and read the `chat.id`
   (for private chats it is usually a negative-free numeric id; for groups the id
   starts with `-100`).

## Deployment on NixOS

The repository is a flake and ships a NixOS module.

```nix
# configuration.nix
{
  inputs.invitro-notification-bot.url = "github:<you>/invitro-notification-bot";

  services.invitroBot = {
    enable = true;
    package = inputs.invitro-notification-bot.packages.${pkgs.system}.default;
    environmentFile = "/run/secrets/invitro-bot.env";
  };
}
```

`/run/secrets/invitro-bot.env` (mode 600, e.g. via agenix/sops-nix or a manual
systemd credentials file) contains:

```ini
INVITRO_TELEGRAM_BOT_TOKEN=123:abc
INVITRO_TELEGRAM_CHAT_ID=456
```

Everything else can be overridden through the module options
(`doctorBitrixId`, `citySlug`, `specialtySlug`, `serviceIds`, `pollIntervalSecs`,
`notifyOnFirstRun`).

The service runs as a dynamic user with a hardened sandbox
(`ProtectSystem=strict`, `NoNewPrivileges`, …), stores its state in
`/var/lib/invitro-bot/state.json`, restarts on failure, and logs to journald:

```sh
journalctl -u invitro-bot -f
```

You can also build or run the binary directly from the flake:

```sh
nix build .#default
nix run .# -- --check
```
