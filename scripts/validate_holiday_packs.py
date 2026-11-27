#!/usr/bin/env python3
"""Validate holiday packs: holidays/<COUNTRY>[-<REGION>]/<YEAR>.csv (research R21, FR-034)."""
import csv
import datetime
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1] / "holidays"
errors = []
packs = sorted(ROOT.glob("*/*.csv"))
if not packs:
    errors.append("no holiday packs found")
for path in packs:
    rel = path.relative_to(ROOT)
    if not re.fullmatch(r"[A-Z]{2}(-[A-Z0-9]{1,3})?", path.parent.name):
        errors.append(f"{rel}: folder must be COUNTRY or COUNTRY-REGION")
    if not re.fullmatch(r"\d{4}", path.stem):
        errors.append(f"{rel}: file name must be a year")
        continue
    year = int(path.stem)
    with path.open(newline="", encoding="utf-8") as f:
        reader = csv.reader(f)
        header = next(reader, None)
        if header != ["date", "name", "kind"]:
            errors.append(f"{rel}: header must be date,name,kind")
            continue
        seen = set()
        for line, row in enumerate(reader, start=2):
            if len(row) != 3:
                errors.append(f"{rel}:{line}: expected 3 columns")
                continue
            date, name, kind = (c.strip() for c in row)
            try:
                d = datetime.date.fromisoformat(date)
            except ValueError:
                errors.append(f"{rel}:{line}: bad date {date!r}")
                continue
            if d.year != year:
                errors.append(f"{rel}:{line}: {date} is not in {year}")
            if not 1 <= len(name) <= 120:
                errors.append(f"{rel}:{line}: name must be 1-120 characters")
            if kind not in ("mandatory", "optional"):
                errors.append(f"{rel}:{line}: kind must be mandatory or optional")
            if (d, name) in seen:
                errors.append(f"{rel}:{line}: duplicate {date} {name}")
            seen.add((d, name))
for e in errors:
    print(e, file=sys.stderr)
print(f"{len(packs)} pack(s) checked, {len(errors)} error(s)")
sys.exit(1 if errors else 0)
