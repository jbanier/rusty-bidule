#!/usr/bin/env python3
import argparse
import datetime as dt
import json
import os
import pathlib
import shutil

from hashlib import sha256


def file_hash(path: pathlib.Path) -> str:
    digest = sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def copy_tree(source: pathlib.Path, target: pathlib.Path) -> list[pathlib.Path]:
    copied: list[pathlib.Path] = []
    for item in source.rglob("*"):
        rel = item.relative_to(source)
        dest = target / rel
        if item.is_dir():
            dest.mkdir(parents=True, exist_ok=True)
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(item, dest)
        copied.append(dest)
    return copied


def filesystem_scope() -> str:
    return os.environ.get("RUSTY_BIDULE_FILESYSTEM_SCOPE", "full")


def filesystem_root() -> pathlib.Path:
    raw = os.environ.get("RUSTY_BIDULE_FILESYSTEM_ROOT")
    if raw:
        return pathlib.Path(raw).expanduser().resolve()
    return pathlib.Path.cwd().resolve()


def path_is_relative_to(path: pathlib.Path, root: pathlib.Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def resolve_scoped_path(path: str, *, must_exist: bool) -> pathlib.Path:
    root = filesystem_root()
    candidate = pathlib.Path(path).expanduser()
    if not candidate.is_absolute():
        candidate = root / candidate
    if must_exist or candidate.exists():
        resolved = candidate.resolve()
    else:
        resolved = candidate.parent.resolve() / candidate.name
    if filesystem_scope() != "full" and not path_is_relative_to(resolved, root):
        raise SystemExit(
            f"path outside filesystem workspace scope: {resolved} (workspace root: {root})"
        )
    return resolved


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-dir", required=True)
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--case-name", required=True)
    parser.add_argument("--summary-json", default="{}")
    args = parser.parse_args()

    source = resolve_scoped_path(args.source_dir, must_exist=True)
    output_root = resolve_scoped_path(args.output_dir, must_exist=False)
    timestamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%d%H%M%S")
    bundle_dir = output_root / f"{args.case_name}-{timestamp}"
    bundle_dir.mkdir(parents=True, exist_ok=True)

    copied = copy_tree(source, bundle_dir / "conversation")
    summary = {
        "bundle_created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "bundle_dir": str(bundle_dir),
        "case_name": args.case_name,
        "conversation_source": str(source),
        "copied_file_count": len(copied),
        "operator_summary": json.loads(args.summary_json),
    }

    summary_path = bundle_dir / "bundle.json"
    summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True))
    copied.append(summary_path)

    manifest_lines = []
    for path in sorted(copied):
        rel = path.relative_to(bundle_dir)
        manifest_lines.append(f"{file_hash(path)}  {rel}")
    manifest_path = bundle_dir / "manifest.sha256"
    manifest_path.write_text("\n".join(manifest_lines) + "\n")

    print(
        json.dumps(
            {
                "bundle_dir": str(bundle_dir),
                "bundle_json": str(summary_path),
                "manifest_sha256": str(manifest_path),
                "copied_file_count": len(copied),
                "limitations": [
                    "unsigned archive",
                    "analyst review required",
                    "not an immutable chain-of-custody format",
                ],
            },
            indent=2,
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
