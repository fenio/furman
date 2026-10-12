#!/usr/bin/env python3
"""Check release-tag/source version consistency (Python 3.11+, no dependencies)."""

import argparse
import json
from pathlib import Path
import re
import sys
import tomllib


def check_versions(root: Path, tag: str) -> list[str]:
    """Return all problems, including missing/malformed files and version fields."""
    number = r"(?:0|[1-9][0-9]*)"
    prerelease = r"(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
    semver = rf"{number}\.{number}\.{number}(?:-{prerelease}(?:\.{prerelease})*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    if not re.fullmatch(rf"v{semver}", tag):
        return [f"Invalid release tag {tag!r}: expected v followed by a semantic version"]
    expected = tag[1:]
    problems = []

    def compare(label, version):
        if version != expected:
            problems.append(f"{label}: expected {expected!r}, found {version!r}")

    for filename in ("package.json", "package-lock.json", "src-tauri/tauri.conf.json"):
        try:
            data = json.loads((root / filename).read_text(encoding="utf-8"))
            compare(f"{filename} version", data.get("version"))
            if filename == "package-lock.json":
                compare(f'{filename} packages[""] version', data.get("packages", {}).get("", {}).get("version"))
        except (OSError, ValueError, AttributeError) as error:
            problems.append(f"{filename}: cannot read version: {error}")

    for filename in ("src-tauri/Cargo.toml", "src-tauri/Cargo.lock"):
        try:
            data = tomllib.loads((root / filename).read_text(encoding="utf-8"))
            if filename.endswith("Cargo.toml"):
                compare(f"{filename} package.version", data.get("package", {}).get("version"))
            else:
                packages = [package for package in data.get("package", []) if package.get("name") == "furman"]
                if len(packages) != 1:
                    problems.append(f"{filename}: expected exactly one furman package, found {len(packages)}")
                else:
                    compare(f"{filename} furman version", packages[0].get("version"))
        except (OSError, ValueError, AttributeError, TypeError) as error:
            problems.append(f"{filename}: cannot read version: {error}")

    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag", help="Release tag, e.g. v0.3.17")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    args = parser.parse_args()
    problems = check_versions(args.root, args.tag)
    if problems:
        print("Release version validation failed:", file=sys.stderr)
        for problem in problems:
            print(f"- {problem}", file=sys.stderr)
        return 1
    print(f"Release {args.tag}: all source and lockfile versions match.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
