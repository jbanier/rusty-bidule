#!/usr/bin/env python3
"""Fetch Webex room messages by room title and EU date interval."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timedelta
import json
import os
import sys
from typing import Any
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from zoneinfo import ZoneInfo

WEBEX_API_BASE = "https://webexapis.com/v1"
DEFAULT_PAGE_SIZE = 100

_DEFAULT_TOKEN_FILE = Path.home() / ".config" / "rusty-bidule" / "webex_token.json"


class WebexFetchError(RuntimeError):
    """Raised when the Webex fetch workflow cannot complete."""


def _load_token_from_file() -> str | None:
    """Return an access token from the saved token file, if present and readable."""
    env_path = os.getenv("WEBEX_TOKEN_FILE", "").strip()
    token_file = Path(env_path) if env_path else _DEFAULT_TOKEN_FILE
    if not token_file.exists():
        return None
    try:
        data = json.loads(token_file.read_text(encoding="utf-8"))
        token = str(data.get("access_token") or "").strip()
        return token if token else None
    except Exception:  # noqa: BLE001
        return None


@dataclass(frozen=True)
class TimeWindow:
    since_iso: str
    until_iso: str
    timezone: str


def parse_eu_date(raw: str, field_name: str) -> datetime:
    """Parse a DD/MM/YYYY date into a naive datetime at midnight."""
    try:
        return datetime.strptime(raw, "%d/%m/%Y")
    except ValueError as exc:  # pragma: no cover - covered by caller-level tests
        raise WebexFetchError(
            f"Invalid {field_name} date '{raw}'. Expected format DD/MM/YYYY, for example 26/03/2026."
        ) from exc


def _validate_hour(raw: int | None, field_name: str) -> int | None:
    if raw is None:
        return None
    if 0 <= raw <= 23:
        return raw
    raise WebexFetchError(f"{field_name} must be between 0 and 23.")


def build_time_window(
    since_raw: str,
    until_raw: str | None,
    timezone_name: str,
    hour_start: int | None = None,
    hour_end: int | None = None,
) -> TimeWindow:
    """Convert EU dates to inclusive ISO8601 range in the selected timezone."""
    try:
        tz = ZoneInfo(timezone_name)
    except Exception as exc:  # noqa: BLE001
        raise WebexFetchError(f"Invalid timezone '{timezone_name}'.") from exc

    since_date = parse_eu_date(since_raw, "since")
    start_hour = _validate_hour(hour_start, "hour_start")
    end_hour = _validate_hour(hour_end, "hour_end")
    since_dt = since_date.replace(hour=start_hour or 0, tzinfo=tz)

    if until_raw:
        until_date = parse_eu_date(until_raw, "until")
        if until_date < since_date:
            raise WebexFetchError("until date must be on or after since date.")
        until_dt = until_date.replace(
            hour=end_hour if end_hour is not None else 23,
            minute=59,
            second=59,
            microsecond=999999,
            tzinfo=tz,
        )
    else:
        until_dt = datetime.now(tz)
        if end_hour is not None:
            until_dt = since_date.replace(
                hour=end_hour,
                minute=59,
                second=59,
                microsecond=999999,
                tzinfo=tz,
            )

    if until_dt < since_dt:
        raise WebexFetchError("hour_end must be on or after hour_start within the selected range.")

    return TimeWindow(
        since_iso=since_dt.isoformat(),
        until_iso=until_dt.isoformat(),
        timezone=timezone_name,
    )


def _request_json(url: str, token: str) -> tuple[dict[str, Any], dict[str, str]]:
    req = urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))
            headers = {key.lower(): value for key, value in response.headers.items()}
            return payload, headers
    except urllib.error.HTTPError as exc:
        body = ""
        try:
            body = exc.read().decode("utf-8", errors="replace")
        except Exception:  # noqa: BLE001
            body = ""
        raise WebexFetchError(
            f"Webex API request failed with HTTP {exc.code} for {url}. Response: {body[:400]}"
        ) from exc
    except urllib.error.URLError as exc:
        raise WebexFetchError(f"Webex API request failed for {url}: {exc}") from exc


def _next_link(link_header: str | None) -> str | None:
    if not link_header:
        return None
    parts = [part.strip() for part in link_header.split(",") if part.strip()]
    for part in parts:
        if 'rel="next"' not in part:
            continue
        if part.startswith("<") and ">" in part:
            return part[1 : part.index(">")]
    return None


def find_room_by_name(room_name: str, token: str) -> dict[str, Any]:
    """Resolve a room ID from room title, failing on 0 or multiple matches."""
    encoded_title = urllib.parse.quote(room_name)
    url = f"{WEBEX_API_BASE}/rooms?max=1000&title={encoded_title}"
    payload, _headers = _request_json(url, token)
    items = payload.get("items", [])

    if not isinstance(items, list):
        raise WebexFetchError("Unexpected rooms API response format.")

    exact_matches = [item for item in items if str(item.get("title", "")) == room_name]
    if len(exact_matches) == 1:
        return exact_matches[0]

    if len(exact_matches) > 1:
        candidates = [
            {"id": item.get("id"), "title": item.get("title")}
            for item in exact_matches[:10]
        ]
        raise WebexFetchError(
            f"Room name '{room_name}' is ambiguous. Multiple exact matches found: {json.dumps(candidates)}"
        )

    ci_matches = [
        item for item in items if str(item.get("title", "")).casefold() == room_name.casefold()
    ]
    if len(ci_matches) == 1:
        return ci_matches[0]

    if len(ci_matches) > 1:
        candidates = [
            {"id": item.get("id"), "title": item.get("title")}
            for item in ci_matches[:10]
        ]
        raise WebexFetchError(
            f"Room name '{room_name}' is ambiguous. Multiple case-insensitive matches found: {json.dumps(candidates)}"
        )

    raise WebexFetchError(f"No Webex room found with title '{room_name}'.")


def fetch_messages(room_id: str, token: str, window: TimeWindow) -> list[dict[str, Any]]:
    """Fetch all messages for a room in an interval with pagination."""
    query = urllib.parse.urlencode(
        {
            "roomId": room_id,
            "max": str(DEFAULT_PAGE_SIZE),
            "since": window.since_iso,
            "before": window.until_iso,
        }
    )
    next_url = f"{WEBEX_API_BASE}/messages?{query}"
    all_messages: list[dict[str, Any]] = []

    while next_url:
        payload, headers = _request_json(next_url, token)
        items = payload.get("items", [])
        if not isinstance(items, list):
            raise WebexFetchError("Unexpected messages API response format.")
        all_messages.extend(item for item in items if isinstance(item, dict))
        next_url = _next_link(headers.get("link"))

    return all_messages


def enrich_person_names(messages: list[dict[str, Any]], token: str) -> None:
    """Attach person display names to messages when personId is present."""
    person_ids = sorted(
        {str(message.get("personId")) for message in messages if message.get("personId")}
    )
    cache: dict[str, str] = {}

    for person_id in person_ids:
        encoded = urllib.parse.quote(person_id)
        url = f"{WEBEX_API_BASE}/people/{encoded}"
        try:
            payload, _headers = _request_json(url, token)
        except WebexFetchError:
            continue
        display_name = str(payload.get("displayName") or payload.get("nickName") or "")
        if display_name:
            cache[person_id] = display_name

    for message in messages:
        person_id = message.get("personId")
        if person_id and str(person_id) in cache:
            message["person_display_name"] = cache[str(person_id)]


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Fetch all messages from a Webex room for a DD/MM/YYYY interval."
    )
    parser.add_argument(
        "--room",
        "--room-name",
        dest="room",
        required=True,
        help="Webex room title (exact human name)",
    )
    parser.add_argument(
        "--since",
        "--date-start",
        dest="since",
        required=True,
        help="Start date in DD/MM/YYYY",
    )
    parser.add_argument("--until", "--date-end", dest="until", help="End date in DD/MM/YYYY (inclusive)")
    parser.add_argument("--hour-start", type=int, help="Optional start hour in 24h format (0-23)")
    parser.add_argument("--hour-end", type=int, help="Optional end hour in 24h format (0-23)")
    parser.add_argument(
        "--timezone",
        default="UTC",
        help="Timezone used to interpret dates (default: UTC)",
    )
    parser.add_argument(
        "--include-person-names",
        action="store_true",
        help="Resolve personId values to display names using the People API",
    )
    return parser.parse_args(argv)


def run(argv: list[str]) -> int:
    args = parse_args(argv)
    token = os.getenv("WEBEX_ACCESS_TOKEN", "").strip()
    if not token:
        token = _load_token_from_file() or ""
    if not token:
        raise WebexFetchError(
            "No Webex token found. Set WEBEX_ACCESS_TOKEN or run: "
            "python3 webex_auth.py login --client-id <id> --client-secret <secret>"
        )

    window = build_time_window(
        args.since,
        args.until,
        args.timezone,
        hour_start=args.hour_start,
        hour_end=args.hour_end,
    )
    room = find_room_by_name(args.room, token)
    room_id = str(room.get("id") or "")
    if not room_id:
        raise WebexFetchError("Resolved room has no id in API response.")

    messages = fetch_messages(room_id, token, window)
    if args.include_person_names:
        enrich_person_names(messages, token)

    output = {
        "query": {
            "room": args.room,
            "room_id": room_id,
            "since": window.since_iso,
            "until": window.until_iso,
            "timezone": window.timezone,
            "hour_start": args.hour_start,
            "hour_end": args.hour_end,
            "include_person_names": bool(args.include_person_names),
        },
        "room": {
            "id": room_id,
            "title": room.get("title"),
            "type": room.get("type"),
            "isLocked": room.get("isLocked"),
        },
        "total_messages": len(messages),
        "messages": messages,
    }
    print(json.dumps(output, indent=2, ensure_ascii=True))
    return 0


def main() -> int:
    try:
        return run(sys.argv[1:])
    except WebexFetchError as exc:
        print(json.dumps({"error": str(exc)}), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
