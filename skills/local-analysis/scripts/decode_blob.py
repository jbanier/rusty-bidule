#!/usr/bin/env python3
import argparse
import binascii

from common import (
    add_common_text_arg,
    decode_b64,
    decode_hex,
    json_dump,
    looks_like_base64,
    looks_like_hex,
    preview_text,
    read_text_argument,
    safe_text,
)


def main() -> None:
    parser = argparse.ArgumentParser()
    add_common_text_arg(parser)
    parser.add_argument("--max-layers", type=int, default=3)
    args = parser.parse_args()

    current = read_text_argument(args).strip()
    layers: list[dict[str, str]] = []

    for _index in range(max(args.max_layers, 1)):
        decoded = None
        mode = None
        try:
            if looks_like_hex(current):
                decoded = decode_hex(current)
                mode = "hex"
            elif looks_like_base64(current):
                decoded = decode_b64(current)
                mode = "base64"
        except (ValueError, binascii.Error):
            decoded = None

        if decoded is None:
            break

        text = safe_text(decoded)
        layers.append(
            {
                "encoding": mode,
                "decoded_preview": preview_text(text, 800),
            }
        )
        current = text.strip()

    json_dump(
        {
            "final_text_preview": preview_text(current, 2000),
            "layers": layers,
            "layer_count": len(layers),
        }
    )


if __name__ == "__main__":
    main()
