"""T195 — scripts/keka_import.py: maps raw Keka exports to Leavo's canonical import CSVs
(contracts/csv-formats.md §2–4) offline, checking leave types and email domains."""
import csv
import io
import json
import pathlib
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import keka_import  # noqa: E402

BALANCES = (
    "﻿Employee Number,Employee Name,Email,Date Of Joining,Leave Type,Available Balance\n"
    "E-1,Asha Rao,Asha@Acme.test,01-Apr-2025,Casual Leave,8\n"
    "E-2,Ravi K,ravi@gmail.com,,Casual Leave,3\n"
    "E-3,Kiran,kiran@acme.test,,Optional Holiday,1\n"
)
HISTORY = (
    "Work Email,Leave Type,From Date,To Date,No Of Days,Status,Reason\n"
    "asha@acme.test,Casual Leave,02/11/2026,03/11/2026,2,Approved,Trip\n"
    "asha@acme.test,Casual Leave,10-Nov-2026,10-Nov-2026,1,Rejected,No\n"
)
MAP = {"Casual Leave": "CL"}


def rows(text):
    return list(csv.DictReader(io.StringIO(text)))


def test_balances_are_mapped_and_bad_rows_reported():
    out, errors = keka_import.map_balances(rows(BALANCES), MAP, 2026, {"acme.test"})
    assert out == [
        {
            "email": "asha@acme.test",
            "full_name": "Asha Rao",
            "date_of_joining": "2025-04-01",
            "leave_type_code": "CL",
            "leave_year": "2026",
            "available_days": "8",
            "employee_code": "E-1",
        }
    ]
    assert [(e["row"], e["code"]) for e in errors] == [(3, "UNKNOWN_DOMAIN"), (4, "UNKNOWN_LEAVE_TYPE")]
    assert "Optional Holiday" in errors[1]["message"]


def test_only_approved_history_is_kept():
    out, errors, skipped = keka_import.map_history(rows(HISTORY), MAP, {"acme.test"})
    assert errors == []
    assert skipped == 1
    assert out == [
        {
            "email": "asha@acme.test",
            "leave_type_code": "CL",
            "from_date": "2026-11-02",
            "to_date": "2026-11-03",
            "days": "2",
            "reason": "Trip",
            "approved_by_email": "",
        }
    ]


def test_command_line_writes_canonical_files(tmp_path):
    (tmp_path / "bal.csv").write_text(BALANCES.replace("ravi@gmail.com", "ravi@acme.test").replace("Optional Holiday", "Casual Leave"), encoding="utf-8")
    (tmp_path / "hist.csv").write_text(HISTORY, encoding="utf-8")
    (tmp_path / "map.json").write_text(json.dumps(MAP), encoding="utf-8")
    result = subprocess.run(
        [sys.executable, str(pathlib.Path(keka_import.__file__)),
         "--balances", str(tmp_path / "bal.csv"), "--history", str(tmp_path / "hist.csv"),
         "--map", str(tmp_path / "map.json"), "--leave-year", "2026", "--domain", "acme.test",
         "--out-dir", str(tmp_path / "out")],
        capture_output=True, text=True,
    )
    assert result.returncode == 0, result.stderr
    balances = (tmp_path / "out" / "balances.csv").read_text(encoding="utf-8")
    assert balances.splitlines()[0] == "email,full_name,date_of_joining,leave_type_code,leave_year,available_days,employee_code"
    assert len(balances.splitlines()) == 4
    assert (tmp_path / "out" / "history.csv").read_text(encoding="utf-8").count("\n") == 2


def test_command_line_fails_on_errors(tmp_path):
    (tmp_path / "bal.csv").write_text(BALANCES, encoding="utf-8")
    (tmp_path / "map.json").write_text(json.dumps(MAP), encoding="utf-8")
    result = subprocess.run(
        [sys.executable, str(pathlib.Path(keka_import.__file__)),
         "--balances", str(tmp_path / "bal.csv"), "--map", str(tmp_path / "map.json"),
         "--leave-year", "2026", "--domain", "acme.test", "--out-dir", str(tmp_path / "out")],
        capture_output=True, text=True,
    )
    assert result.returncode == 1
    assert "UNKNOWN_DOMAIN" in result.stderr
    assert not (tmp_path / "out").exists()
