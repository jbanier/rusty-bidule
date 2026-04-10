#!/usr/bin/env python3
import argparse
import datetime as dt
import re

from common import add_common_text_arg, json_dump, read_text_argument


COMMON_FORMATS = [
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%d %H:%M:%S.%f",
    "%Y/%m/%d %H:%M:%S",
    "%d/%m/%Y %H:%M:%S",
    "%b %d %H:%M:%S",
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%dT%H:%M:%S.%f",
]

FILETIME_EPOCH = dt.datetime(1601, 1, 1, tzinfo=dt.timezone.utc)


def normalize(value: str) -> tuple[str, str]:
    value = value.strip()

    if re.fullmatch(r"\d{17,18}", value):
        filetime = int(value)
        instant = FILETIME_EPOCH + dt.timedelta(microseconds=filetime / 10)
        return "windows_filetime", instant.isoformat()

    if re.fullmatch(r"\d{10}(?:\.\d+)?", value):
        instant = dt.datetime.fromtimestamp(float(value), tz=dt.timezone.utc)
        return "unix_seconds", instant.isoformat()

    if re.fullmatch(r"\d{13}", value):
        instant = dt.datetime.fromtimestamp(int(value) / 1000, tz=dt.timezone.utc)
        return "unix_milliseconds", instant.isoformat()

    try:
        instant = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
        if instant.tzinfo is None:
            instant = instant.replace(tzinfo=dt.timezone.utc)
        return "iso8601", instant.astimezone(dt.timezone.utc).isoformat()
    except ValueError:
        pass

    for fmt in COMMON_FORMATS:
        try:
            instant = dt.datetime.strptime(value, fmt)
            instant = instant.replace(tzinfo=dt.timezone.utc)
            return fmt, instant.isoformat()
        except ValueError:
            continue

    raise SystemExit(f"unsupported timestamp format: {value}")


def main() -> None:
    parser = argparse.ArgumentParser()
    add_common_text_arg(parser)
    args = parser.parse_args()
    value = read_text_argument(args)
    source_format, normalized = normalize(value)
    json_dump({"input": value, "normalized_utc": normalized, "source_format": source_format})


if __name__ == "__main__":
    main()
