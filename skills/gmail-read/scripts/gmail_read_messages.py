#!/usr/bin/env python3
"""Read Gmail messages with a saved read-only token."""

from __future__ import annotations

import argparse
import base64
from datetime import UTC, datetime
import html
import json
from pathlib import Path
import re
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

GMAIL_SCOPE = "https://www.googleapis.com/auth/gmail.readonly"
GMAIL_API_BASE = "https://gmail.googleapis.com/gmail/v1/users/me"
DEFAULT_TOKEN_FILE = CONFIG_DIR / "gmail_token.json"


def resolve_token_path() -> Path:
    import os

    raw = os.getenv("GMAIL_TOKEN_FILE", "").strip()
    return Path(raw).expanduser() if raw else DEFAULT_TOKEN_FILE


def parse_label_ids(raw: str | None) -> list[str]:
    if not raw:
        return []
    return [part.strip() for part in raw.split(",") if part.strip()]


def decode_base64url(raw: str | None) -> str:
    if not raw:
        return ""
    padded = raw + "=" * (-len(raw) % 4)
    decoded = base64.urlsafe_b64decode(padded.encode("utf-8"))
    return decoded.decode("utf-8", errors="replace")


def strip_html(raw: str) -> str:
    text = re.sub(r"(?is)<(script|style).*?>.*?</\\1>", " ", raw)
    text = re.sub(r"(?s)<[^>]+>", " ", text)
    text = html.unescape(text)
    text = re.sub(r"[ \t]+", " ", text)
    text = re.sub(r"\n\s+\n", "\n\n", text)
    return text.strip()


def headers_map(payload: dict[str, Any]) -> dict[str, str]:
    headers = {}
    for header in (payload.get("headers") or []):
        if not isinstance(header, dict):
            continue
        name = str(header.get("name") or "").strip().lower()
        value = str(header.get("value") or "").strip()
        if name and value and name not in headers:
            headers[name] = value
    return headers


def extract_body(part: dict[str, Any]) -> str:
    mime_type = str(part.get("mimeType") or "")
    body_data = ((part.get("body") or {}).get("data")) if isinstance(part.get("body"), dict) else None
    parts = part.get("parts") or []

    if mime_type == "text/plain" and body_data:
        return decode_base64url(body_data)

    plain_chunks = []
    html_chunks = []
    for child in parts:
        if not isinstance(child, dict):
            continue
        child_mime = str(child.get("mimeType") or "")
        child_body = extract_body(child)
        if not child_body:
            continue
        if child_mime == "text/plain":
            plain_chunks.append(child_body)
        else:
            html_chunks.append(child_body)

    if plain_chunks:
        return "\n".join(chunk.strip() for chunk in plain_chunks if chunk.strip()).strip()

    if mime_type == "text/html" and body_data:
        return strip_html(decode_base64url(body_data))

    if html_chunks:
        return "\n".join(strip_html(chunk) for chunk in html_chunks if chunk.strip()).strip()

    if body_data:
        return decode_base64url(body_data)
    return ""


def has_attachments(part: dict[str, Any]) -> bool:
    filename = str(part.get("filename") or "").strip()
    body = part.get("body") or {}
    if filename and isinstance(body, dict) and body.get("attachmentId"):
        return True
    for child in part.get("parts") or []:
        if isinstance(child, dict) and has_attachments(child):
            return True
    return False


def compact_message(payload: dict[str, Any], *, include_body: bool, body_max_chars: int) -> dict[str, Any]:
    message_payload = payload.get("payload") or {}
    header_values = headers_map(message_payload)
    internal_date_raw = str(payload.get("internalDate") or "").strip()
    internal_date = None
    if internal_date_raw.isdigit():
        internal_date = datetime.fromtimestamp(int(internal_date_raw) / 1000, tz=UTC).isoformat().replace(
            "+00:00", "Z"
        )

    message = {
        "id": payload.get("id"),
        "thread_id": payload.get("threadId"),
        "label_ids": payload.get("labelIds") or [],
        "snippet": payload.get("snippet"),
        "internal_date": internal_date,
        "size_estimate": payload.get("sizeEstimate"),
        "from": header_values.get("from"),
        "to": header_values.get("to"),
        "cc": header_values.get("cc"),
        "bcc": header_values.get("bcc"),
        "subject": header_values.get("subject"),
        "date": header_values.get("date"),
        "message_id": header_values.get("message-id"),
        "has_attachments": has_attachments(message_payload) if isinstance(message_payload, dict) else False,
    }
    if include_body and isinstance(message_payload, dict):
        body_text = extract_body(message_payload)
        if body_max_chars > 0 and len(body_text) > body_max_chars:
            body_text = f"{body_text[:body_max_chars]}...(truncated)"
        message["body_text"] = body_text
    return message


