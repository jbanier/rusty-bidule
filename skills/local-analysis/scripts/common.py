#!/usr/bin/env python3
import argparse
import base64
import binascii
import hashlib
import json
import math
import mimetypes
import os
import pathlib
import re
import shutil
import subprocess
import sys
from typing import Any


def json_dump(payload: dict[str, Any]) -> None:
    print(json.dumps(payload, indent=2, sort_keys=True))


def read_text_argument(args: argparse.Namespace) -> str:
    if getattr(args, "text", None):
        return args.text
    data = sys.stdin.read()
    if data:
        return data
    raise SystemExit("expected --text or stdin input")


def read_bytes_from_file(path: str) -> bytes:
    return pathlib.Path(path).read_bytes()


def shannon_entropy(data: bytes) -> float:
    if not data:
        return 0.0
    counts = [0] * 256
    for byte in data:
        counts[byte] += 1
    entropy = 0.0
    length = len(data)
    for count in counts:
        if not count:
            continue
        p = count / length
        entropy -= p * math.log2(p)
    return entropy


def command_exists(name: str) -> bool:
    return shutil.which(name) is not None


def run_command(argv: list[str]) -> tuple[int, str, str]:
    proc = subprocess.run(argv, capture_output=True, text=True)
    return proc.returncode, proc.stdout, proc.stderr


def maybe_magic_description(path: str) -> str | None:
    if not command_exists("file"):
        return None
    code, stdout, _stderr = run_command(["file", "-b", path])
    if code != 0:
        return None
    return stdout.strip() or None


def sha256_file(path: str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


HEX_RE = re.compile(r"^(?:0x)?[0-9a-fA-F]+$")


def looks_like_hex(text: str) -> bool:
    stripped = re.sub(r"\s+", "", text)
    if len(stripped) < 2 or len(stripped) % 2 != 0:
        return False
    return bool(HEX_RE.fullmatch(stripped))


BASE64_RE = re.compile(r"^[A-Za-z0-9+/=\s_-]+$")


def looks_like_base64(text: str) -> bool:
    stripped = re.sub(r"\s+", "", text)
    if len(stripped) < 8:
        return False
    return bool(BASE64_RE.fullmatch(stripped))


def decode_hex(text: str) -> bytes:
    stripped = re.sub(r"\s+", "", text)
    stripped = stripped[2:] if stripped.lower().startswith("0x") else stripped
    return bytes.fromhex(stripped)


def decode_b64(text: str) -> bytes:
    stripped = re.sub(r"\s+", "", text)
    if len(stripped) % 4:
        stripped += "=" * (4 - (len(stripped) % 4))
    return base64.b64decode(stripped, validate=False)


def safe_text(data: bytes) -> str:
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return data.decode("utf-8", errors="replace")


def preview_text(text: str, limit: int = 2000) -> str:
    if len(text) <= limit:
        return text
    return text[:limit] + "\n...[truncated]..."


def add_common_text_arg(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--text", help="Text input. If omitted, stdin is used.")


def add_common_json_out(payload: dict[str, Any]) -> None:
    json_dump(payload)
