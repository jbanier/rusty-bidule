#!/usr/bin/env python3
import argparse
import datetime as dt
import pathlib

from common import json_dump, maybe_magic_description, mimetypes, read_bytes_from_file, shannon_entropy


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--path", required=True)
    parser.add_argument("--sample-bytes", type=int, default=65536)
    args = parser.parse_args()

    path = pathlib.Path(args.path).expanduser().resolve()
    stat = path.stat()
    sample = read_bytes_from_file(str(path))[: max(args.sample_bytes, 1)]
    guessed_mime, _encoding = mimetypes.guess_type(str(path))

    json_dump(
        {
            "created_at": dt.datetime.fromtimestamp(stat.st_ctime, tz=dt.timezone.utc).isoformat(),
            "entropy_sample": round(shannon_entropy(sample), 4),
            "is_symlink": path.is_symlink(),
            "magic_description": maybe_magic_description(str(path)),
            "mime_guess": guessed_mime,
            "modified_at": dt.datetime.fromtimestamp(stat.st_mtime, tz=dt.timezone.utc).isoformat(),
            "path": str(path),
            "size_bytes": stat.st_size,
            "viewed_at": dt.datetime.fromtimestamp(stat.st_atime, tz=dt.timezone.utc).isoformat(),
        }
    )


if __name__ == "__main__":
    main()
