#!/usr/bin/env python3
import argparse
from urllib.parse import parse_qsl, urlparse

from common import add_common_text_arg, json_dump, read_text_argument


def main() -> None:
    parser = argparse.ArgumentParser()
    add_common_text_arg(parser)
    args = parser.parse_args()
    text = read_text_argument(args).strip()
    parsed = urlparse(text)

    json_dump(
        {
            "fragment": parsed.fragment,
            "fqdn": parsed.hostname,
            "netloc": parsed.netloc,
            "params": [{"key": key, "value": value} for key, value in parse_qsl(parsed.query, keep_blank_values=True)],
            "password": parsed.password,
            "path": parsed.path,
            "port": parsed.port,
            "query": parsed.query,
            "scheme": parsed.scheme,
            "url": text,
            "username": parsed.username,
        }
    )


if __name__ == "__main__":
    main()
