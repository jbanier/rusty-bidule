#!/usr/bin/env python3
"""Shared Google OAuth helpers for local rusty-bidule skills."""

from __future__ import annotations

import base64
from dataclasses import dataclass
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import secrets
import threading
import time
from typing import Any, Iterable
import urllib.error
import urllib.parse
import urllib.request
import webbrowser

CONFIG_DIR = Path.home() / ".config" / "rusty-bidule"
DEFAULT_CLIENT_FILE = CONFIG_DIR / "google-oauth-client.json"


class GoogleAuthError(RuntimeError):
    """Raised when the Google OAuth workflow cannot complete."""


def resolve_client_file(path: str | None = None) -> Path:
    raw = (path or "").strip()
    if raw:
        return Path(raw).expanduser()
    env_value = os.getenv("GOOGLE_OAUTH_CLIENT_FILE", "").strip()
    if env_value:
        return Path(env_value).expanduser()
    return DEFAULT_CLIENT_FILE


def normalize_scopes(scopes: Iterable[str]) -> tuple[str, ...]:
    return tuple(sorted({scope.strip() for scope in scopes if scope and scope.strip()}))


@dataclass(frozen=True)
class GoogleOAuthClient:
    client_id: str
    client_secret: str
    auth_uri: str
    token_uri: str

    @classmethod
    def from_file(cls, path: Path) -> "GoogleOAuthClient":
        if not path.exists():
            raise GoogleAuthError(
                f"OAuth client file not found at {path}. Download a Desktop OAuth client JSON from Google Cloud and pass --credentials-file or set GOOGLE_OAUTH_CLIENT_FILE."
            )
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            raise GoogleAuthError(f"Failed to parse OAuth client file {path}: {exc}") from exc

        root = payload.get("installed") or payload.get("web")
        if not isinstance(root, dict):
            raise GoogleAuthError(
                f"OAuth client file {path} must contain an 'installed' or 'web' object."
            )

        client_id = str(root.get("client_id") or "").strip()
        auth_uri = str(root.get("auth_uri") or "").strip()
        token_uri = str(root.get("token_uri") or "").strip()
        client_secret = str(root.get("client_secret") or "").strip()
        if not client_id or not auth_uri or not token_uri:
            raise GoogleAuthError(
                f"OAuth client file {path} is missing one of client_id, auth_uri, or token_uri."
            )

        return cls(
            client_id=client_id,
            client_secret=client_secret,
            auth_uri=auth_uri,
            token_uri=token_uri,
        )


@dataclass(frozen=True)
class GoogleOAuthToken:
    access_token: str
    token_type: str
    refresh_token: str | None
    expires_at: float | None
    scopes: tuple[str, ...]
    client_id: str
    client_secret: str
    auth_uri: str
    token_uri: str

    def is_expired(self, skew_seconds: int = 60) -> bool:
        return self.expires_at is not None and time.time() >= self.expires_at - skew_seconds

    def has_scopes(self, required_scopes: Iterable[str]) -> bool:
        required = set(normalize_scopes(required_scopes))
        return required.issubset(set(self.scopes))

    def to_dict(self) -> dict[str, Any]:
        return {
            "access_token": self.access_token,
            "token_type": self.token_type,
            "refresh_token": self.refresh_token,
            "expires_at": self.expires_at,
            "scopes": list(self.scopes),
            "scope": " ".join(self.scopes),
            "client_id": self.client_id,
            "client_secret": self.client_secret,
            "auth_uri": self.auth_uri,
            "token_uri": self.token_uri,
        }

    @classmethod
    def from_dict(cls, payload: dict[str, Any]) -> "GoogleOAuthToken":
        scopes = payload.get("scopes")
        if isinstance(scopes, list):
            normalized_scopes = normalize_scopes(
                str(scope) for scope in scopes if str(scope).strip()
            )
        else:
            scope_value = str(payload.get("scope") or "").strip()
            normalized_scopes = normalize_scopes(scope_value.split())

        access_token = str(payload.get("access_token") or "").strip()
        token_type = str(payload.get("token_type") or "Bearer").strip() or "Bearer"
        if not access_token:
            raise GoogleAuthError("Saved Google token is missing access_token.")

        expires_at_raw = payload.get("expires_at")
        expires_at = float(expires_at_raw) if expires_at_raw is not None else None

        return cls(
            access_token=access_token,
            token_type=token_type,
            refresh_token=str(payload.get("refresh_token") or "").strip() or None,
            expires_at=expires_at,
            scopes=normalized_scopes,
            client_id=str(payload.get("client_id") or "").strip(),
            client_secret=str(payload.get("client_secret") or "").strip(),
            auth_uri=str(payload.get("auth_uri") or "").strip(),
            token_uri=str(payload.get("token_uri") or "").strip(),
        )

    @classmethod
    def from_token_response(
        cls,
        payload: dict[str, Any],
        client: GoogleOAuthClient,
        *,
        refresh_token: str | None = None,
        scopes: Iterable[str] = (),
    ) -> "GoogleOAuthToken":
        access_token = str(payload.get("access_token") or "").strip()
        if not access_token:
            raise GoogleAuthError("Google token response did not include access_token.")

        expires_at = None
        expires_in = payload.get("expires_in")
        if expires_in is not None:
            try:
                expires_at = time.time() + max(int(expires_in) - 60, 0)
            except (TypeError, ValueError):
                expires_at = None

        response_scopes = normalize_scopes(str(payload.get("scope") or "").split())
        normalized_scopes = response_scopes or normalize_scopes(scopes)

        return cls(
            access_token=access_token,
            token_type=str(payload.get("token_type") or "Bearer").strip() or "Bearer",
            refresh_token=str(payload.get("refresh_token") or "").strip() or refresh_token,
            expires_at=expires_at,
            scopes=normalized_scopes,
            client_id=client.client_id,
            client_secret=client.client_secret,
            auth_uri=client.auth_uri,
            token_uri=client.token_uri,
        )


