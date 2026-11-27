#!/usr/bin/env python3
"""Map raw Keka exports to Leavo's canonical import CSVs (T200, contracts/csv-formats.md §2–4).

The admin screen (Admin › Import › Keka) does the same mapping in the app; this developer tool
does it offline so the files can be reviewed or fixed before uploading as a generic CSV import.

    scripts/keka_import.py --balances keka-balances.csv [--history keka-leaves.csv] \
        --map map.json --leave-year 2026 --domain acme.com [--domain ...] --out-dir out/

`map.json` maps Keka leave type names to Leavo codes, e.g. {"Casual Leave": "CL"}. Keka column
names vary by report, so each column is found by any of its usual names (case-insensitive).
Only approved history rows are kept. Errors are printed and nothing is written when any exist.
Standard library only.
"""
import argparse
import csv
import datetime
import decimal
import json
import pathlib
import sys

EMAIL = ["email", "work email", "email id", "official email", "official email id", "employee email"]
NAME = ["employee name", "full name", "name"]
NUMBER = ["employee number", "employee id", "employee code", "emp id"]
DOJ = ["date of joining", "joining date", "doj"]
TYPE = ["leave type", "leave type name", "leave"]
BALANCE = ["available balance", "closing balance", "balance", "available", "leave balance"]
FROM = ["from date", "start date", "from", "leave from"]
TO = ["to date", "end date", "to", "leave to"]
DAYS = ["no of days", "no. of days", "number of days", "total days", "days", "leave days", "duration"]
STATUS = ["status", "leave status", "request status"]
REASON = ["reason", "note", "notes", "remarks", "comment"]
APPROVER = ["approved by email", "approver email", "approved by"]

DATE_FORMATS = ["%Y-%m-%d", "%d-%b-%Y", "%d %b %Y", "%d/%m/%Y", "%d-%m-%Y", "%d-%B-%Y", "%d %B %Y"]

BALANCE_COLUMNS = ["email", "full_name", "date_of_joining", "leave_type_code", "leave_year", "available_days", "employee_code"]
HISTORY_COLUMNS = ["email", "leave_type_code", "from_date", "to_date", "days", "reason", "approved_by_email"]


def _norm(header):
    return header.replace("﻿", "").strip().lower()


def _get(row, names):
    """The first non-empty value among a row's columns matching any of `names`."""
    by_name = {_norm(k): (v or "").strip() for k, v in row.items() if k is not None}
    for n in names:
        if by_name.get(n):
            return by_name[n]
    return ""


def _has(rows, names):
    return bool(rows) and any(_norm(k) in names for k in rows[0].keys() if k is not None)


def parse_date(value):
    for fmt in DATE_FORMATS:
        try:
            return datetime.datetime.strptime(value.strip(), fmt).date()
        except ValueError:
            continue
    return None


def parse_days(value):
    try:
        d = decimal.Decimal(value.strip())
    except (decimal.InvalidOperation, AttributeError):
        return None
    if (d * 2) % 1 != 0 or abs(d) > 400:
        return None
    text = format(d.normalize(), "f")
    return "0" if text in ("-0", "") else text


def _err(row, code, message):
    return {"row": row, "code": code, "message": message}


def _code(type_map, name):
    lower = {k.lower(): v for k, v in type_map.items()}
    return (lower.get(name.lower()) or "").upper()


def map_balances(rows, type_map, leave_year, domains):
    """Keka balance rows → canonical balance rows (§2), and errors by row (header = row 1)."""
    out, errors = [], []
    domains = {d.lower() for d in domains}
    for i, row in enumerate(rows, start=2):
        email = _get(row, EMAIL).lower()
        keka_type = _get(row, TYPE)
        if not email or not keka_type:
            errors.append(_err(i, "MISSING_VALUE", "Each row needs an email and a leave type."))
            continue
        if email.rsplit("@", 1)[-1] not in domains:
            errors.append(_err(i, "UNKNOWN_DOMAIN", f"{email} is not on an allowed domain."))
            continue
        code = _code(type_map, keka_type)
        if not code:
            errors.append(_err(i, "UNKNOWN_LEAVE_TYPE", f'Keka leave type "{keka_type}" is not mapped.'))
            continue
        days = parse_days(_get(row, BALANCE))
        if days is None:
            errors.append(_err(i, "INVALID_DAYS", "The balance must be whole or half days."))
            continue
        doj = _get(row, DOJ)
        doj_date = parse_date(doj) if doj else None
        if doj and doj_date is None:
            errors.append(_err(i, "INVALID_DATE", "Date of joining is not a date."))
            continue
        out.append({
            "email": email,
            "full_name": _get(row, NAME),
            "date_of_joining": doj_date.isoformat() if doj_date else "",
            "leave_type_code": code,
            "leave_year": str(leave_year),
            "available_days": days,
            "employee_code": _get(row, NUMBER),
        })
    return out, errors