def list_message_ids(access_token: str, *, query: str | None, label_ids: list[str], max_results: int) -> list[str]:
    url = f"{GMAIL_API_BASE}/messages"
    page_token: str | None = None
    ids: list[str] = []

    while len(ids) < max_results:
        payload = request_json(
            url,
            access_token,
            params={
                "maxResults": min(max_results - len(ids), 100),
                "q": query or None,
                "labelIds": label_ids or None,
                "pageToken": page_token,
            },
        )
        messages = payload.get("messages") or []
        if not isinstance(messages, list):
            raise GoogleAuthError("Gmail list response had an unexpected format.")
        for item in messages:
            if isinstance(item, dict) and item.get("id"):
                ids.append(str(item["id"]))
                if len(ids) >= max_results:
                    break
        page_token = payload.get("nextPageToken")
        if not page_token:
            break

    return ids


def get_message(access_token: str, message_id: str, *, include_body: bool) -> dict[str, Any]:
    encoded_id = urllib.parse.quote(message_id, safe="")
    return request_json(
        f"{GMAIL_API_BASE}/messages/{encoded_id}",
        access_token,
        params={
            "format": "full" if include_body else "metadata",
            "metadataHeaders": ["From", "To", "Cc", "Bcc", "Subject", "Date", "Message-ID"],
        },
    )


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Read Gmail messages with a saved read-only token.\n\n"
            "Examples:\n"
            "  --query 'is:unread newer_than:7d'\n"
            "  --label-ids INBOX,IMPORTANT --max-results 5\n"
            "  --query 'from:alice@example.com' --include-body\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--query", help="Gmail search query, for example 'is:unread newer_than:7d'")
    parser.add_argument(
        "--label-ids",
        help="Comma-separated Gmail label IDs such as INBOX,UNREAD",
    )
    parser.add_argument(
        "--max-results",
        type=int,
        default=10,
        help="Maximum number of messages to return, default: 10",
    )
    parser.add_argument(
        "--include-body",
        action="store_true",
        help="Include decoded message body text",
    )
    parser.add_argument(
        "--body-max-chars",
        type=int,
        default=4000,
        help="Maximum characters of body text per message when --include-body is set",
    )
    return parser.parse_args(argv)


def main() -> int:
    args = parse_args(sys.argv[1:])
    try:
        if args.max_results <= 0:
            raise GoogleAuthError("--max-results must be greater than zero.")
        if args.body_max_chars < 0:
            raise GoogleAuthError("--body-max-chars must be zero or greater.")

        label_ids = parse_label_ids(args.label_ids)
        token = ensure_token(resolve_token_path(), [GMAIL_SCOPE])
        message_ids = list_message_ids(
            token.access_token,
            query=args.query,
            label_ids=label_ids,
            max_results=args.max_results,
        )
        messages = [
            compact_message(
                get_message(token.access_token, message_id, include_body=args.include_body),
                include_body=args.include_body,
                body_max_chars=args.body_max_chars,
            )
            for message_id in message_ids
        ]
        print(
            json.dumps(
                {
                    "query": args.query,
                    "label_ids": label_ids,
                    "max_results": args.max_results,
                    "include_body": args.include_body,
                    "total_messages": len(messages),
                    "messages": messages,
                },
                indent=2,
            )
        )
        return 0
    except GoogleAuthError as exc:
        print(
            json.dumps(
                {
                    "error": (
                        f"{exc} Run `python3 skills/gmail-read/scripts/gmail_auth.py login` "
                        "if you have not authenticated yet."
                    )
                }
            ),
            file=sys.stderr,
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
