#!/usr/bin/env python3
"""Read Google Calendar events with a saved read-only token."""

from __future__ import annotations

import argparse
from datetime import UTC, datetime, timedelta
import json
from pathlib import Path
import sys
from typing import Any
import urllib.parse

SHARED_DIR = Path(__file__).resolve().parents[2] / "_google_common"
sys.path.insert(0, str(SHARED_DIR))

from google_workspace_auth import (  # noqa: E402
    CONFIG_DIR,
    GoogleAuthError,
    ensure_token,
    request_json,
)

CALENDAR_SCOPE = "https://www.googleapis.com/auth/calendar.readonly"
CALENDAR_API_BASE = "https://www.googleapis.com/calendar/v3"
DEFAULT_TOKEN_FILE = CONFIG_DIR / "google_calendar_token.json"


def resolve_token_path() -> Path:
    import os

    raw = os.getenv("GOOGLE_CALENDAR_TOKEN_FILE", "").strip()
    return Path(raw).expanduser() if raw else DEFAULT_TOKEN_FILE


def parse_time_boundary(raw: str | None, *, is_end: bool) -> datetime | None:
    if raw is None or not raw.strip():
        return None

    value = raw.strip()
    if len(value) == 10 and value.count("-") == 2:
        day = datetime.strptime(value, "%Y-%m-%d")
        if is_end:
            day = day.replace(hour=23, minute=59, second=59, microsecond=0)
        return day.replace(tzinfo=UTC)

    normalized = value.replace("Z", "+00:00")
    try:
        dt = datetime.fromisoformat(normalized)
    except ValueError as exc:
        raise GoogleAuthError(
            f"Invalid time value '{raw}'. Use YYYY-MM-DD or RFC3339 such as 2026-04-16T09:00:00Z."
        ) from exc

    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=UTC)
    return dt.astimezone(UTC)


def compact_event(item: dict[str, Any], include_description: bool) -> dict[str, Any]:
    start = item.get("start") or {}
    end = item.get("end") or {}
    attendees = []
    for attendee in item.get("attendees", [])[:20]:
        if not isinstance(attendee, dict):
            continue
        attendees.append(
            {
                "email": attendee.get("email"),
                "display_name": attendee.get("displayName"),
                "response_status": attendee.get("responseStatus"),
                "self": attendee.get("self", False),
                "organizer": attendee.get("organizer", False),
            }
        )

    event = {
        "id": item.get("id"),
        "status": item.get("status"),
        "summary": item.get("summary"),
        "location": item.get("location"),
        "html_link": item.get("htmlLink"),
        "conference_link": item.get("hangoutLink"),
        "created": item.get("created"),
        "updated": item.get("updated"),
        "start": start,
        "end": end,
        "all_day": "date" in start and "dateTime" not in start,
        "organizer": {
            "email": (item.get("organizer") or {}).get("email"),
            "display_name": (item.get("organizer") or {}).get("displayName"),
            "self": (item.get("organizer") or {}).get("self", False),
        },
        "creator": {
            "email": (item.get("creator") or {}).get("email"),
            "display_name": (item.get("creator") or {}).get("displayName"),
            "self": (item.get("creator") or {}).get("self", False),
        },
        "attendees": attendees,
    }
    if include_description and item.get("description"):
        event["description"] = item.get("description")
    return event


def fetch_events(
    access_token: str,
    *,
    calendar_id: str,
    time_min: str,
    time_max: str,
    query: str | None,
    max_results: int,
    include_description: bool,
) -> dict[str, Any]:
    page_token: str | None = None
    items: list[dict[str, Any]] = []
    timezone_name: str | None = None
    encoded_calendar_id = urllib.parse.quote(calendar_id, safe="")
    url = f"{CALENDAR_API_BASE}/calendars/{encoded_calendar_id}/events"

    while len(items) < max_results:
        payload = request_json(
            url,
            access_token,
            params={
                "singleEvents": "true",
                "orderBy": "startTime",
                "timeMin": time_min,
                "timeMax": time_max,
                "maxResults": min(max_results - len(items), 2500),
                "q": query or None,
                "pageToken": page_token,
            },
        )
        if timezone_name is None:
            timezone_name = str(payload.get("timeZone") or "").strip() or None

        batch = payload.get("items", [])
        if not isinstance(batch, list):
            raise GoogleAuthError("Calendar API returned an unexpected events payload.")

        for event in batch:
            if isinstance(event, dict):
                items.append(compact_event(event, include_description))
                if len(items) >= max_results:
                    break

        page_token = payload.get("nextPageToken")
        if not page_token:
            break

    return {
        "calendar_id": calendar_id,
        "calendar_time_zone": timezone_name,
        "time_min": time_min,
        "time_max": time_max,
        "query": query,
        "total_events": len(items),
        "events": items,
    }


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "List Google Calendar events with a saved read-only token.\n\n"
            "Examples:\n"
            "  --days-ahead 3\n"
            "  --calendar-id primary --time-min 2026-04-16 --time-max 2026-04-18\n"
            "  --query incident --include-description\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--calendar-id", default="primary", help="Calendar ID, default: primary")
    parser.add_argument(
        "--days-ahead",
        type=int,
        default=7,
        help="Default look-ahead window when time-min/time-max are omitted",
    )
    parser.add_argument(
        "--time-min",
        help="Start bound as YYYY-MM-DD or RFC3339. Defaults to now.",
    )
    parser.add_argument(
        "--time-max",
        help="End bound as YYYY-MM-DD or RFC3339. Defaults to now + days-ahead.",
    )
    parser.add_argument("--query", help="Optional free-text search query")
    parser.add_argument(
        "--max-results",
        type=int,
        default=20,
        help="Maximum number of events to return, default: 20",
    )
    parser.add_argument(
        "--include-description",
        action="store_true",
        help="Include event descriptions in the output",
    )
    return parser.parse_args(argv)


def main() -> int:
    args = parse_args(sys.argv[1:])
    try:
        if args.days_ahead < 0:
            raise GoogleAuthError("--days-ahead must be zero or greater.")
        if args.max_results <= 0:
            raise GoogleAuthError("--max-results must be greater than zero.")

        now_utc = datetime.now(UTC)
        time_min_dt = parse_time_boundary(args.time_min, is_end=False) or now_utc
        default_end = now_utc + timedelta(days=args.days_ahead)
        time_max_dt = parse_time_boundary(args.time_max, is_end=True) or default_end
        if time_max_dt <= time_min_dt:
            raise GoogleAuthError("time-max must be after time-min.")
        time_min = time_min_dt.isoformat().replace("+00:00", "Z")
        time_max = time_max_dt.isoformat().replace("+00:00", "Z")

        token = ensure_token(resolve_token_path(), [CALENDAR_SCOPE])
        payload = fetch_events(
            token.access_token,
            calendar_id=args.calendar_id,
            time_min=time_min,
            time_max=time_max,
            query=args.query,
            max_results=args.max_results,
            include_description=args.include_description,
        )
        print(json.dumps(payload, indent=2))
        return 0
    except GoogleAuthError as exc:
        print(
            json.dumps(
                {
                    "error": (
                        f"{exc} Run `python3 scripts/google_calendar_auth.py login` "
                        "if you have not authenticated yet."
                    )
                }
            ),
            file=sys.stderr,
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
