# HTTP API Contract: Leavo v1

**Base path**: `/api/v1` · **Format**: JSON (UTF-8) · **Auth**: `leavo_sid` session cookie
**Source of truth at build time**: `contracts/openapi.yaml`, generated from Rust types with `utoipa`; CI fails if it drifts from this document's shapes.

## Conventions

- **CSRF**: every non-GET request must send `X-Requested-With: leavo`; otherwise `403 CSRF`.
- **Tenant**: always taken from the session, never from body or URL.
- **Errors**: `{"error": {"code": "INSUFFICIENT_BALANCE", "message": "Human readable", "details": {...}}}`. Every code and its HTTP status is defined once in the [error code catalogue](../data-model.md#error-code-catalogue); this document uses only codes listed there.
- **Idempotency**: `POST /leaves`, `POST /comp-offs` and `POST /approvals/{kind}/{id}/decide` require an `Idempotency-Key` header (UUID). A repeat with the same key within 24 h returns the original response; a different body with the same key returns `409 IDEMPOTENCY_MISMATCH` (FR-019).
- **Versioning** (FR-050): `/api/v1` changes only by addition (new endpoints, new optional fields, new enum values clients must tolerate). Removals or behaviour changes need `/api/v2` in a major release; anything deprecated is flagged with a `Deprecation` response header and in the release notes at least one minor release before removal.
- **Other people's leave** (FR-058): every response that includes another user's leave — `/away`, `/team/calendar`, `/leaves/{id}`, `/approvals` (`team_away`), `/admin/dashboard` — includes `leave_type` and `reason` **only** when the viewer is an Org Admin or an eligible approver. For request-level responses (`/leaves/{id}`, `/approvals`) eligibility is the request's fixed approver set from its `routed_group_id`; for person-level responses (`/away`, `/team/calendar`, `team_away`, `/admin/dashboard`) it is the user's current group's approvers, or any approver when the user has no group (FR-022, FR-058). Otherwise those fields are omitted, not nulled. Attachments follow the same rule (FR-029).
- **Pagination**: `?cursor=<opaque>&limit=<1..100, default 25>` → `{"items": [...], "next_cursor": "…" | null}`.
- **Dates**: `YYYY-MM-DD` in org time zone; instants RFC 3339 UTC.
- **Days**: JSON numbers, multiples of 0.5.
- **Roles**: `E` employee, `A` approver, `AD` admin. "Any" = any signed-in user.

---

## Setup and instance

| Method | Path | Role | Purpose |
|---|---|---|---|
| GET | `/setup/status` | Public | `{ "setup_required": bool, "mode": "single"\|"multi", "holiday_packs": [{ id, name, count }] }` |
| POST | `/setup/verify-token` | Public | `{ setup_token }` → `204`; wrong → `403` (rate limited) |
| POST | `/setup/test-email` | Public + setup token | `{ setup_token, to }` → `{ ok, error? }` with SMTP error text |
| POST | `/setup/complete` | Public + setup token + verified code | Creates org, admin, domain, optional holiday pack & seeded types; sets session |
| GET | `/admin/instance` | AD | `{ version, latest_version?, update_check_enabled, email: {status, last_error?}, storage: "local"\|"s3", migrations: {applied, pending}, jobs: {queued, dead} }` — in `LEAVO_MODE=multi` only `version`, `latest_version`, `update_check_enabled`, and `email`/`jobs` scoped to the caller's org are returned; `storage` and `migrations` are omitted (FR-051) |
| POST | `/admin/instance/test-email` | AD | Send test email to caller |

`POST /setup/complete` body:
```json
{
  "setup_token": "…",
  "org": { "name": "Acme", "timezone": "Asia/Kolkata", "weekend": ["sat","sun"], "leave_year_start_month": 1 },
  "admin": { "full_name": "Asha Rao", "email": "asha@acme.com", "code": "123456" },
  "extra_domains": ["acme.in"],
  "holiday_pack": "IN/2026" ,
  "seed_leave_types": true
}
```

## Auth

| Method | Path | Role | Purpose |
|---|---|---|---|
| POST | `/auth/code` | Public | `{ email, purpose: "login"\|"signup"\|"register_org"\|"setup", full_name?, date_of_joining?, setup_token? }` → `202 {"status":"sent"}` (see rules below) |
| POST | `/auth/verify` | Public | `{ email, code, purpose, keep_signed_in?: bool }` → `200 { user }` + `Set-Cookie` (login/signup) or `200 { verified: true, ticket }` (register_org; ticket valid 30 min). The `setup` code is checked by `POST /setup/complete` instead |
| POST | `/auth/logout` | Any | `?all=true` revokes every session of the user |
| GET | `/auth/recovery/{link}` | Public → `303` | Link from `leavo recovery-login`: valid → session cookie and `/admin`; otherwise `/login?link=used\|expired\|invalid` (FR-067) |
| POST | `/orgs` | Public + `ticket` | Multi-org mode only (`404` in single mode). `{ ticket, name, timezone, leave_year_start_month, full_name }` → `201 { org, user }` + cookie |

`/auth/code` response rules (FR-003):
- `login`: always `202`, whether or not the user exists or is active. A code is sent only to active users.
- `signup`: `422 DOMAIN_NOT_REGISTERED` ("Your company isn't on Leavo yet. Ask your HR to register.") when the email's domain is not whitelisted — this reveals only whether a *company domain* uses this instance, never whether a person has an account. For a whitelisted domain the response is always `202`; if the person already has an account, the email sent is a login code instead.
- `register_org` (multi mode; `404` in single mode): `409 DOMAIN_TAKEN` if the domain belongs to another org; `422 PUBLIC_DOMAIN` for public providers.
- `setup`: requires a valid `setup_token` (`403 SETUP_TOKEN_INVALID`, counts toward the 5-try lock); `422 PUBLIC_DOMAIN` for public providers.
- Rate-limited requests return `429` with `Retry-After` for all purposes.

## Me & dashboard

| Method | Path | Role | Response |
|---|---|---|---|
| GET | `/me` | Any | `{ id, email, full_name, role, department, employee_code, date_of_joining, approver_group, org: { id, name, timezone, weekend, leave_year_start_month }, last_notify: [{ user_id?, email }] }` — `last_notify` = notified members of the user's last request, offered as suggestions |
| PATCH | `/me` | Any | `{ full_name?, employee_code?, department? }` (empty string clears; unknown fields → `422`) — date of joining is set at signup (`/auth/verify` signup payload) and afterwards only by admins (FR-064) |
| GET | `/me/export` | Any | Download own personal data as JSON: profile, leave and comp-off history, ledger rows (FR-045) |
| GET | `/me/balances?year=` | Any | `[{ leave_type: {id, code, name, color, is_comp_off}, leave_year, allotted, carried, taken, adjusted, pending, available, next_expiry? }]` — only types available to the user; `available = balance − pending`; a leave year that has not started shows its projected allotment (FR-055) |
| GET | `/me/leaves?year=&type=&status=&cursor=&limit=` | Any | `{ items: [history item], next_cursor }`, newest first by start date. `year` = leave year (any day in it), `type` = leave type id, `status` = request status; a bad value → `422 VALIDATION` with `details.field` |
| GET | `/me/leaves.csv?year=&type=&status=` | Any | CSV export of every matching request (csv-formats.md §7) |
| GET | `/away?range=today\|week\|month&scope=org\|team` | Any | Role-shaped list (below). `week` = Monday–Sunday containing today, `month` = calendar month, org time zone; default `today`/`org`. `team` = the viewer's approver group, the groups they approve, or their department |
| GET | `/team/calendar?month=YYYY-MM&scope=org\|team` | Any | Same field rule as `/away`, per booked day: `{ month, days: [{ date, people: [{ user: {id, full_name}, half: null\|"first"\|"second", leave_type?, reason? }] }] }`; days with nobody away omitted; default month = current |
| GET | `/users/search?q=` | Any | `[{ id, full_name, email, avatar_url? }]`, q ≥ 2 chars, same org, active users, max 10 |

`/away` item for **employee** viewers (clarification Q1):
```json
{ "user": { "id": "…", "full_name": "Ravi K" }, "from": "2026-12-24", "to": "2026-12-29", "days": 2.5 }
```
For **eligible approvers** of that user and **admins**, items additionally carry `"leave_type": {id, code, name, color}` and `"reason"` (FR-058). Response envelope: `{ "count_today": 4, "items": [...] }`, approved leave only, sorted by `from`.

History item:
```json
{ "id": "…", "kind": "leave", "leave_type": {"code":"CL","name":"Casual Leave","color":"#3B82F6"},
  "from": "2026-12-12", "to": "2026-12-14", "start_half": null, "end_half": null, "days": 2,
  "status": "approved", "requested_at": "…", "decided_by": {"full_name":"Priya"}, "decided_at": "…",
  "reason": "Family function", "decision_comment": "Enjoy!", "notify": ["Ravi K", "client@x.com"],
  "attachment": { "id": "…", "name": "note.pdf" } }
```

## Leave requests

| Method | Path | Role | Purpose |
|---|---|---|---|
| POST | `/attachments` | Any | multipart `file` (PDF/PNG/JPEG/WebP/HEIC, ≤ 5 MB) → `201 { id, name, content_type, size }` |
| GET | `/attachments/{id}/url` | Owner, their approvers, AD | `{ url, expires_at }` (5 min); others → `404`. S3: a pre-signed bucket URL; local storage: the download link below |
| GET | `/attachments/{id}/download?o=&exp=&sig=` | Signed link (no session) | The file with `Content-Disposition: attachment`; expired or tampered → `403 FORBIDDEN` |
| POST | `/leaves/preview` | Any | Same body as create → `{ days, excluded: [{date, reason: "weekend"\|"holiday", name?}], by_leave_year: [{leave_year, days, available_after}], errors: [{code, message}] }` — optional holidays are not excluded (FR-016); a range across the leave-year boundary returns one entry per year (FR-055); an empty reason is not reported while previewing |
| POST | `/leaves` | Any | Create (requires `Idempotency-Key`; missing → `422 VALIDATION` `details.field: "Idempotency-Key"`) → `201 { id, status: "pending", days, excluded, by_leave_year }`; a broken rule → its catalogue code (the first one; `INSUFFICIENT_BALANCE` carries `details.leave_year`); invalid notify entry → `422 VALIDATION` `details.field: "notify"`; someone else's or already-used attachment → `details.field: "attachment_id"` |
| GET | `/leaves/{id}` | Owner, eligible approvers (from `routed_group_id`), AD | History-item shape plus `user`, `version`; others → `404` |
| POST | `/leaves/{id}/cancel` | Owner | `{ reason? }` (≤ 500) → `200 { status }`. Pending: `cancelled`, days released. Approved and today (org time zone) is before `from`: `cancelled`, every day returned (`leave_reversed` per leave year). Started (today ≥ `from`): `cancel_requested`; nothing changes until an approver decides — on acceptance only days dated on/after the org-local date the cancellation was requested are returned and only those day rows are deactivated (FR-020). Eligible approvers who are not the owner → `403 FORBIDDEN`; anyone else → `404`; rejected, cancelled or already `cancel_requested` → `409 ALREADY_DECIDED`; long reason → `422 VALIDATION` `details.field: "reason"`. No `Idempotency-Key`: a repeat is a `409` |

Create body:
```json
{
  "leave_type_id": "…", "from_date": "2026-12-24", "to_date": "2026-12-29",
  "start_half": null, "end_half": "first",
  "reason": "Family trip", "attachment_id": null,
  "notify": [{ "user_id": "…" }, { "email": "client@acme.com" }]
}
```

## Approvals

| Method | Path | Role | Purpose |
|---|---|---|---|
| GET | `/approvals?status=pending&kind=leave\|comp_off\|cancel` | A, AD (employees → `403`) | Oldest first. Item: `{ id, kind, requester: {id, full_name}, leave_type, from, to, start_half, end_half, days, reason, requested_at, version, attachment, balance: {available}, team_away: [{full_name, from, to, leave_type?, reason?}] }` — `team_away` = approved leave of the requester's current group or department overlapping the dates; `leave_type`/`reason` only for viewers eligible for that teammate (FR-058). `kind=cancel` items (oldest cancellation first) add `cancel: { requested_at, reason, days_returned }`; `kind=comp_off` items: `leave_type` = the comp-off type, `from`/`to` = first/last worked date, `team_away: []`, plus `dates: [{date, portion}]`; decided with `POST /approvals/comp_off/{id}/decide` (approve creates one credit per date with a `comp_off_credit` ledger row; reject frees the dates) |
| POST | `/approvals/{kind}/{id}/decide` | A, AD (eligible, not requester; requires `Idempotency-Key`) | `{ decision: "approve"\|"reject", comment?, version, action_token? }` → `200 { status, decided_by: {id, full_name}, decided_at }`; `409 ALREADY_DECIDED` with `details { status, decided_by, decided_at }`; `403 SELF_APPROVAL` / `NOT_ELIGIBLE_APPROVER`; `403 FORBIDDEN` when `action_token` is not this approver's valid link for this decision; `422 COMMENT_REQUIRED` |
| GET | `/approvals/action/{token}` | Public → `303` | Valid → `/approvals?request={id}&decision=…&t={token}` (session required on landing; opening never decides); otherwise `/approvals?link=used\|expired\|invalid`. Links last 72 h and all links of a request retire when it is decided |

`kind=cancel` items are `cancel_requested` leaves; `POST /approvals/cancel/{id}/decide` with `approve` accepts the cancellation (`200 { status: "cancelled" }`) and `reject` keeps the leave (`200 { status: "approved" }`, comment required). Same eligibility, `version` and `409 ALREADY_DECIDED` rules as leave decisions; a leave decision on a `cancel_requested` request (or the reverse) is `409`. Email action links exist only for `kind=leave`. The request's original decision fields are kept; accept/decline is recorded in the audit log.

## Comp-offs

| Method | Path | Role | Purpose |
|---|---|---|---|
| GET | `/comp-offs?user=` | Any (`user` = another employee: AD only, else `403`) | `{ available, next_expiry: {date, days}\|null, credits: [{id, worked_date, days, days_used, days_reserved, days_expired, status: "active"\|"used"\|"expired"\|"revoked", approved_by, expires_on, used_on: [{leave_request_id, from, to, days}], revoke_reason}], requests: [{id, status, reason, dates: [{date, portion}], days, requested_at, decided_by, decided_at, decision_comment, version}] }` — credits soonest expiry first, requests newest first |
| POST | `/comp-offs` | Any (requires `Idempotency-Key`) | `{ dates: [{ date, portion: 1\|0.5 }], reason, notify: [...] }` → `201 { id, status: "pending", days }`. 1–31 distinct dates; each past (org time zone) and a weekend day or **mandatory** holiday, not claimed by a pending/approved request, else `422 COMP_OFF_DATE_INVALID` with `details.date` (the first bad date). Bad list/portion → `VALIDATION details.field: "dates"`; no active comp-off type → `TYPE_NOT_AVAILABLE`. Approvers get action links (`request_kind: comp_off`); notified members a minimal FYI |
| POST | `/comp-offs/{id}/cancel` | Owner | Withdraw a pending comp-off request (FR-030) → `200 { status: "cancelled" }`; its dates can be claimed again. Eligible approvers → `403`; others → `404`; not pending → `409 ALREADY_DECIDED` |
| POST | `/admin/comp-off-credits/{id}/revoke` | AD | `{ reason }` — only if unused and unreserved, else `422 CREDIT_IN_USE`; missing reason → `422 NOTE_REQUIRED`; already revoked → `409 ALREADY_DECIDED`. Writes `comp_off_revoked −(days − days_expired)` and audit `comp_off.revoke`; returns the credit (FR-061) |

## Holidays

| Method | Path | Role | Purpose |
|---|---|---|---|
| GET | `/holidays?year=` | Any | `[{ id, date, name, kind, location }]` of a leave year; year ∈ {current, next} for employees, 2000–2200 for admins |
| GET | `/holiday-packs` | AD | `[{ id: "IN/2027", name, count }]` |
| POST | `/admin/holidays` | AD | `{ date, name, kind?, location? }` → `201` holiday. Same date and name (ignoring case) exists → `422 VALIDATION details.field: "name"` |
| PATCH / DELETE | `/admin/holidays/{id}` | AD | PATCH any of `date`, `name`, `kind`, `location` (null clears) → `200`; DELETE → `204`. Every add/edit/delete/import applies FR-035 to the dates whose mandatory status changed: a new mandatory day frees booked days (`leave_reversed` for approved, recount for pending); a day that stops being one (deleted, made optional, or moved away) is booked again as the request's half on its edge days, charged with `leave_taken` for approved even below zero. Weekends are never booked. Affected approved requests email the employee (`holiday_changed_leave`), and admins too when a day is charged again |
| POST | `/admin/holidays/import?dry_run=` | AD | multipart CSV (`file`) or `{ pack: "IN/2027" }` or `{ copy_from_year: 2026, to_year: 2027 }` → `200 { dry_run, added: [{date, name, kind, location}], skipped: [{date, name, reason: "exists"}], errors: [{row, code, message}] }`. Rows already in the calendar are skipped. Applying with CSV row errors → `422 IMPORT_HAS_ERRORS`, nothing written. Copy shifts dates by whole years (29 Feb → 28 Feb). Unknown pack → `VALIDATION field: pack` |

## Admin: organization & policy

| Method | Path | Role | Purpose |
|---|---|---|---|
| GET / PATCH | `/admin/org` | AD | name, timezone, weekend, leave_year_start_month (`409 LEAVE_YEAR_LOCKED` once leave data exists), carry_forward_cap, comp_off_expiry_days. Timezone/weekend changes apply to new requests only (FR-062) |
| POST | `/admin/org/sign-out-all` | AD | Revoke every session in the org, including the caller's (FR-063) |
| GET / POST | `/admin/org/domains` | AD | add `{ domain }`; `409 DOMAIN_TAKEN`, `422 PUBLIC_DOMAIN` |
| DELETE | `/admin/org/domains/{id}` | AD | |
| GET / POST | `/admin/leave-types` | AD | |
| PATCH | `/admin/leave-types/{id}` | AD | Quota change requires `apply_from: "now"\|"next_year"` |
| GET / POST | `/admin/leave-types/{id}/grants` | AD | GET `?include_revoked=true` adds history. POST `{ user_id }` or `{ email }` → `201` grant + full-year allotment (once per user/type/leave year); `200` if already granted; `422` for a non-restricted type |
| DELETE | `/admin/leave-types/{id}/grants/{user_id}` | AD | revoke |
| GET | `/admin/instance` | AD | `{ version, mode, update_check: {enabled, latest_version, last_checked_at, update_available}, email: {sent_24h, failed_24h, last_error, last_error_at}, jobs: {queued, dead, oldest_queued_seconds}, storage?, migrations?: {applied, pending} }` — `storage`/`migrations` only in single mode; in multi mode email and job figures cover only the caller's org (FR-051) |
| POST | `/admin/instance/test-email` | AD | Queues a test email to the caller → `202 { to }` |
| GET | `/admin/carry-forward/preview?year=` | AD | `year` = the leave year being closed (default current). `[{ user: {id, full_name, email}, leave_type: {id, code, name, color}, unused, carry, lapse }]` per active employee and carry-forward type with a non-zero balance; `carry` is negative for a carried deficit, `lapse` counts positive days lost. Before year end it is the plan the job will write; after, it is read back from the ledger — the same numbers. `/admin/carry-forward/preview.csv` downloads it |
| GET | `/admin/carry-forward/preview.csv?year=` | AD | The same rows as CSV `email,full_name,leave_type,unused,carry,lapse` (csv-formats.md §7) |

Leave type body:
```json
{ "code": "SL", "name": "Sick Leave", "color": "#EF4444", "yearly_quota": 6,
  "allow_half_day": true, "allow_negative": false, "max_days_per_request": null,
  "min_notice_days": 0, "max_backdate_days": 7, "negative_limit": 0, "attachment_required_above_days": 2,
  "availability": "all", "receives_carry_forward": false, "active": true }
```

## Admin: people, balances, reports

| Method | Path | Role | Purpose |
|---|---|---|---|
| GET | `/admin/users?q=&status=&role=&group=&cursor=&limit=` | AD | `{ items: [{ id, email, full_name, role, status, employee_code, department, date_of_joining, approver_group: {id, name}\|null, anonymised }], next_cursor }` by name; `q` matches name, email or employee code |
| GET | `/admin/users/{id}` | AD | one person, same shape |
| POST | `/admin/users` | AD | invite one `{ email, full_name, employee_code?, department?, date_of_joining?, role?, approver_group_id? }` → `201` user (`invited`), this year's quotas allotted (pro-rated), `invite` email. Email not on an org domain → `422 DOMAIN_NOT_REGISTERED`; already a user anywhere on the instance → `VALIDATION field: email`; employee code taken → `field: employee_code` |
| POST | `/admin/users/import?dry_run=` | AD | multipart CSV `file` (csv-formats §1) → `{ dry_run, to_create, skipped: [{row, email, reason: "exists"}], warnings: [GROUP_WILL_BE_CREATED], errors: [{row, code, message}] }`; apply is all-or-nothing (`422 IMPORT_HAS_ERRORS`), creates missing groups |
| PATCH | `/admin/users/{id}` | AD | profile incl. date_of_joining (no automatic re-allotment, FR-064), role, group, status (`LAST_ADMIN` guard). Setting `deactivated` revokes all sessions (FR-006) |
| POST | `/admin/users/{id}/export` | AD | personal data export (JSON), any user (FR-045) |
| POST | `/admin/users/{id}/anonymise` | AD | deactivated users only (`422 VALIDATION field: status`; `409 ALREADY_ANONYMISED`): replaces name/email everywhere incl. audit log, deletes reasons, comments and attachments (rows and stored objects), keeps dates, days and ledger (FR-045). Ledger notes are append-only and stay |
| GET | `/admin/users/{id}/balances?year=` | AD | `{ balances: [as /me/balances], ledger: [{ id, leave_type: {id, code, name}, leave_year, delta, reason, note, ref_kind, ref_id, created_by, created_at }] }` — the year's entries plus every comp-off entry |
| POST | `/admin/users/{id}/adjustments` | AD | `{ leave_type_id, leave_year, delta, note }` → `201` ledger entry (`manual_adjustment`, author recorded). Missing/blank note → `422 NOTE_REQUIRED`; delta 0 or not a multiple of 0.5 → `VALIDATION field: delta`; audit `balance.adjust` with balance before/after |
| GET / POST | `/admin/approver-groups` | AD | `{ name, member_ids: [] }` → `{ id, name, members: [{id, full_name, email, role}], employees }`. Members must be approvers or admins (not deactivated); names unique (case-insensitive). Demoting someone to employee removes their memberships; deleting a group leaves its employees without one |
| PATCH / DELETE | `/admin/approver-groups/{id}` | AD | PATCH `{ name?, member_ids? }` (members replaced) → the group; DELETE → `204`, its employees have no group |
| GET | `/admin/dashboard` | AD | `{ away_today: [{user, from, to, days, leave_type}], pending, pending_over_48h, upcoming_holidays: [next 3], next_year_holidays_missing, setup_checklist: { holidays, approvers, invite } }` — `pending` counts leave, comp-off and cancellation requests; "over 48 h" uses the org's reminder threshold; the holiday reminder starts two months before the next leave year (1 November for a January year) |
| GET | `/admin/reports/leaves?group_by=type\|user\|month&year=` | AD | `[{ group, key, leave_type: {id, code, name}, days }]` — booked days of approved leave (and days kept when started leave was cancelled) in the leave year; `.csv` = `group,leave_type,days` (FR-044) |
| GET | `/admin/reports/leaves.csv?group_by=&year=` | AD | The same report as CSV `group,leave_type,days` |
| GET | `/admin/audit-log?actor=&entity=&action=&from=&to=&cursor=&limit=` | AD | `{ items: [{ id, actor, action, entity_kind, entity_id, before, after, ip, created_at }], next_cursor }`, newest first; `entity` = id or kind; `action` ending in `.` matches a prefix; dates in org time zone, inclusive |
| POST | `/admin/import/{kind}?dry_run=true\|false` | AD | `kind` = `keka` or `csv`; multipart CSV(s), see [csv-formats.md](csv-formats.md); parts `balances` (required), `history` (optional); Keka also `leave_type_map` (JSON object Keka name → code) and `leave_year` (default current). Response `{ dry_run, users_to_create: [{email, full_name, approver_group}], groups_to_create, balances_to_set: [{row, email, leave_type_code, leave_year, available_days}], history_rows: [{row, email, leave_type_code, from, to, days}], errors: [{file, row, code, message}], warnings: [...], keka_leave_types?: [{name, code}], skipped_history? }`. Apply is all-or-nothing and returns `422 IMPORT_HAS_ERRORS` if any error row exists (FR-043) |

## Operational endpoints (outside `/api/v1`)

| Path | Purpose |
|---|---|
| `GET /healthz` | Liveness: `200 ok` |
| `GET /readyz` | DB reachable and migrations applied |
| `GET /metrics` | Prometheus; requires `Authorization: Bearer $LEAVO_METRICS_TOKEN` when set |
| `GET /*` | Embedded SPA (fallback to `index.html`) |

---

## Appendix: Requirement traceability

Every functional requirement maps to an API operation, a CLI/config entry ([configuration.md](configuration.md)), a file format ([csv-formats.md](csv-formats.md)), or a background job / documentation deliverable ([plan.md](../plan.md)).

| FR | Covered by |
|---|---|
| 001–003 | `/auth/code`, `/auth/verify`; `rate_limits` |
| 004 | `/auth/code` (signup), `/auth/verify` |
| 005, 006 | session cookie rules; `/auth/logout?all=true`; `PATCH /admin/users/{id}` (deactivate) |
| 007 | `/setup/*`; CLI `leavo setup-token` |
| 008 | `LEAVO_MODE`; `POST /orgs` |
| 009 | `/admin/org/domains` |
| 010, 062 | `/admin/org` |
| 011, 056 | `/admin/leave-types` |
| 012 | `/admin/leave-types/{id}/grants` |
| 013, 064 | `/auth/verify` signup payload; `PATCH /admin/users/{id}`; allotment on signup |
| 014 | `LAST_ADMIN` on `PATCH /admin/users/{id}` |
| 015–019, 055, 057 | `/attachments`, `/leaves/preview`, `POST /leaves` (+ `Idempotency-Key`) |
| 020 | `/leaves/{id}/cancel`; `/approvals?kind=cancel` |
| 021 | `/admin/users/{id}` (role), `/admin/approver-groups` |
| 022–024 | `/approvals`, `/approvals/{kind}/{id}/decide`, `/approvals/action/{token}` |
| 025, 060, 070 | jobs `approval_reminder`, `comp_off_expiry`, scheduler catch-up |
| 026 | `/me/balances` |
| 027, 058 | `/away`, `/team/calendar`, field rule in Conventions |
| 028 | `/me/leaves`, `/me/leaves.csv` |
| 029 | `/attachments/{id}/url`, field rule in Conventions |
| 030, 031 | `/comp-offs`, `/comp-offs/{id}/cancel` |
| 032 | `/comp-offs`; job `comp_off_expiry` |
| 061 | `/admin/comp-off-credits/{id}/revoke` |
| 033 | `/holidays` |
| 034 | `/admin/holidays`, `/admin/holidays/import`, `/holiday-packs`; `holidays/IN/2026.csv`, `IN/2027.csv` |
| 035 | `/admin/holidays` (side effects on booked days) |
| 036 | `/admin/dashboard` (`next_year_holidays_missing`) |
| 037, 038 | job `year_end_rollover` |
| 039 | `/admin/carry-forward/preview` |
| 040 | ledger on every write; `/admin/users/{id}/balances` |
| 041 | `/admin/users/{id}/adjustments` |
| 042 | `/admin/users`, `/admin/users/import` |
| 043 | `/admin/import/{kind}` (`keka`, `csv`) |
| 044 | `/admin/dashboard`, `/admin/reports/leaves`, `/admin/audit-log` |
| 045 | `/me/export`, `/admin/users/{id}/export`, `/admin/users/{id}/anonymise` |
| 046, 047, 059 | job `send_email` (outbox); email templates |
| 048 | all endpoints (tenant from session; RLS) |
| 049 | all date handling (org time zone) |
| 050 | Versioning in Conventions; migrations on start-up |
| 051 | `/admin/instance`, `/admin/instance/test-email` |
| 052 | `LEAVO_UPDATE_CHECK`; no telemetry |
| 053 | `SMTP_URL`, `STORAGE`, `S3_*` |
| 054 | `locales/en.json` |
| 063 | `/admin/org/sign-out-all`; `LEAVO_SECRET_KEY` rotation |
| 065 | logs; `/metrics`; operator docs |
| 066 | demo instance (release deliverable) |
| 067 | CLI `leavo recovery-login` |
| 068 | backup & restore guide |
| 069 | migration runner behaviour; `/readyz` |
