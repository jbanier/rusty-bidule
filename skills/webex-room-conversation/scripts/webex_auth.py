#!/usr/bin/env python3
"""Generate and save a Webex OAuth access token for use with webex_room_message_fetch.

Usage:
    python3 webex_auth.py --client-id <id> --client-secret <secret>

Creates a Webex integration at https://developer.webex.com/my-apps with:
    - Grant type: Authorization Code
    - Redirect URI: http://127.0.0.1:8910/callback
    - Scopes: spark:messages_read spark:rooms_read spark:people_read

Saved token file: ~/.config/rusty-bidule/webex_token.json   (or WEBEX_TOKEN_FILE env var)
The fetch script reads this file automatically when WEBEX_ACCESS_TOKEN is not set.
"""

from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import webbrowser

WEBEX_AUTH_URL = "https://webexapis.com/v1/authorize"
WEBEX_TOKEN_URL = "https://webexapis.com/v1/access_token"
WEBEX_SCOPES = "spark:messages_read spark:rooms_read spark:people_read"
REDIRECT_HOST = "127.0.0.1"
REDIRECT_PORT = 8910
REDIRECT_PATH = "/callback"
CALLBACK_TIMEOUT = 300
_CLOCK_SKEW = 60.0

DEFAULT_TOKEN_FILE = Path.home() / ".config" / "rusty-bidule" / "webex_token.json"


class WebexAuthError(RuntimeError):
    """Raised when authentication cannot complete."""


@dataclass
class WebexToken:
    access_token: str
    token_type: str = "Bearer"
    refresh_token: str | None = None
    expires_at: float | None = None
    scope: str | None = None

    def is_expired(self) -> bool:
        if self.expires_at is None:
            return False
        return (self.expires_at - _CLOCK_SKEW) <= time.time()

    def to_dict(self) -> dict:
        return {
            "access_token": self.access_token,
            "token_type": self.token_type,
            "refresh_token": self.refresh_token,
            "expires_at": self.expires_at,
            "scope": self.scope,
        }

    @classmethod
    def from_dict(cls, data: dict) -> "WebexToken":
        return cls(
            access_token=str(data["access_token"]),
            token_type=str(data.get("token_type") or "Bearer"),
            refresh_token=data.get("refresh_token"),
            expires_at=float(data["expires_at"]) if data.get("expires_at") is not None else None,
            scope=data.get("scope"),
        )

    @classmethod
    def from_token_response(cls, payload: dict) -> "WebexToken":
        expires_at: float | None = None
        expires_in = payload.get("expires_in")
        if expires_in is not None:
            try:
                expires_at = time.time() + float(expires_in)
            except (TypeError, ValueError):
                expires_at = None
        return cls(
            access_token=str(payload["access_token"]),
            token_type=str(payload.get("token_type") or "Bearer"),
            refresh_token=payload.get("refresh_token"),
            expires_at=expires_at,
            scope=payload.get("scope"),
        )


def resolve_token_path() -> Path:
    env_path = os.getenv("WEBEX_TOKEN_FILE", "").strip()
    return Path(env_path) if env_path else DEFAULT_TOKEN_FILE


def load_token() -> WebexToken | None:
    path = resolve_token_path()
    if not path.exists():
        return None
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        return WebexToken.from_dict(data)
    except Exception:  # noqa: BLE001
        return None


def save_token(token: WebexToken) -> Path:
    path = resolve_token_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(token.to_dict(), indent=2), encoding="utf-8")
    try:
        path.chmod(0o600)
    except OSError:
        pass
    return path


def _post_form(url: str, data: dict[str, str]) -> dict:
    body = urllib.parse.urlencode(data).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        body_str = ""
        try:
            body_str = exc.read().decode("utf-8", errors="replace")
        except Exception:  # noqa: BLE001
            pass
        raise WebexAuthError(
            f"Token endpoint returned HTTP {exc.code}: {body_str[:400]}"
        ) from exc
    except urllib.error.URLError as exc:
        raise WebexAuthError(f"Token endpoint request failed: {exc}") from exc


