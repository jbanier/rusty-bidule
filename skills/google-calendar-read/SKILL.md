---
name: google-calendar-read
description: Reads Google Calendar events from a saved read-only OAuth token. Use when the user asks to inspect upcoming meetings, events in a date range, or a specific Google Calendar.
compatibility: Requires a Google Cloud Desktop OAuth client JSON plus HTTPS access to googleapis.com.
metadata:
  keywords: google calendar, calendar, events, meetings, schedule, agenda
---

# Google Calendar Read

Use this skill to read Google Calendar events after a one-time local OAuth login.

Tools:
  - name: List Google Calendar Events
    slug: google_calendar_list_events
    description: Read events from a Google Calendar. Optional parameters: calendar_id (default primary), days_ahead, time_min, time_max, query, max_results, include_description.
    script: scripts/google_calendar_list_events.py
    network: true
    filesystem: read_only

## When to use

- Review upcoming meetings on the primary calendar
- Check events in a specific date window
- Search a calendar for matching event text

## Authentication setup

1. In Google Cloud, enable the Google Calendar API.
2. Create an OAuth client with application type `Desktop app`.
3. Download the client JSON and either:
   - save it as `~/.config/rusty-bidule/google-oauth-client.json`, or
   - pass it explicitly with `--credentials-file`
4. Run:

```bash
python3 scripts/google_calendar_auth.py login
```

Optional explicit credentials file:

```bash
python3 scripts/google_calendar_auth.py login \
  --credentials-file /path/to/google-oauth-client.json
```

Saved token path defaults to `~/.config/rusty-bidule/google_calendar_token.json`.
Override it with `GOOGLE_CALENDAR_TOKEN_FILE=/path/to/token.json`.

## Fetch events directly

Upcoming events over the next 3 days:

```bash
python3 scripts/google_calendar_list_events.py \
  --days-ahead 3
```

Specific date range:

```bash
python3 scripts/google_calendar_list_events.py \
  --calendar-id primary \
  --time-min 2026-04-16 \
  --time-max 2026-04-18 \
  --max-results 25
```

## Output

The script returns JSON with:

- selected calendar metadata
- query window (`time_min`, `time_max`)
- `events` array containing compact event records

## Edge cases

- Missing token: exits with guidance to run the auth script
- Missing OAuth client JSON: exits with setup guidance
- Invalid time range: exits with a validation error
- Google API failure: returns HTTP context from the failing request
