#!/usr/bin/env bash
set -euo pipefail

# Prints the chat id of the most recent conversation with the Telegram bot.
#
# Usage:
#   scripts/get-chat-id.sh [TOKEN_FILE]     # defaults to ./TOKEN
#
# Requirements: curl, python3.
# First send the bot a message (e.g. /start) from the chat you want to use,
# then run this script. The token file must contain the bot token (no quotes).

TOKEN_FILE="${1:-TOKEN}"

if [ ! -r "$TOKEN_FILE" ]; then
  echo "error: cannot read token file '$TOKEN_FILE'" >&2
  exit 1
fi

TOKEN="$(tr -d '[:space:]' < "$TOKEN_FILE")"
if [ -z "$TOKEN" ]; then
  echo "error: token file '$TOKEN_FILE' is empty" >&2
  exit 1
fi

curl -s "https://api.telegram.org/bot${TOKEN}/getUpdates?timeout=5" | python3 -c '
import json
import sys

data = json.load(sys.stdin)
if not data.get("ok"):
    print("error:", data.get("description"), file=sys.stderr)
    sys.exit(1)

updates = data.get("result", [])
if not updates:
    print("No updates found. Send the bot a message first (e.g. /start).", file=sys.stderr)
    sys.exit(1)


def chat_id(update):
    for path in ("message", "edited_message", "channel_post", "edited_channel_post"):
        msg = update.get(path)
        if isinstance(msg, dict) and isinstance(msg.get("chat"), dict):
            return msg["chat"]["id"]
    cb = update.get("callback_query")
    if isinstance(cb, dict) and isinstance(cb.get("message"), dict):
        return cb["message"]["chat"]["id"]
    return None


last = None
for update in updates:
    cid = chat_id(update)
    if cid is not None:
        last = cid

if last is None:
    print("No chat id found in updates.", file=sys.stderr)
    sys.exit(1)

print(last)
'
