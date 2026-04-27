#!/usr/bin/env python3
"""Manage a Gmail read-only OAuth token."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_google_common"
sys.path.insert(0, str(SHARED_DIR))

from google_workspace_auth import (  # noqa: E402
    CONFIG_DIR,
    GoogleAuthError,
    GoogleOAuthClient,
    authorize_user,
    load_token,
    refresh_token,
    resolve_client_file,
    save_token,
    token_metadata,
)

GMAIL_SCOPE = "https://www.googleapis.com/auth/gmail.readonly"
DEFAULT_TOKEN_FILE = CONFIG_DIR / "gmail_token.json"


def resolve_token_path() -> Path:
    import os

    raw = os.getenv("GMAIL_TOKEN_FILE", "").strip()
    return Path(raw).expanduser() if raw else DEFAULT_TOKEN_FILE


def cmd_login(credentials_file: str | None, open_browser: bool) -> None:
    client = GoogleOAuthClient.from_file(resolve_client_file(credentials_file))
    token = authorize_user(client, [GMAIL_SCOPE], open_browser=open_browser)
    path = save_token(resolve_token_path(), token)
    print(f"Token saved to {path}", flush=True)
    if not token.refresh_token:
        print(
            "Warning: Google did not return a refresh token. Re-run login if automatic refresh fails later.",
            flush=True,
        )


def cmd_refresh() -> None:
    token = load_token(resolve_token_path())
    if token is None:
        raise GoogleAuthError(
            f"No saved token found at {resolve_token_path()}. Run the login command first."
        )
    refreshed = refresh_token(token)
    path = save_token(resolve_token_path(), refreshed)
    print(f"Token refreshed and saved to {path}", flush=True)


def cmd_show() -> None:
    token = load_token(resolve_token_path())
    if token is None:
        raise GoogleAuthError(
            f"No saved token found at {resolve_token_path()}. Run the login command first."
        )
    print(json.dumps(token_metadata(resolve_token_path(), token), indent=2))


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Manage the Gmail read-only token used by the gmail-read skill.\n\n"
            "Create a Google Cloud Desktop OAuth client, download the JSON, and either:\n"
            f"  1. save it to {CONFIG_DIR / 'google-oauth-client.json'}\n"
            "  2. or pass --credentials-file explicitly\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    sub = parser.add_subparsers(dest="command", required=True)

    login = sub.add_parser("login", help="Authorize via browser and save a token")
    login.add_argument(
        "--credentials-file",
        help="Path to the downloaded Google OAuth client JSON",
    )
    login.add_argument(
        "--no-open-browser",
        action="store_true",
        help="Print the authorization URL without launching a browser",
    )

    sub.add_parser("refresh", help="Refresh the saved token")
    sub.add_parser("show", help="Show saved token metadata")
    return parser.parse_args(argv)


def main() -> int:
    args = parse_args(sys.argv[1:])
    try:
        if args.command == "login":
            cmd_login(args.credentials_file, open_browser=not args.no_open_browser)
        elif args.command == "refresh":
            cmd_refresh()
        else:
            cmd_show()
        return 0
    except GoogleAuthError as exc:
        print(json.dumps({"error": str(exc)}), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
