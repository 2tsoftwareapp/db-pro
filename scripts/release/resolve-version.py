#!/usr/bin/env python3
"""Read the native app version and validate the release tag before packaging."""

import argparse
from pathlib import Path
import re
import sys
import tomllib

# Release version shape: keep Cargo prerelease/build metadata readable without
# allowing newlines or workflow-output delimiters into the resolved identity.
VERSION_SHAPE = r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?"


def read_app_version(manifest: Path) -> str:
    with manifest.open("rb") as handle:
        version = tomllib.load(handle)["package"]["version"]
    if not isinstance(version, str) or not re.fullmatch(VERSION_SHAPE, version):
        raise ValueError("native app version must use MAJOR.MINOR.PATCH with optional prerelease/build metadata")
    return version


def validate_tag(version: str, source_ref: str) -> None:
    tag = source_ref.removeprefix("refs/tags/")
    if tag == f"v{version}":
        return
    if "-" not in version and "+" not in version and re.fullmatch(rf"v{re.escape(version)}-rc\.[1-9][0-9]*", tag):
        return
    raise ValueError(
        f"Release tag {tag} does not match source app version {version} "
        f"(expected v{version} or v{version}-rc.N)"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--tag", help="Tag name or refs/tags/ name; omit for branch/commit builds")
    args = parser.parse_args()
    try:
        version = read_app_version(args.manifest)
        if args.tag is not None:
            validate_tag(version, args.tag)
    except (OSError, KeyError, TypeError, tomllib.TOMLDecodeError, ValueError) as error:
        print(f"::error::{error}", file=sys.stderr)
        return 1
    # Only the source version reaches stdout/GITHUB_OUTPUT. Archive names stay
    # unchanged; source_ref in release provenance identifies the exact RC tag.
    print(version)
    return 0


if __name__ == "__main__":
    sys.exit(main())