class _LoopbackHTTPServer(HTTPServer):
    auth_code: str | None
    auth_error: str | None
    returned_state: str | None
    done_event: threading.Event


class _LoopbackHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802
        parsed = urllib.parse.urlparse(self.path)
        if parsed.path != "/callback":
            self.send_error(404)
            return

        params = urllib.parse.parse_qs(parsed.query)
        self.server.auth_code = params.get("code", [None])[0]  # type: ignore[attr-defined]
        self.server.auth_error = params.get("error", [None])[0]  # type: ignore[attr-defined]
        self.server.returned_state = params.get("state", [None])[0]  # type: ignore[attr-defined]
        self.server.done_event.set()  # type: ignore[attr-defined]

        if self.server.auth_error:  # type: ignore[attr-defined]
            body = "<html><body><h1>Google authorization failed.</h1><p>You can close this tab.</p></body></html>"
        else:
            body = "<html><body><h1>Google authorization complete.</h1><p>You can close this tab and return to the terminal.</p></body></html>"

        encoded = body.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def log_message(self, format: str, *args: Any) -> None:  # noqa: A003
        return


def _post_form(url: str, data: dict[str, str]) -> dict[str, Any]:
    encoded = urllib.parse.urlencode(data).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=encoded,
        headers={
            "Content-Type": "application/x-www-form-urlencoded",
            "Accept": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        body = ""
        try:
            body = exc.read().decode("utf-8", errors="replace")
        except Exception:  # noqa: BLE001
            body = ""
        raise GoogleAuthError(
            f"Google token endpoint returned HTTP {exc.code}: {body[:400]}"
        ) from exc
    except urllib.error.URLError as exc:
        raise GoogleAuthError(f"Google token request failed: {exc}") from exc


def save_token(path: Path, token: GoogleOAuthToken) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(token.to_dict(), indent=2), encoding="utf-8")
    try:
        path.chmod(0o600)
    except OSError:
        pass
    return path


def load_token(path: Path) -> GoogleOAuthToken | None:
    if not path.exists():
        return None
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise GoogleAuthError(f"Failed to parse saved Google token file {path}: {exc}") from exc
    if not isinstance(payload, dict):
        raise GoogleAuthError(f"Saved Google token file {path} must contain a JSON object.")
    return GoogleOAuthToken.from_dict(payload)


def token_metadata(path: Path, token: GoogleOAuthToken) -> dict[str, Any]:
    expires_at = (
        time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(token.expires_at))
        if token.expires_at
        else None
    )
    return {
        "token_file": str(path),
        "token_type": token.token_type,
        "expires_at": expires_at,
        "has_refresh_token": bool(token.refresh_token),
        "is_expired": token.is_expired(),
        "scopes": list(token.scopes),
    }


def _code_verifier() -> str:
    return base64.urlsafe_b64encode(secrets.token_bytes(64)).decode("utf-8").rstrip("=")


def _code_challenge(verifier: str) -> str:
    digest = hashlib.sha256(verifier.encode("utf-8")).digest()
    return base64.urlsafe_b64encode(digest).decode("utf-8").rstrip("=")


