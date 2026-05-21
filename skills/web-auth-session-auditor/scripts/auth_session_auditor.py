#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
from http.cookies import SimpleCookie
import json
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, parse_json_arg, split_items  # noqa: E402


def decode_jwt(token: str) -> dict[str, object]:
    parts = token.split(".")
    if len(parts) < 2:
        return {"valid_shape": False}
    decoded = {"valid_shape": True}
    for label, part in [("header", parts[0]), ("claims", parts[1])]:
        padded = part + "=" * (-len(part) % 4)
        try:
            decoded[label] = json.loads(base64.urlsafe_b64decode(padded).decode("utf-8"))
        except Exception as exc:
            decoded[label] = {"decode_error": str(exc)}
    return decoded


def analyze_cookie_header(raw: str) -> list[dict[str, object]]:
    findings: list[dict[str, object]] = []
    for value in split_items(raw):
        cookie = SimpleCookie()
        cookie.load(value)
        for name, morsel in cookie.items():
            missing = [attr for attr in ["secure", "httponly", "samesite"] if not morsel[attr]]
            findings.append({"name": name, "missing_attributes": missing, "path": morsel["path"], "domain": morsel["domain"]})
    return findings


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--set-cookie", default="")
    parser.add_argument("--jwt", default="")
    parser.add_argument("--csrf-token-names", default="")
    parser.add_argument("--observations-json", default="[]")
    parser.add_argument("--oauth-notes", default="")
    args = parser.parse_args()

    observations = parse_json_arg(args.observations_json, [])
    json_dump(
        {
            "status": "ok",
            "cookies": analyze_cookie_header(args.set_cookie),
            "jwt": decode_jwt(args.jwt) if args.jwt else None,
            "csrf_token_names": split_items(args.csrf_token_names),
            "observations": observations,
            "oauth_notes": args.oauth_notes,
            "checklist": [
                "Verify login, logout, password reset, MFA, and session invalidation flows.",
                "Confirm session cookies use Secure, HttpOnly, and SameSite where appropriate.",
                "Confirm CSRF tokens protect state-changing requests.",
                "Confirm JWT claims are minimal, signed with expected algorithm, and expire appropriately.",
                "Confirm account lockout/rate limiting exists without enabling denial-of-service abuse.",
            ],
        }
    )


if __name__ == "__main__":
    main_wrapper(main)