def map_history(rows, type_map, domains):
    """Approved Keka leave rows → canonical history rows (§3); returns (rows, errors, skipped)."""
    out, errors, skipped = [], [], 0
    domains = {d.lower() for d in domains}
    for i, row in enumerate(rows, start=2):
        status = _get(row, STATUS)
        if status and status.lower() != "approved":
            skipped += 1
            continue
        email = _get(row, EMAIL).lower()
        keka_type = _get(row, TYPE)
        if not email or not keka_type:
            errors.append(_err(i, "MISSING_VALUE", "Each row needs an email and a leave type."))
            continue
        if email.rsplit("@", 1)[-1] not in domains:
            errors.append(_err(i, "UNKNOWN_DOMAIN", f"{email} is not on an allowed domain."))
            continue
        code = _code(type_map, keka_type)
        if not code:
            errors.append(_err(i, "UNKNOWN_LEAVE_TYPE", f'Keka leave type "{keka_type}" is not mapped.'))
            continue
        start, end = parse_date(_get(row, FROM)), parse_date(_get(row, TO))
        if start is None or end is None or end < start:
            errors.append(_err(i, "INVALID_DATE", "From and to must be dates, from first."))
            continue
        days = parse_days(_get(row, DAYS))
        if days is None or decimal.Decimal(days) <= 0:
            errors.append(_err(i, "INVALID_DAYS", "Days must be a positive number of whole or half days."))
            continue
        approver = _get(row, APPROVER).lower()
        out.append({
            "email": email,
            "leave_type_code": code,
            "from_date": start.isoformat(),
            "to_date": end.isoformat(),
            "days": days,
            "reason": _get(row, REASON)[:500],
            "approved_by_email": approver if "@" in approver else "",
        })
    return out, errors, skipped


def _read(path):
    with open(path, newline="", encoding="utf-8-sig") as f:
        return list(csv.DictReader(f))


def _write(path, columns, rows):
    with open(path, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=columns, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--balances", required=True)
    p.add_argument("--history")
    p.add_argument("--map", required=True, help="JSON: Keka leave type name → Leavo code")
    p.add_argument("--leave-year", required=True, type=int)
    p.add_argument("--domain", action="append", required=True, help="allowed email domain (repeatable)")
    p.add_argument("--out-dir", required=True)
    args = p.parse_args(argv)

    type_map = json.loads(pathlib.Path(args.map).read_text(encoding="utf-8"))
    balance_rows = _read(args.balances)
    if not (_has(balance_rows, EMAIL) and _has(balance_rows, TYPE) and _has(balance_rows, BALANCE)):
        print(f"{args.balances}: needs Email, Leave Type and Available Balance columns", file=sys.stderr)
        return 1
    balances, errors = map_balances(balance_rows, type_map, args.leave_year, args.domain)
    problems = [(args.balances, e) for e in errors]
    history, skipped = [], 0
    if args.history:
        history, herrors, skipped = map_history(_read(args.history), type_map, args.domain)
        problems += [(args.history, e) for e in herrors]
    for path, e in problems:
        print(f"{path}:{e['row']}: {e['code']} {e['message']}", file=sys.stderr)
    if problems:
        return 1
    out = pathlib.Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)
    _write(out / "balances.csv", BALANCE_COLUMNS, balances)
    if args.history:
        _write(out / "history.csv", HISTORY_COLUMNS, history)
    print(f"wrote {len(balances)} balances and {len(history)} history rows ({skipped} not approved, skipped) to {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
