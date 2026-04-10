#!/usr/bin/env python3
import argparse
import pathlib

from common import json_dump, preview_text


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--path", required=True)
    parser.add_argument("--max-bytes", type=int, default=4096)
    parser.add_argument("--start-byte", type=int, default=0)
    args = parser.parse_args()

    path = pathlib.Path(args.path).expanduser().resolve()
    with path.open("rb") as handle:
        handle.seek(max(args.start_byte, 0))
        data = handle.read(max(args.max_bytes, 1))

    json_dump(
        {
            "path": str(path),
            "read_offset": max(args.start_byte, 0),
            "read_size": len(data),
            "text_preview": preview_text(data.decode("utf-8", errors="replace"), 2000),
        }
    )


if __name__ == "__main__":
    main()
