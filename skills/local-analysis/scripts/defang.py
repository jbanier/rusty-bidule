#!/usr/bin/env python3
import argparse
import re

from common import add_common_text_arg, read_text_argument, json_dump


def defang(text: str) -> str:
    text = re.sub(r"(?i)\bhttps://", "hxxps://", text)
    text = re.sub(r"(?i)\bhttp://", "hxxp://", text)
    text = text.replace(".", "[.]")
    text = text.replace("@", "[@]")
    return text


def main() -> None:
    parser = argparse.ArgumentParser()
    add_common_text_arg(parser)
    args = parser.parse_args()
    source = read_text_argument(args)
    json_dump({"original": source, "result": defang(source)})


if __name__ == "__main__":
    main()
