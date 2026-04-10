#!/usr/bin/env python3
import argparse
import hashlib
import pathlib
import shutil
import subprocess

from common import json_dump, sha256_file


def digest(path: pathlib.Path, algorithm: str) -> str:
    h = hashlib.new(algorithm)
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def maybe_ssdeep(path: pathlib.Path) -> tuple[str | None, str | None]:
    binary = shutil.which("ssdeep")
    if not binary:
        return None, "ssdeep command not found"
    proc = subprocess.run([binary, "-b", str(path)], capture_output=True, text=True)
    if proc.returncode != 0:
        return None, proc.stderr.strip() or "ssdeep failed"
    parts = [line for line in proc.stdout.splitlines() if line.strip()]
    if not parts:
        return None, "ssdeep produced no output"
    first = parts[0].split(",", 1)[0].strip()
    return first or None, None


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--path", required=True, help="Local file path")
    args = parser.parse_args()

    path = pathlib.Path(args.path).expanduser().resolve()
    stat = path.stat()
    ssdeep_value, ssdeep_error = maybe_ssdeep(path)

    payload = {
        "hashes": {
            "md5": digest(path, "md5"),
            "sha1": digest(path, "sha1"),
            "sha256": sha256_file(str(path)),
            "sha512": digest(path, "sha512"),
            "ssdeep": ssdeep_value,
        },
        "path": str(path),
        "size_bytes": stat.st_size,
    }
    if ssdeep_error:
        payload["ssdeep_error"] = ssdeep_error
    json_dump(payload)


if __name__ == "__main__":
    main()