class _CallbackServer:
    """Temporary loopback server that captures the authorization code from the redirect."""

    def __init__(self) -> None:
        self._ready = threading.Event()
        self._result: dict[str, str] = {}
        parent = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802
                parsed = urllib.parse.urlsplit(self.path)
                if parsed.path != REDIRECT_PATH:
                    self.send_response(404)
                    self.end_headers()
                    self.wfile.write(b"Not found")
                    return
                params = urllib.parse.parse_qs(parsed.query)
                parent._result = {k: v[0] for k, v in params.items() if v}
                parent._ready.set()
                success = "error" not in parent._result
                body = (
                    "Webex authentication completed. You can return to the terminal."
                    if success
                    else "Webex authentication failed. You can return to the terminal."
                ).encode("utf-8")
                self.send_response(200)
                self.send_header("Content-Type", "text/plain; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, fmt: str, *args) -> None:  # noqa: A003
                return

        self._server = ThreadingHTTPServer((REDIRECT_HOST, REDIRECT_PORT), Handler)
        self._server.daemon_threads = True
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    def __enter__(self) -> "_CallbackServer":
        self._thread.start()
        return self

    def __exit__(self, *_) -> None:
        self._server.shutdown()
        self._server.server_close()
        self._thread.join(timeout=1)

    @property
    def redirect_uri(self) -> str:
        return f"http://{REDIRECT_HOST}:{REDIRECT_PORT}{REDIRECT_PATH}"

    def wait(self) -> dict[str, str]:
        if not self._ready.wait(CALLBACK_TIMEOUT):
            raise WebexAuthError(
                f"Timed out waiting for Webex OAuth callback after {CALLBACK_TIMEOUT}s."
            )
        return dict(self._result)


def _authorize(client_id: str, client_secret: str) -> WebexToken:
    """Run the Authorization Code flow and return a fresh token."""
    state = secrets.token_urlsafe(24)
    redirect_uri = f"http://{REDIRECT_HOST}:{REDIRECT_PORT}{REDIRECT_PATH}"

    auth_params = urllib.parse.urlencode(
        {
            "response_type": "code",
            "client_id": client_id,
            "redirect_uri": redirect_uri,
            "scope": WEBEX_SCOPES,
            "state": state,
        }
    )
    auth_url = f"{WEBEX_AUTH_URL}?{auth_params}"

    with _CallbackServer() as server:
        print(f"Opening browser for Webex sign-in...\n{auth_url}", flush=True)
        webbrowser.open(auth_url)
        result = server.wait()

    if "error" in result:
        raise WebexAuthError(
            f"Webex authorization failed: {result.get('error')} - {result.get('error_description', '')}"
        )

    returned_state = result.get("state", "")
    if returned_state != state:
        raise WebexAuthError(
            "OAuth state mismatch. Possible CSRF. Aborting."
        )

    code = result.get("code", "")
    if not code:
        raise WebexAuthError("No authorization code received from Webex.")

    payload = _post_form(
        WEBEX_TOKEN_URL,
        {
            "grant_type": "authorization_code",
            "client_id": client_id,
            "client_secret": client_secret,
            "code": code,
            "redirect_uri": redirect_uri,
        },
    )
    return WebexToken.from_token_response(payload)


def refresh_token(token: WebexToken, client_id: str, client_secret: str) -> WebexToken:
    """Exchange a refresh token for a new access token."""
    if not token.refresh_token:
        raise WebexAuthError("No refresh token available. Re-run --login to authenticate again.")

    payload = _post_form(
        WEBEX_TOKEN_URL,
        {
            "grant_type": "refresh_token",
            "client_id": client_id,
            "client_secret": client_secret,
            "refresh_token": token.refresh_token,
        },
    )
    new_token = WebexToken.from_token_response(payload)
    # Preserve the refresh token if the response did not issue a new one
    if not new_token.refresh_token:
        new_token = WebexToken(
            access_token=new_token.access_token,
            token_type=new_token.token_type,
            refresh_token=token.refresh_token,
            expires_at=new_token.expires_at,
            scope=new_token.scope,
        )
    return new_token


def cmd_login(client_id: str, client_secret: str) -> None:
    token = _authorize(client_id, client_secret)
    path = save_token(token)
    print(f"Token saved to {path}", flush=True)
    print("Set WEBEX_ACCESS_TOKEN to use it directly, or the fetch script will load it automatically.")


def cmd_refresh(client_id: str, client_secret: str) -> None:
    existing = load_token()
    if existing is None:
        raise WebexAuthError(
            "No saved token found. Run --login first."
        )
    new_token = refresh_token(existing, client_id, client_secret)
    path = save_token(new_token)
    print(f"Token refreshed and saved to {path}", flush=True)


def cmd_show() -> None:
    token = load_token()
    if token is None:
        raise WebexAuthError(
            f"No token found at {resolve_token_path()}. Run --login first."
        )
    expires_str = (
        time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(token.expires_at))
        if token.expires_at
        else "unknown"
    )
    print(
        json.dumps(
            {
                "token_file": str(resolve_token_path()),
                "token_type": token.token_type,
                "expires_at": expires_str,
                "has_refresh_token": bool(token.refresh_token),
                "is_expired": token.is_expired(),
                "scope": token.scope,
            },
            indent=2,
        )
    )


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Manage a Webex OAuth token for use with webex_room_message_fetch.\n\n"
            "Create a Webex integration at https://developer.webex.com/my-apps with:\n"
            f"  Redirect URI: http://{REDIRECT_HOST}:{REDIRECT_PORT}{REDIRECT_PATH}\n"
            f"  Scopes: {WEBEX_SCOPES}"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    sub = parser.add_subparsers(dest="command", required=True)

    login_p = sub.add_parser("login", help="Authorize via browser and save token")
    login_p.add_argument("--client-id", required=True, help="Webex integration client ID")
    login_p.add_argument("--client-secret", required=True, help="Webex integration client secret")

    refresh_p = sub.add_parser("refresh", help="Refresh an existing saved token")
    refresh_p.add_argument("--client-id", required=True, help="Webex integration client ID")
    refresh_p.add_argument("--client-secret", required=True, help="Webex integration client secret")

    sub.add_parser("show", help="Show saved token metadata (no secrets printed)")

    return parser.parse_args(argv)


def main() -> int:
    args = parse_args(sys.argv[1:])
    try:
        if args.command == "login":
            cmd_login(args.client_id, args.client_secret)
        elif args.command == "refresh":
            cmd_refresh(args.client_id, args.client_secret)
        else:
            cmd_show()
        return 0
    except WebexAuthError as exc:
        print(json.dumps({"error": str(exc)}), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
