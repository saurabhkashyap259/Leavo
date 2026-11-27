# Data Model: Leavo v1

**Feature**: [spec.md](spec.md) · **Research**: [research.md](research.md) · **Date**: 2026-11-24

Conventions:
- IDs are UUID v7 (`uuid`), generated in the application.
- Dates are `date`; instants are `timestamptz`. "Today" and leave years are computed in the org's time zone.
- Day quantities are `numeric(5,1)` and always a multiple of 0.5.
- Every tenant table carries `org_id` and has a Row-Level Security policy `org_id = current_setting('app.org_id')::uuid` (FR-048).
- Balances are never stored; they are derived from `balance_ledger` (FR-040).
- Emails are `citext`, stored trimmed and lower-cased.

---

## Entity overview

```text
organizations 1─* org_domains
organizations 1─* users *─0..1 approver_groups 1─* approver_group_members *─1 users
organizations 1─* leave_types 1─* leave_type_grants *─1 users
users 1─* leave_requests 1─* leave_request_days
leave_requests 1─0..1 attachments
leave_requests / comp_off_requests 1─* request_notifications
users 1─* comp_off_requests 1─* comp_off_request_dates
comp_off_request_dates 1─0..1 comp_off_credits 1─* comp_off_usages *─1 leave_requests
users 1─* balance_ledger *─1 leave_types
organizations 1─* holidays
organizations 1─* rollover_runs
users 1─* sessions ; login_codes (by email) ; action_tokens ; audit_log ; jobs ; rate_limits ; instance_settings
```

---

## Organization & access

### organizations
| Field | Type | Rules |
|---|---|---|
| id | uuid PK | |
| name | text | 1–120 chars |
| timezone | text | IANA name, default `Asia/Kolkata` |
| weekend_mask | smallint | bitmask Mon=1…Sun=64, default Sat+Sun (96); ≥1 working day |
| leave_year_start_month | smallint | 1–12, default 1; changeable only while the org has no requests and no ledger rows other than initial allotments (FR-062) |
| carry_forward_cap | numeric(5,1) | ≥ 0, default 0 |
| comp_off_expiry_days | int | 1–365, default 90 |
| reminder_after_hours | int | default 48 |
| timezone / weekend changes | — | apply to requests submitted afterwards and to scheduling from then on; existing `leave_request_days` are not recounted (FR-062) |
| created_at | timestamptz | |

### org_domains
| Field | Type | Rules |
|---|---|---|
| id | uuid PK | |
| org_id | uuid FK | |
| domain | citext | **UNIQUE globally**; lower-case, no `@`, valid hostname; not in public-provider blocklist (FR-009) |
| created_at | timestamptz | |

Removing a domain blocks new signups only; existing users keep access.

### users
| Field | Type | Rules |
|---|---|---|
| id | uuid PK | |
| org_id | uuid FK | |
| email | citext | UNIQUE globally |
| full_name | text | 1–120 chars |
| employee_code | text NULL | unique per org when set |
| department | text NULL | used by "My team" filter |
| date_of_joining | date NULL | drives pro-rating (FR-013); set by employee at signup, afterwards admin-only; changes do not recompute allotments (FR-064) |
| role | enum `employee \| approver \| admin` | |
| status | enum `invited \| active \| deactivated` | deactivated → cannot sign in (FR-006) |
| approver_group_id | uuid NULL FK | at most one group (FR-021) |
| last_notify_list | jsonb | last used notify members (suggestion) |
| anonymised_at | timestamptz NULL | set by anonymisation (FR-045): name → `Former employee <short id>`, email → `anon-<id>@invalid`; reasons and attachments deleted; dates, days and ledger kept |
| created_at | timestamptz | |

No gender or parent-status fields (clarification Q5).

Invariant: each org has ≥ 1 user with `role = admin AND status = active` (FR-014), enforced in the service layer under an org advisory lock.

### approver_groups / approver_group_members
- `approver_groups(id, org_id, name UNIQUE per org, created_at)`
- `approver_group_members(group_id, user_id, PK(group_id,user_id))` — member must have role `approver` or `admin`.

Routing (FR-022): requester's group members (or all approvers if no group) ∪ all admins, minus the requester. If the resulting set is empty, request is routed to all admins except the requester; if still empty the submit fails with `NO_APPROVER_AVAILABLE` and the setup checklist warns.

