# File Format Contract: CSV Import / Export and Holiday Packs

All files are UTF-8, comma-separated, header row required, dates `YYYY-MM-DD`, day values multiples of 0.5. Unknown columns are ignored; missing required columns fail the whole file.

## 1. Employee invite (`/admin/users/import`)

| Column | Required | Notes |
|---|---|---|
| email | Yes | domain must be whitelisted |
| full_name | Yes | |
| employee_code | No | unique per org |
| department | No | |
| date_of_joining | No | drives pro-rating |
| role | No | `employee` (default) \| `approver` \| `admin` |
| approver_group | No | group name; created if missing (dry run shows it) |

## 2. Opening balances (`/admin/import/csv`, also the canonical Keka target)

| Column | Required | Notes |
|---|---|---|
| email | Yes | creates invited user if absent (needs `full_name`) |
| full_name | When creating | |
| date_of_joining | No | |
| leave_type_code | Yes | must exist in org (e.g. `CL`, `SL`, `CO`) |
| leave_year | Yes | e.g. `2026` |
| available_days | Yes | multiple of 0.5, may be negative. One `import` ledger row makes the user's balance for that type and leave year (the whole pool for comp-off) equal this value; on re-import it is the compensating difference, and nothing is written if it already matches |
| employee_code, department, approver_group | No | used only when creating the person; a missing group is created (warning `GROUP_WILL_BE_CREATED`) |

## 3. Leave history (optional, same endpoint, `history` file part)

| Column | Required | Notes |
|---|---|---|
| email | Yes | |
| leave_type_code | Yes | |
| from_date, to_date | Yes | |
| days | Yes | trusted as given (Keka's count) |
| reason | No | defaults to `Imported from Keka` |
| approved_by_email | No | |

History rows are informational (FR-043): they become `approved` requests with booked days, so they appear in history and block overlapping requests, but they write **no** `leave_taken` ledger rows because the opening balance already reflects them.

## 4. Keka importer (`/admin/import/keka`)

Accepts Keka's raw export columns and maps them to formats 2–3 using a configurable map (Keka leave type name → Leavo code) shown in the dry-run screen; names equal to a Leavo type's code or name are suggested. Columns are found by any usual header name, case-insensitive (e.g. `Email`/`Work Email`, `Leave Type`, `Available Balance`/`Closing Balance`, `From Date`, `To Date`, `No Of Days`, `Status`, `Employee Name`, `Employee Number`, `Date Of Joining`); dates as `YYYY-MM-DD`, `DD-Mon-YYYY`, `DD Mon YYYY` or `DD/MM/YYYY`. Only history rows with status Approved are imported. The balance report has no year, so the leave year is chosen in the screen. `scripts/keka_import.py` performs the same mapping offline for developers.

## 5. Dry-run result rows

Apply is **all or nothing**: it runs only when the dry run reports zero errors; otherwise the API returns `422 IMPORT_HAS_ERRORS` and nothing is written (FR-043).

**Errors** (row not applicable; block the whole apply): `UNKNOWN_DOMAIN`, `UNKNOWN_LEAVE_TYPE`, `INVALID_DATE`, `INVALID_DAYS`, `DUPLICATE_ROW`, `MISSING_COLUMN`, `OVERLAP`, plus `MISSING_VALUE` (no email/type, or a new person without `full_name`) and `UNKNOWN_USER` (history for someone neither in the org nor in the balances file). Row numbers count the header line; issues carry the `file` (`balances` or `history`).

**Warnings** (row applied; shown for review): `USER_EXISTS` (balance applied to the existing user), `GROUP_WILL_BE_CREATED`, `BALANCE_REPLACES_EARLIER_IMPORT`, `HISTORY_OUTSIDE_LEAVE_YEAR`.

## 6. Holidays (`/admin/holidays/import` and packs in `holidays/<CC>[-<REGION>]/<YEAR>.csv`)

| Column | Required | Notes |
|---|---|---|
| date | Yes | must fall in `<YEAR>` for packs |
| name | Yes | |
| kind | No | `mandatory` (default) \| `optional` |
| location | No | |

Columns are matched by header name, in any order. Row errors (row numbers count the header line): `MISSING_COLUMN` (no `date` or `name` header), `MISSING_VALUE` (empty name or over 120 characters), `INVALID_DATE` (not `YYYY-MM-DD`), `INVALID_KIND`, `DUPLICATE_ROW` (same date and name, ignoring case). Blank lines are ignored.

CI validates every pack: parseable, sorted, no duplicate `(date,name)`, year matches filename.

v1 ships `holidays/IN/2026.csv` and `holidays/IN/2027.csv` (India national holidays). State packs (e.g. `IN-KA`) and other countries are community contributions (FR-034).

## 7. Exports

- `/me/leaves.csv`: `leave_type,from,to,days,half_day,status,requested_on,decided_by,decided_on,reason,approver_comment,notified`
  - Dates (`requested_on`, `decided_on`) in the org time zone; `leave_type` is the type name; `half_day` is empty, `first half`/`second half` for a single day, or `first day: second half; last day: first half`; `notified` is names (members) or emails joined with `; `. Values starting with `=`, `+`, `-` or `@` are prefixed with `'`.
- `/admin/reports/leaves.csv?group_by=…`: `group,leave_type,days`
- `/admin/carry-forward/preview.csv` (also attached to the year-end admin summary as `carry-forward-<closing year>.csv`): `email,full_name,leave_type,unused,carry,lapse` — `leave_type` is the type code, numbers without trailing zeros, CRLF
