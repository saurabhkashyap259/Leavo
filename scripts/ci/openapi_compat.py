#!/usr/bin/env python3
"""Fail if the API removes something compared with the latest release (FR-050).

Within a major version the API may only grow. This compares the committed
`contracts/openapi.yaml` with the copy at the most recent `vX.Y.Z` git tag of the
same major version and reports removed operations or removed schema properties.
No release tag yet -> nothing to compare, exit 0.

The generated YAML has a fixed shape (utoipa), so a small indentation parser is
enough and keeps this script dependency-free.
"""
import re
import subprocess
import sys

PATH = "specs/001-leave-management-v1/contracts/openapi.yaml"
METHODS = {"get", "post", "put", "patch", "delete"}


def surface(text):
    ops, props = set(), set()
    section = path = schema = None
    in_props = False
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        indent = len(line) - len(line.lstrip())
        key = line.strip().split(":")[0].strip("'\"")
        if indent == 0:
            section = key
            continue
        if section == "paths":
            if indent == 2:
                path = key
            elif indent == 4 and key in METHODS:
                ops.add(f"{key.upper()} {path}")
        elif section == "components":
            if indent == 4:
                schema, in_props = key, False
            elif indent == 6:
                in_props = key == "properties"
            elif indent == 8 and in_props:
                props.add(f"{schema}.{key}")
    return ops, props


def git(*args):
    return subprocess.run(["git", *args], capture_output=True, text=True)


def main():
    current = open(PATH).read()
    major = re.search(r"^  version: (\d+)\.", current, re.M)
    tags = git("tag", "--list", "v*", "--sort=-v:refname").stdout.split()
    if major:
        tags = [t for t in tags if t.startswith(f"v{major.group(1)}.")]
    if not tags:
        print("openapi-compat: no previous release in this major version; skipping")
        return 0
    base = git("show", f"{tags[0]}:{PATH}")
    if base.returncode != 0:
        print(f"openapi-compat: {tags[0]} has no {PATH}; skipping")
        return 0
    old_ops, old_props = surface(base.stdout)
    new_ops, new_props = surface(current)
    removed = sorted(old_ops - new_ops) + sorted(old_props - new_props)
    if removed:
        print(f"openapi-compat: breaking changes against {tags[0]}:")
        for r in removed:
            print(f"  removed {r}")
        return 1
    print(f"openapi-compat: OK against {tags[0]} ({len(new_ops)} operations)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