### login_codes
| Field | Type | Rules |
|---|---|---|
| id | uuid PK | |
| email | citext | |
| purpose | enum `login \| signup \| register_org \| setup \| recovery` | `recovery` = operator-issued admin sign-in, 15-min lifetime, created only by `leavo recovery-login` (FR-067) |
| code_hash | bytea | HMAC-SHA256 |
| payload | jsonb | e.g. signup name, org registration details |
| expires_at | timestamptz | now + 10 min |
| attempts | smallint | locked after 5 |
| consumed_at | timestamptz NULL | issuing a new code consumes older ones (FR-002) |
| created_at | timestamptz | |

### sessions
`id, user_id, token_hash bytea UNIQUE, expires_at, absolute_expires_at, keep_signed_in bool, user_agent, ip inet, created_at, last_seen_at, revoked_at`.
Lifetime 7 d or 30 d; extended when used within the last 24 h, never beyond `absolute_expires_at` = sign-in + 90 d (FR-005). "Sign out everywhere" revokes the user's rows; deactivation does the same (FR-006); "sign out everyone" revokes all rows of the org (FR-063). Rotating `LEAVO_SECRET_KEY` makes every token, code hash and action link unverifiable.

### action_tokens
`id (nonce), org_id, request_id, request_kind, approver_id, decision, expires_at (72 h), used_at`.

### rate_limits
`bucket text, key text, window_start timestamptz, count int, PK(bucket,key,window_start)`; buckets: `code_email_30s`, `code_email_1h`, `code_ip_1h`, `verify_lock`.

### instance_settings (singleton, not tenant-scoped)
`id = 1, mode (single|multi), setup_token_hash, setup_token_failed_attempts, setup_token_locked_until, setup_completed_at, last_update_check_at, latest_version_seen`. The setup token is cleared when setup completes (FR-007).

---

## Policy

### leave_types
| Field | Type | Rules |
|---|---|---|
| id | uuid PK | |
| org_id | uuid FK | |
| code | text | 1–6 uppercase chars, UNIQUE per org |
| name | text | |
| color | text | hex `#RRGGBB` |
| yearly_quota | numeric(5,1) NULL | NULL only for comp-off; multiple of 0.5 |
| is_comp_off | bool | exactly one per org |
| receives_carry_forward | bool | v1: true only for Casual (CL) |
| allow_half_day | bool | default true |
| allow_negative | bool | default false |
| negative_limit | numeric(5,1) | ≥ 0, default 5; used only when `allow_negative` (FR-057) |
| max_days_per_request | numeric(5,1) NULL | |
| min_notice_days | int | default 0; applies only to requests starting today or later |
| max_backdate_days | int | 0–30, default 0; a request may start this many days before today (FR-017) |
| attachment_required_above_days | numeric(4,1) NULL | clarification Q4; seed SL = 2 |
| availability | enum `all \| restricted` | clarification Q5 |
| active | bool | inactive → not requestable, history kept |
| sort_order | int | |

Seeds: CL 12 (carry forward), SL 6 (attachment > 2, `max_backdate_days` 7), CCL 10 restricted, ML 130 restricted (26 weeks × 5 working days), PL 5 restricted, CO comp-off. Quotas are editable defaults pending HR confirmation (dependency D2). Back-dating and notice behaviour come only from these fields, never from the type's code or name.

