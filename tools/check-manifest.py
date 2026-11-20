#!/usr/bin/env python3
"""Check every package in a manifest against the live Arch repositories.

A manifest whose names are wrong fails at install time on the user's machine, which is the
worst possible moment. This turns that into a build failure instead.

Usage: check-manifest.py [MANIFEST ...]   (default: host/packages/packages.txt)
Exit: 0 every name resolves, 1 otherwise, 2 the repositories could not be reached.
"""

import json
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

API = "https://archlinux.org/packages/search/json/?name={}"
RETRIES = 3
TIMEOUT = 25


def read_manifest(path: Path) -> list[str]:
    names = []
    for line in path.read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            names.append(line)
    return names


def lookup(name: str) -> tuple[str, str] | None | Exception:
    """The (repo, version) of `name`, None if no such package, or the error."""
    last: Exception | None = None
    for attempt in range(RETRIES):
        try:
            with urllib.request.urlopen(API.format(name), timeout=TIMEOUT) as r:
                data = json.load(r)
            for entry in data.get("results", []):
                if entry["pkgname"] == name:
                    return entry["repo"], entry["pkgver"]
            return None
        except Exception as e:  # noqa: BLE001 - any transport failure is retried
            last = e
            time.sleep(1.5 * (attempt + 1))
    return last if last else RuntimeError("unknown")


def main(argv: list[str]) -> int:
    paths = [Path(p) for p in argv[1:]] or [Path("host/packages/packages.txt")]
    bad = 0
    errors = 0
    total = 0
    for path in paths:
        if not path.is_file():
            print(f"{path}: no such file", file=sys.stderr)
            return 2
        names = read_manifest(path)
        print(f"== {path} ({len(names)} packages)")
        for name in names:
            total += 1
            result = lookup(name)
            if isinstance(result, Exception):
                print(f"  ERROR    {name}: {result}")
                errors += 1
            elif result is None:
                print(f"  MISSING  {name}")
                bad += 1
            else:
                repo, version = result
                print(f"  ok       {name:<24} {repo:<9} {version}")
    print(f"\n{total} checked: {bad} missing, {errors} unreachable")
    if bad:
        return 1
    if errors:
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
