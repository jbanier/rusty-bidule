#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, split_items  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--upload-endpoints", default="")
    parser.add_argument("--download-endpoints", default="")
    parser.add_argument("--allowed-extensions", default="")
    parser.add_argument("--feature-notes", default="")
    args = parser.parse_args()
    json_dump(
        {
            "status": "ok",
            "upload_endpoints": split_items(args.upload_endpoints),
            "download_endpoints": split_items(args.download_endpoints),
            "allowed_extensions": split_items(args.allowed_extensions),
            "feature_notes": args.feature_notes,
            "checklist": [
                "Verify authentication and authorization for upload, download, edit, and delete actions.",
                "Check extension, MIME type, file signature, filename, and size enforcement.",
                "Confirm uploads are stored outside executable web roots or served with safe content disposition.",
                "Check duplicate names, path traversal sequences, metadata reflection, and error leakage.",
                "Verify malware scanning behavior and timeout/rate-limit handling with benign test files.",
            ],
        }
    )


if __name__ == "__main__":
    main_wrapper(main)