Quota change mid-year: admin must choose `apply_from = now | next_year` (FR-056). `now` writes a `quota_change` ledger row per affected user (formula in [Pro-rating and quota changes](#pro-rating-and-quota-changes)); `next_year` stores `pending_yearly_quota numeric(5,1) NULL`, applied and cleared by the next rollover.

### leave_type_grants
| Field | Type | Rules |
|---|---|---|
| id | uuid PK | |
| org_id | uuid | |
| leave_type_id | uuid FK | type must be `restricted` |
| user_id | uuid FK | |
| granted_by | uuid FK users | |
| granted_at | timestamptz | |
| revoked_by / revoked_at | NULL | revocation blocks new requests; pending ones can still be decided |

Partial UNIQUE `(leave_type_id, user_id) WHERE revoked_at IS NULL`. Grant inserts a full-year `allotment` ledger row for the current leave year (once per user/type/year).

### holidays
`id, org_id, date, name, kind (mandatory|optional), location text NULL, source (manual|csv|pack:<id>), UNIQUE(org_id, date, name)`.
Only `mandatory` holidays are excluded from day counts and are valid comp-off dates; `optional` holidays are ordinary working days in v1 (FR-016, FR-031).
- Adding a mandatory holiday inside approved leave → deactivate that `leave_request_days` row and write `leave_reversed` (+portion); inside pending leave → deactivate the row and reduce `days`.
- Deleting a mandatory holiday or changing it to optional inside approved leave → insert a new active `leave_request_days` row and write `leave_taken` (−portion), even if the balance goes negative; email employee and admins; pending requests are recounted without ledger rows (FR-035).

---

## Requests

### leave_requests
| Field | Type | Rules |
|---|---|---|
| id | uuid PK | |
| org_id, user_id, leave_type_id | uuid | type must be active and available to user |
| from_date, to_date | date | to ≥ from |
| start_half, end_half | enum `first \| second` NULL | only if type allows half day |
| days | numeric(5,1) | > 0, computed by engine |
| reason | text | 1–500 chars |
| status | enum (below) | |
| routed_group_id | uuid NULL | requester's approver group at submit time. The request's eligible approvers (who may decide it and see its type/reason) are always computed from this value, never from the requester's current group (FR-022, FR-058) |
| decided_by, decided_at, decision_comment | | comment required on reject |
| cancel_requested_at, cancel_reason | NULL | for after-start cancellation |
| version | int | optimistic lock |
| created_at, updated_at | timestamptz | |

**States** (FR-018, FR-020, FR-023):

```text
            submit
              │
              ▼
          ┌────────┐  withdraw          ┌───────────┐
          │pending │──────────────────▶ │ cancelled │
          └────────┘                    └───────────┘
           │      │ reject                    ▲  ▲
   approve │      ▼                           │  │ cancel before start
           │  ┌────────┐                      │  │
           │  │rejected│                      │  │
           ▼  └────────┘                      │  │
      ┌────────┐ ─────────────────────────────┘  │
      │approved│                                 │
      └────────┘ ── request cancel after start ──▶ ┌──────────────────┐
                                                   │cancel_requested  │
                                                   └──────────────────┘
                                                     │ accept → cancelled
                                                     │ decline → approved
```

Enum: `pending, approved, rejected, cancelled, cancel_requested`.

Ledger effects:
- approve → `leave_taken −days`.
- cancel of approved leave before start → `leave_reversed +days`; all day rows deactivated.
- accepted `cancel_requested` (leave already started) → `leave_reversed +` the portions of day rows dated on or after the cancellation request date; only those rows are deactivated. Days already passed stay taken. `days` on the request is not rewritten; the ledger carries the truth.
- pending → no ledger rows; reflected in `pending`.

### leave_request_days
`leave_request_id, org_id, user_id, day date, leave_year int, portion numeric(2,1) ∈ {0.5,1.0}, half enum NULL, active bool`. `leave_year` is the leave year the day falls in; ledger rows for a request are written per leave year (FR-055).
- Partial UNIQUE `(user_id, day, half) NULLS NOT DISTINCT WHERE active` (a NULL half is the full day).
- Engine additionally rejects a full day and a half day on the same date.
- `active=false` on reject / cancel / holiday added.

### attachments
`id, org_id, owner_user_id, request_id NULL (UNIQUE when set), storage_key, file_name, content_type, size_bytes ≤ 5 MB, created_at`. Uploaded before submit (unattached rows garbage-collected after 24 h).

### request_notifications
`id, org_id, request_id, request_kind (leave|comp_off), user_id NULL, email citext NOT NULL`, UNIQUE `(request_kind, request_id, email)`. ≤ 20 per request; external emails must be syntactically valid.

### comp_off_requests / comp_off_request_dates
- `comp_off_requests`: `id, org_id, user_id, reason (1–500), status (pending|approved|rejected|cancelled), decided_by, decided_at, decision_comment, version, created_at`.
- `comp_off_request_dates`: `id, comp_off_request_id, worked_date, portion (0.5|1.0)`. Each date must be past and either a weekend day (org mask) or a holiday; UNIQUE active claim per `(user_id, worked_date)` across pending/approved requests (FR-031).

### comp_off_credits
`id, org_id, user_id, comp_off_request_date_id, worked_date, days, days_used, days_reserved, expires_on = worked_date + org.comp_off_expiry_days, expired_at NULL, revoked_at NULL, revoked_by NULL, revoke_reason NULL`. Created on approval with ledger `comp_off_credit +days`.
- `days_reserved` = days held by pending comp-off leave requests; the expiry job expires only `days − days_used − days_reserved`, and re-checks a credit when a reservation is released after `expires_on` (FR-060).
- Admin revocation allowed only when `days_used = 0 AND days_reserved = 0`; writes `comp_off_revoked −days` (FR-061).

### comp_off_usages
`comp_off_credit_id, leave_request_id, days`. Filled at approval of a comp-off leave, consuming credits ordered by `expires_on` (FR-032); reversed on cancellation.

---

## Ledger & audit

### balance_ledger
| Field | Type | Rules |
|---|---|---|
| id | uuid PK | |
| org_id, user_id, leave_type_id | uuid | |
| leave_year | int | year in which the leave year **starts**; for the comp-off type it records when the entry happened but balances ignore it (FR-032) |
| delta | numeric(6,1) | non-zero |
| reason | enum | `allotment, prorate, quota_change, carry_forward, negative_carry, lapse, leave_taken, leave_reversed, comp_off_credit, comp_off_expired, comp_off_revoked, manual_adjustment, import` |
| ref_kind / ref_id | text / uuid NULL | request, credit, rollover run, import batch, grant |
| note | text NULL | **required** for `manual_adjustment` (FR-041) |
| created_by | uuid NULL | NULL for system jobs |
| created_at | timestamptz | |

Append-only: the app role has `INSERT, SELECT` only. Index `(user_id, leave_type_id, leave_year)`.

### v_ledger_balances (view) — created with the ledger (migration 0005)
Per `(user_id, leave_type_id, leave_year)`, from `balance_ledger` only:
- `allotted` = Σ allotment + prorate + quota_change + import
- `carried` = Σ carry_forward + negative_carry
- `taken` = −Σ leave_taken − Σ leave_reversed (net)
- `adjusted` = Σ manual_adjustment + comp_off_credit + comp_off_expired + comp_off_revoked + lapse
- `balance` = Σ delta
- **Comp-off type**: one balance across all leave years (Σ of all comp-off rows), i.e. `leave_year` is not part of its grouping (FR-032).

### v_balances (view) — created with requests (migration 0006)
`v_ledger_balances` joined with pending days from `leave_request_days` of `pending` requests, grouped by `leave_year`:
- `pending` = Σ portion of active days of `pending` requests in that leave year
- `available` = balance − pending
- **Future leave year** (not yet rolled over): `balance` = projected allotment (quota — the pending quota if one is scheduled — pro-rated, or granted) + Σ any rows already written for that year; carry forward appears only after rollover (FR-055). The view holds ledger + pending; the application adds the projection (`leaves_repo::projected_allotment`) while the year is in the future and has no allotment rows.
- **Negative limit**: a request is refused when `available − days < −negative_limit` (or `< 0` if `allow_negative = false`).

### rollover_runs
`org_id, leave_year, started_at, finished_at, summary jsonb, PK(org_id, leave_year)` — idempotency guard (FR-038).
Per user and type at rollover: positive unused casual → `carry_forward +min(unused, cap)` (new year) and `lapse −rest` (old year); positive unused of other types → `lapse`; any negative balance → `lapse +|deficit|` (old year, closes it at 0) and `negative_carry −|deficit|` (new year) (FR-057). Comp-off untouched.

### audit_log
`id, org_id, actor_id NULL, action text (e.g. leave.approve, policy.update, user.deactivate, auth.recovery_login, org.sign_out_all), entity_kind, entity_id, before jsonb, after jsonb, ip inet, created_at`. Retained 7 years by default. Leave reasons are stored in `before/after` only for leave entities and the endpoint is admin-only. Anonymisation rewrites the user's name, email and leave reasons inside these JSON values; the rows themselves are kept (FR-045).

### idempotency_keys
`org_id, user_id, key text, endpoint text, request_hash bytea, status smallint, response jsonb, created_at, PK(user_id, key)`. Create endpoints for leave and comp-off requests require an `Idempotency-Key`; a repeat with the same key within 24 h returns the stored response, a repeat with a different body returns `409 IDEMPOTENCY_MISMATCH` (FR-019). Rows deleted after 24 h by `cleanup`.

### jobs
`id, kind, org_id NULL, payload jsonb, run_at, attempts, max_attempts=5, status (queued|running|done|failed|dead), locked_by, locked_at, last_error, dedupe_key UNIQUE NULL, created_at`.
Scheduled kinds use `dedupe_key = kind:org:period` (e.g. `reminder:<org>:<request>:<day>`, `rollover:<org>:<year>`). On start-up the scheduler enqueues every period missed since its last tick, so work due while the instance was down runs once on restart without duplicate emails (FR-070).

---

## Error code catalogue

This is the single list of error codes returned by the API (`{"error":{"code",...}}`). [contracts/http-api.md](contracts/http-api.md) refers here and must not introduce codes that are not listed.

### Domain rules (engine, `leavo-core`) — HTTP 422 unless noted

| Code | Rule | Source |
|---|---|---|
| `INVALID_RANGE` | to < from | FR-017 |
| `ZERO_WORKING_DAYS` | no working days in range | FR-017 |
| `HALF_DAY_NOT_ALLOWED` | half day on type without it | FR-011 |
| `INSUFFICIENT_BALANCE` | days > available in any affected leave year and `allow_negative = false` | FR-017, FR-055 |
| `NEGATIVE_LIMIT_EXCEEDED` | would go below −`negative_limit` | FR-057 |
| `OVERLAP` | any active day overlaps (incl. half/full) | FR-017 |
| `BACKDATE_NOT_ALLOWED` | `from` earlier than today − `max_backdate_days` of the type | FR-017 |
| `NOTICE_TOO_SHORT` | `from` ≥ today and `from − today < min_notice_days` (back-dated requests are exempt) | FR-017 |
| `MAX_DAYS_EXCEEDED` | days > max_days_per_request | FR-017 |
| `ATTACHMENT_REQUIRED` | days > attachment_required_above_days and no attachment | FR-017, Q4 |
| `TYPE_NOT_AVAILABLE` | inactive, or restricted without active grant | FR-012 |
| `TOO_MANY_NOTIFY` | > 20 notify members | FR-015 |
| `REASON_REQUIRED` / `REASON_TOO_LONG` | 1–500 chars | FR-015 |
| `COMP_OFF_DATE_INVALID` | not past, not weekend/mandatory holiday, or already claimed | FR-031 |
| `CREDIT_IN_USE` | revoking a comp-off credit that is used or reserved | FR-061 |
| `NO_APPROVER_AVAILABLE` | routing finds nobody other than the requester | FR-022 |
| `SELF_APPROVAL` (403) | decider = requester | FR-022 |
| `NOT_ELIGIBLE_APPROVER` (403) | decider not in the request's approver set | FR-022 |
| `ALREADY_DECIDED` (409) | version mismatch / not pending | FR-023 |
| `ALREADY_ANONYMISED` (409) | anonymising a person who has already been anonymised | FR-045 |
| `COMMENT_REQUIRED` | reject without comment | FR-023 |
| `NOTE_REQUIRED` | manual adjustment or credit revocation without reason | FR-041, FR-061 |
| `LAST_ADMIN` | would leave org without active admin | FR-014 |
| `LEAVE_YEAR_LOCKED` (409) | changing leave-year start month after leave data exists | FR-062 |
| `QUOTA_APPLY_FROM_REQUIRED` | quota change without `apply_from` | FR-056 |

### Organization and domains

| Code | HTTP | Rule | Source |
|---|---|---|---|
| `DOMAIN_NOT_REGISTERED` | 422 | signup email domain not whitelisted anywhere | FR-003, FR-004 |
| `DOMAIN_TAKEN` | 409 | domain belongs to another org | FR-009 |
| `PUBLIC_DOMAIN` | 422 | public email provider | FR-009 |
| `INVALID_DOMAIN` | 422 | not a valid hostname | FR-009 |

### Access, transport and generic

| Code | HTTP | Meaning |
|---|---|---|
| `UNAUTHENTICATED` | 401 | no or expired session |
| `FORBIDDEN` | 403 | role or ownership check failed |
| `CSRF` | 403 | missing `X-Requested-With` header |
| `SETUP_TOKEN_INVALID` | 403 | wrong or used setup token (FR-007) |
| `CODE_INVALID` | 422 | wrong, expired or consumed code |
| `LOCKED` | 429 | 5 wrong codes or setup-token tries; `Retry-After` set |
| `RATE_LIMITED` | 429 | FR-003 limits; `Retry-After` set |
| `NOT_FOUND` | 404 | includes other-org resources (never 403, to avoid leaking existence) |
| `IDEMPOTENCY_MISMATCH` | 409 | same `Idempotency-Key`, different body (FR-019) |
| `VALIDATION` | 422 | field-level schema errors in `details` |
| `FILE_TOO_LARGE` / `FILE_TYPE_NOT_ALLOWED` | 413 / 415 | attachments |
| `IMPORT_HAS_ERRORS` | 422 | apply attempted while dry run reports errors (FR-043) |
| `INTERNAL` | 500 | unexpected; request id in `details` |

Import row codes are listed in [contracts/csv-formats.md](contracts/csv-formats.md) §5.

## Pro-rating and quota changes

- **Joiners** (FR-013): `prorated = round_half(quota × remaining_months / 12)`. The joining month counts if joined on or before the 15th, otherwise counting starts next month. Joined before the leave year started → full quota.
- **Quota change "apply from now"** (FR-056): each affected user gets `quota_change` = `round_half((new − old) × remaining_months / 12)`, remaining months counted from the month of the change with the same 15th rule. "Next year" stores `pending_yearly_quota` for the next rollover.
- **Restricted grant** (FR-012): full `yearly_quota`, not pro-rated. Revocation writes `lapse −(balance − pending)` when positive.
- `round_half(x) = round(x × 2) / 2`.