def authorize_user(
    client: GoogleOAuthClient,
    scopes: Iterable[str],
    *,
    open_browser: bool = True,
    timeout_seconds: int = 300,
) -> GoogleOAuthToken:
    normalized_scopes = normalize_scopes(scopes)
    if not normalized_scopes:
        raise GoogleAuthError("At least one OAuth scope is required.")

    verifier = _code_verifier()
    challenge = _code_challenge(verifier)
    state = secrets.token_urlsafe(24)

    server = _LoopbackHTTPServer(("127.0.0.1", 0), _LoopbackHandler)
    server.auth_code = None
    server.auth_error = None
    server.returned_state = None
    server.done_event = threading.Event()
    port = int(server.server_address[1])
    redirect_uri = f"http://127.0.0.1:{port}/callback"

    params = urllib.parse.urlencode(
        {
            "client_id": client.client_id,
            "redirect_uri": redirect_uri,
            "response_type": "code",
            "scope": " ".join(normalized_scopes),
            "access_type": "offline",
            "prompt": "consent",
            "include_granted_scopes": "true",
            "state": state,
            "code_challenge": challenge,
            "code_challenge_method": "S256",
        }
    )
    auth_url = f"{client.auth_uri}?{params}"

    thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.2}, daemon=True)
    thread.start()
    try:
        print(f"Open this URL to authorize the skill:\n{auth_url}", flush=True)
        if open_browser:
            webbrowser.open(auth_url, new=1, autoraise=True)

        if not server.done_event.wait(timeout_seconds):
            raise GoogleAuthError(
                f"Timed out waiting for Google OAuth callback after {timeout_seconds}s."
            )
    finally:
        server.shutdown()
        thread.join(timeout=5)
        server.server_close()

    if server.auth_error:
        raise GoogleAuthError(f"Google OAuth authorization failed: {server.auth_error}")
    if server.returned_state != state:
        raise GoogleAuthError("Google OAuth callback state mismatch.")
    if not server.auth_code:
        raise GoogleAuthError("Google OAuth callback did not include an authorization code.")

    payload = {
        "grant_type": "authorization_code",
        "code": server.auth_code,
        "client_id": client.client_id,
        "redirect_uri": redirect_uri,
        "code_verifier": verifier,
    }
    if client.client_secret:
        payload["client_secret"] = client.client_secret
    token_response = _post_form(client.token_uri, payload)
    return GoogleOAuthToken.from_token_response(
        token_response,
        client,
        scopes=normalized_scopes,
    )


def refresh_token(token: GoogleOAuthToken) -> GoogleOAuthToken:
    if not token.refresh_token:
        raise GoogleAuthError(
            "Saved Google token does not include a refresh token. Re-run the login flow."
        )
    client = GoogleOAuthClient(
        client_id=token.client_id,
        client_secret=token.client_secret,
        auth_uri=token.auth_uri,
        token_uri=token.token_uri,
    )
    payload = {
        "grant_type": "refresh_token",
        "refresh_token": token.refresh_token,
        "client_id": token.client_id,
    }
    if token.client_secret:
        payload["client_secret"] = token.client_secret
    token_response = _post_form(token.token_uri, payload)
    return GoogleOAuthToken.from_token_response(
        token_response,
        client,
        refresh_token=token.refresh_token,
        scopes=token.scopes,
    )


def ensure_token(path: Path, required_scopes: Iterable[str]) -> GoogleOAuthToken:
    token = load_token(path)
    if token is None:
        raise GoogleAuthError(f"No saved Google token found at {path}.")

    normalized_scopes = normalize_scopes(required_scopes)
    if normalized_scopes and not token.has_scopes(normalized_scopes):
        raise GoogleAuthError(
            f"Saved token at {path} is missing required scopes: {', '.join(normalized_scopes)}. Re-run the login flow."
        )

    if token.is_expired():
        token = refresh_token(token)
        save_token(path, token)

    return token


def request_json(
    url: str,
    access_token: str,
    *,
    params: dict[str, Any] | None = None,
) -> dict[str, Any]:
    if params:
        encoded_params: list[tuple[str, str]] = []
        for key, value in params.items():
            if isinstance(value, (list, tuple)):
                encoded_params.extend(
                    (key, str(item)) for item in value if item is not None
                )
            elif value is None:
                continue
            else:
                encoded_params.append((key, str(value)))
        if encoded_params:
            encoded = urllib.parse.urlencode(encoded_params, doseq=True)
            separator = "&" if "?" in url else "?"
            url = f"{url}{separator}{encoded}"

    request = urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {access_token}",
            "Accept": "application/json",
        },
        method="GET",
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        body = ""
        try:
            body = exc.read().decode("utf-8", errors="replace")
        except Exception:  # noqa: BLE001
            body = ""
        raise GoogleAuthError(
            f"Google API request failed with HTTP {exc.code} for {url}: {body[:400]}"
        ) from exc
    except urllib.error.URLError as exc:
        raise GoogleAuthError(f"Google API request failed for {url}: {exc}") from exc
