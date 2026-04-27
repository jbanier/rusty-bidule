---
name: webex-room-conversation
description: Fetches Webex room messages for a named room within a date interval using EU dates (DD/MM/YYYY). Use when investigating incident room timelines, reconstructing conversation context, or exporting room evidence for a specific period.
compatibility: Requires a Webex access token (WEBEX_ACCESS_TOKEN env var or saved via webex_auth.py login) and HTTPS access to webexapis.com.
metadata:
  keywords: webex, room, messages, conversation, chat, incident, timeline, history
---

# Webex Room Conversation Fetch

Use this skill to retrieve all messages from a Webex room identified by room title.
Invoke it through the agent by asking to use the `webex-room-conversation` skill
and providing the room name and date range.

Tools:
  - name: Fetch Webex Room Messages
    slug: webex_room_message_fetch
    description: Fetch all messages from a named Webex room for a date interval. Required parameters: room or room_name (room title), since or date_start (start date DD/MM/YYYY). Optional: until or date_end (end date DD/MM/YYYY), hour_start, hour_end, timezone (default UTC), include_person_names (true/false).
    script: scripts/webex_room_message_fetch.py
    network: true
    filesystem: read_only

## When to use

- Build incident timelines from room conversation history
- Export room messages as evidence for a date interval
- Verify who said what during a known investigation window

## Authentication setup

### Option A — Personal access token (fastest for testing)

Go to https://developer.webex.com/docs/getting-started, copy your personal token, then:

```bash
export WEBEX_ACCESS_TOKEN="<token>"
```

Personal access tokens expire after 12 hours.

### Option B — OAuth integration (recommended for repeated use)

1. Create a Webex integration at https://developer.webex.com/my-apps  
   - Grant type: Authorization Code  
   - Redirect URI: `http://127.0.0.1:8910/callback`  
   - Scopes: `spark:messages_read spark:rooms_read spark:people_read`

2. Authorize and save the token:

```bash
python3 scripts/webex_auth.py login \
  --client-id <your-client-id> \
  --client-secret <your-client-secret>
```

Token is saved to `~/.config/rusty-bidule/webex_token.json` (permissions `0600`).  
The fetch script loads it automatically — no env var needed.

3. Refresh when expired (uses the stored refresh token):

```bash
python3 scripts/webex_auth.py refresh \
  --client-id <id> --client-secret <secret>
```

4. Inspect saved token metadata (no secrets printed):

```bash
python3 scripts/webex_auth.py show
```

Override token file path with `WEBEX_TOKEN_FILE=/path/to/token.json`.

## Fetch messages command

```bash
python3 scripts/webex_room_message_fetch.py \
  --room "Incident room 123456" \
  --since 26/03/2026
```

Optional explicit end date and timezone:

```bash
python3 scripts/webex_room_message_fetch.py \
  --room "Incident room 123456" \
  --since 26/03/2026 \
  --until 27/03/2026 \
  --timezone Europe/Paris \
  --include-person-names
```

## Output

The script returns JSON with:

- query metadata (`room`, `room_id`, `since`, `until`, `timezone`)
- fetch summary (`total_messages`)
- `messages` array containing raw Webex message fields
- optional `person_display_name` enrichment when requested

## Edge cases

- No room title match: exits with a clear error
- Multiple room title matches: exits with an ambiguity error and candidate room IDs
- Invalid date format: exits with validation error and DD/MM/YYYY example
- Missing token: exits with guidance to run `webex_auth.py login`
- API rate limiting or transient failures: returns a clear HTTP error context
