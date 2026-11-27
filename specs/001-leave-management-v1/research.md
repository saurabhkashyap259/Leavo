# Phase 0 Research: Leavo v1

**Feature**: [spec.md](spec.md) · **Plan**: [plan.md](plan.md) · **Date**: 2026-11-24

The ERD fixes most of the stack, so this file records each decision with its reason and the alternatives that were rejected, and settles the points the ERD and spec left open (including the five clarifications from 2026-11-24). No `NEEDS CLARIFICATION` items remain.

---

## R1. Language and runtime

- **Decision**: Rust (stable, edition 2021, MSRV 1.83) with Tokio and Axum 0.8 for HTTP.
- **Rationale**: Team already writes Rust (BRD C1). Single static binary, low memory, fits the 1 vCPU / 1 GB target (SC-013).
- **Alternatives**: Node/TypeScript end to end (one language for contributors, but larger runtime footprint, weaker compile-time SQL checks); Python/FastAPI (team knows it, but heavier image and slower for the leave engine property tests).

## R2. Database access and migrations

- **Decision**: PostgreSQL 15+ via SQLx 0.8 (compile-time checked queries, `sqlx::migrate!` embedded migrations). Offline query metadata (`.sqlx/`) committed so contributors can build without a live DB.
- **Rationale**: Postgres is the single required dependency. SQLx keeps SQL visible (good for reviewers of balance logic) and catches schema drift at compile time.
- **Alternatives**: SeaORM / Diesel (more abstraction, harder to express advisory locks and partial unique indexes); raw `tokio-postgres` (no compile-time checks).
- **Migration policy**: forward-only; run at start-up under `pg_advisory_lock(<constant>)` so two containers starting together don't race (FR-050).

## R3. Tenant isolation

- **Decision**: Two layers. (1) Every repository function takes `OrgId` and filters on it. (2) Postgres Row-Level Security on every tenant table with policy `org_id = current_setting('app.org_id')::uuid`; the API runs `SET LOCAL app.org_id` at the start of each transaction. The app connects as a non-owner role (`leavo_app`) so RLS applies; migrations run as the owner role.
- **Rationale**: FR-048. RLS turns a missed `WHERE` into an empty result instead of a leak.
- **Alternatives**: Schema-per-tenant (complex migrations, poor fit for single-org default); database-per-tenant (operator burden).
- **Note**: Pre-session tables (`login_codes`, `org_domains` lookup during signup) are queried through a narrow `SECURITY DEFINER` function or a dedicated unscoped repository; documented in the code review checklist.

## R4. Authentication (email OTP)

- **Decision**: 6-digit code from `OsRng`, stored as `HMAC-SHA256(LEAVO_SECRET_KEY, lower(email) || ':' || code)`; constant-time compare (`subtle`). Session = 32 random bytes, SHA-256 stored, cookie `leavo_sid` (HttpOnly, Secure, SameSite=Lax). 7 days, or 30 with "keep me signed in"; sliding within the last 24 h, capped at 90 days from sign-in (FR-005). Deactivation and "sign out everyone" revoke sessions immediately (FR-006, FR-063).
- **Rate limits**: stored in Postgres (`rate_limits` table keyed by `(bucket, key, window_start)`) rather than in-memory `governor`, so limits hold across replicas and restarts. Per email: 1 / 30 s, 5 / h. Per IP: 20 / h. Verify: 5 attempts per code then 15-min lock (FR-001–FR-003).
- **Enumeration**: for login, `/auth/code` always returns `202` with the same body and similar latency (work is queued regardless). For signup, an unregistered *domain* is refused immediately: domain registration is treated as public, person-level account existence is not (FR-003).
- **Recovery**: `leavo recovery-login <email>` creates a `recovery` login code for an active admin and prints a one-time link (15 min). Needs shell access, so it doesn't widen the network attack surface (FR-067).
- **CSRF**: SameSite=Lax + required `X-Requested-With: leavo` header on non-GET requests.
- **Alternatives**: Magic links only (break on corporate link scanners that pre-click); JWT in localStorage (XSS exposure); passwords (rejected by BRD O5).

## R5. Email action links

- **Decision**: Token = base64url(`request_id`, `decision`, `approver_id`, `expires_at`, nonce) + HMAC. Stored nonce in `action_tokens` with `used_at` to make it single-use; 72 h expiry. Link opens `/approvals/{id}?decision=approve&t=…`; the page needs an active session and the approver confirms (FR-024).
- **Alternatives**: One-click approve without session (rejected: email forwarding becomes an approval).

## R6. Balance model and concurrency

- **Decision**: Append-only `balance_ledger`; balance = `SUM(delta)` per (user, type, leave year). Pending = sum of pending request days. View `v_balances` exposes allotted / carried / taken / pending / available.
- **Concurrency**: `pg_advisory_xact_lock(hashtextextended(user_id::text || leave_type_id::text, 0))` around submit / approve / cancel; partial unique index on `leave_request_days (user_id, day, coalesce(half,'full')) WHERE active` plus an exclusion check in the engine for full-vs-half conflicts on the same date; optimistic `version` column for decisions → `409 ALREADY_DECIDED` (FR-018, FR-019, FR-023).
- **Rationale**: Makes SC-010 (100% reconciliation) structural rather than a test target.
- **Alternatives**: Mutable `balances` table (simpler reads, but disputes can't be explained and concurrent updates need row locks anyway).

## R7. Day counting, half days, time zones

- **Decision**: Pure function `working_days` in `leavo-core` using `chrono` + `chrono-tz`. Weekend days stored per org as a 7-bit mask. "Today" and leave year derived from org time zone. A single-day request with a half day counts 0.5. Half days on non-working days are dropped.
- **Testing**: `proptest` over random ranges, weekends and holiday sets (invariants: count ≤ calendar days, no weekend/holiday included, half-day only at edges).

## R8. Restricted leave types (clarification Q5)

- **Decision**: `leave_types.availability` enum `all | restricted`. New table `leave_type_grants (user_id, leave_type_id, granted_by, granted_at, revoked_at)`. Granting writes a full-year `allotment` ledger row for the current leave year (not pro-rated); year-end rollover allots restricted types only to users with an active grant. No gender / parent fields on `users`.
- **Rationale**: Removes sensitive personal data (DPDP minimisation), works for all family situations.
- **Alternatives**: Rule-based eligibility via profile fields (rejected in clarification).

## R9. Attachment-required threshold (clarification Q4)

- **Decision**: `leave_types.attachment_required_above_days numeric(4,1) NULL`. Engine validation `ATTACHMENT_REQUIRED` when `days > threshold` and no attachment. Seed Sick Leave = 2 (editable).

## R10. Who's away visibility (clarifications Q1, Q3)

- **Decision**: `/away` returns a different DTO by viewer role: employees get `{user, from, to, days}` only (no type id, no colour); approvers get type + reason for users in their approver group; admins for all. Query parameter `scope=org|team` where team = same approver group OR same department. UI renders employee view in a neutral colour. Same projection is reused by `/team` calendar.
- **Rationale**: Enforced server-side so the client never receives hidden fields.

## R11. Background jobs

- **Decision**: Postgres `jobs` table (outbox). API inserts jobs in the same transaction as the change. Worker loop: `SELECT … FOR UPDATE SKIP LOCKED LIMIT n`, exponential backoff, max 5 attempts, dead-letter status visible on Admin → Instance. Scheduler ticks every minute and enqueues cron jobs keyed by `(job_kind, org_id, period)` to stay idempotent. On start-up it back-fills every period missed since the last recorded tick, so a stopped or scaled-to-zero instance catches up once, without duplicate emails (FR-070). `leavo serve` runs worker in-process when `LEAVO_RUN_JOBS=true`; `leavo worker` runs it alone.
- **Alternatives**: Redis / SQS queues (extra dependency, violates "Postgres only"); `pg_cron` (not available on all managed DBs).

## R12. Year-end rollover

- **Decision**: Daily job at 00:30 org time; acts when today is the first day of the org's leave year. Guarded by `rollover_runs (org_id, leave_year)` unique row inserted in the same transaction as ledger rows (FR-037, FR-038). Carry = `min(unused_casual, cap)`; lapse = remainder, written in the old year. Negative balances of any type are closed in the old year and carried as `negative_carry` into the new one (FR-057). Comp-off is untouched (FR-032). Preview endpoint uses the same pure function without writing.

## R13. Email delivery

- **Decision**: `lettre` 0.11 SMTP transport behind `EmailSender` trait (SMTP impl + in-memory test impl). Templates with `minijinja`, HTML + plain text for every message. `.ics` attachments generated with the `icalendar` crate. Code subject: `123456 is your Leavo code`.
- **FYI emails to notified members** contain only requester name, dates, days and status; no type, reason, attachment or action links (FR-059). Approver emails carry type and reason, since approvers are eligible viewers (FR-058).
- **Alternatives**: MJML (needs Node at build time); provider-specific HTTP APIs (lock-in; SMTP works everywhere — FR-053).

## R14. File storage

- **Decision**: `object_store` crate with `LocalFileSystem` (default `/data/uploads`) or `AmazonS3` (any S3-compatible endpoint). Type sniffed with `infer`; allow PDF/PNG/JPEG/WebP/HEIC; 5 MB cap enforced while streaming. Downloads via signed, 5-minute URLs served by the API for local storage, pre-signed URLs for S3.

## R15. Web application

- **Decision**: React 19 + TypeScript 5 + Vite 6, TanStack Query + TanStack Router, Tailwind CSS 4, Radix UI primitives, `react-hook-form` + `zod`, `i18next` with `locales/en.json`. Built assets embedded into the binary with `rust-embed`; SPA fallback for client routes. API types generated from the OpenAPI document (`openapi-typescript`).
- **Accessibility**: Radix primitives for keyboard/focus; `axe` checks in Playwright; 360 px layout tested (SC-016).
- **Alternatives**: Server-rendered templates with htmx (smaller, but richer forms like live day counts and auto-complete are easier in React; contributors are more familiar with React).

## R16. API contract

- **Decision**: JSON over `/api/v1`, cursor pagination, single error shape `{error:{code,message,details?}}`. OpenAPI 3.1 generated from Rust types with `utoipa` and checked in at `contracts/openapi.yaml`; CI fails on drift.

## R17. Observability

- **Decision**: `tracing` + `tracing-subscriber` JSON logs with request id and org id; redaction layer for `code`, `token`, `reason`, `email` local-part. `/metrics` via `metrics-exporter-prometheus` (protected by optional bearer token `LEAVO_METRICS_TOKEN`). Optional `sentry` when `SENTRY_DSN` set. No outbound telemetry by default; update check (GitHub releases API, daily) disabled by `LEAVO_UPDATE_CHECK=false` (FR-051, FR-052).

## R18. Testing stack

- **Decision**: `cargo test` + `proptest` (core), `sqlx::test` with testcontainers Postgres (integration, cross-org access matrix, concurrency with parallel Tokio tasks), `vitest` + Testing Library (web), Playwright + Mailpit against the built image (E2E, install smoke test), upgrade test (previous release image → new image, balances unchanged — SC-014).

## R19. Packaging and release

- **Decision**: Multi-stage Dockerfile (node build → cargo build with cargo-chef → `gcr.io/distroless/cc` runtime), multi-arch via `docker buildx`, published to GHCR as `:X.Y.Z`, `:X.Y`, `:X`; signed with cosign. `release-please` for versions and CHANGELOG. Docs site: **mdBook** (Rust-native, no Node needed for docs builds) published to GitHub Pages.
- **Alternatives**: Docusaurus (nicer theming, extra Node toolchain for docs).

## R20. Imports

- **Decision**: Built-in importer in the API (`/admin/import/{keka|csv}`) parsing CSV with the `csv` crate; dry run returns a diff without writing; apply runs in one transaction per batch of 500 rows. `scripts/keka_import.py` stays as a developer tool to map raw Keka exports into the canonical CSV. Formats in [contracts/csv-formats.md](contracts/csv-formats.md).

## R21. Holiday packs

- **Decision**: `holidays/<COUNTRY>[-<REGION>]/<YEAR>.csv` (`date,name,kind`), embedded into the binary at build time and listed in the setup wizard. CI validates format, dates within the year, no duplicates.

## R22. Leave-year boundaries, negative balances, idempotency (checklist review)

- **Decision**: Each booked day stores its `leave_year`; the engine groups days by year and checks each group against that year's available balance. Future-year availability = projected allotment − pending/approved days in that year (FR-055).
- **Negative limit**: per-type `negative_limit` (default 5) when `allow_negative` (FR-057).
- **Duplicate submissions**: `Idempotency-Key` header + `idempotency_keys` table (24 h) on create and decide endpoints (FR-019). Simpler than content-based dedupe and safe across retries.
- **Alternatives**: charging the whole request to the start date's year (simple, but wrong for December leave running into January); blocking cross-year requests (bad UX at the holidays).

## R23. Deferred, non-blocking

- Product name and licence (AGPL-3.0 vs MIT): pre-public-release decisions; code uses the working name "Leavo" behind a single `PRODUCT_NAME` constant and `LICENSE` is added before the repo goes public.
- Exact HR quota numbers: configuration, seeded with BRD example values.
- Helm chart: after v1.

---

## Implementation notes (deviations recorded during `/speckit-implement`)

| Date | Area | Plan said | Actual | Why |
|---|---|---|---|---|
| 2026-11-24 | SQL access | SQLx compile-time `query!` macros + committed `.sqlx/` | Runtime `query`/`query_as` with `FromRow` | Macros need a live DB or offline cache on every build; every query is exercised by integration tests against real PostgreSQL. Revisit if query drift becomes a problem. |
| 2026-11-24 | RLS role | App connects as non-owner `leavo_app` | App pool runs `SET ROLE leavo_app` on every connection; migrations run on an owner pool | Self-hosters typically have one DB user (often a superuser, which bypasses RLS). |
| 2026-11-24 | Templates | minijinja (2.x era) | minijinja 3.x (latest) | Latest-versions policy. |
| 2026-11-24 | Web lint | ESLint | oxlint (Vite's current template) + Prettier | Latest-versions policy; Vite scaffolds oxlint now. |
| 2026-11-24 | Web TypeScript | TypeScript 5 | TypeScript 7 for the app; `web/tools/openapi/` keeps TypeScript 5 for openapi-typescript | openapi-typescript (latest 7.13) still peers on TypeScript 5. |
| 2026-11-24 | Dev mail | MailHog | Mailpit | MailHog is unmaintained. |
| 2026-11-24 | Session cookie | Cookie Max-Age = 7 d / 30 d | Cookie Max-Age = 90 d (absolute cap); the database holds the real sliding 7 d / 30 d expiry | A 7-day cookie would be dropped by the browser even after the server slid the session forward. The server is the source of truth (FR-005). |
| 2026-11-24 | Code rate limits | Count issued codes | Every `/auth/code` request is recorded per email and per IP, whether or not the email exists | Counting only issued codes would make the 6th request 429 for real users but 202 for unknown emails — an enumeration leak (FR-003). |
| 2026-11-24 | E2E seeding | Setup wizard creates the org | US1 E2E inserts an org + domain directly (`web/e2e/support/env.ts`) | The wizard is US2 (T081/T086); switch the seed to the wizard once it exists. |
| 2026-11-25 | Setup admin code | `/auth/verify` returns a ticket for `setup` | `/auth/code` takes `setup_token` for purpose `setup`; `POST /setup/complete` checks the admin's code itself | One request creates the org, so a code can't be verified and then lost to a failed create; the token gate stops anyone sending codes before setup. |
| 2026-11-25 | `register_org` ticket | Unspecified | Stateless HMAC ticket `base64url(email).expiry.mac` (30 min), keyed by `LEAVO_SECRET_KEY` | No table needed; replay is harmless because the domain is re-checked (`DOMAIN_TAKEN`) on create. |
| 2026-11-25 | Setup token on start | Generated on first start | Generated on first `serve` only if none exists; later starts print a hint to run `leavo setup-token` | Restarting must not silently invalidate a token the operator already copied. |
| 2026-11-25 | Grants by email | `{ user_id }` | Also `{ email }` | The policy page grants by typing an email until user search exists (US3 `/users/search`). |
| 2026-11-25 | Seeded restricted quotas | "BRD example values" | CCL 10, ML 130, PL 5 | BRD leaves them TBD; a restricted type with quota 0 would make grants useless. Editable; HR to confirm (D2). |
| 2026-11-25 | Holiday packs | IN 2026, 2027 | 17 gazetted Central Government holidays each, all `mandatory`, sources in `holidays/README.md` | Companies observe a subset; admins mark the rest optional or delete them after import. |
| 2026-11-25 | Time zone list | Browser list | Legacy ICU names mapped to current IANA names (`Asia/Calcutta` → `Asia/Kolkata`, …) | Chromium lists legacy names, so `Asia/Kolkata` was missing from the picker (found by `setup.spec.ts`). |
| 2026-11-25 | Admin checklist | `/admin/dashboard` `setup_checklist` | US2 checklist is client-side: an item is done once the admin opens its page (localStorage) | The dashboard endpoint is US10; holidays, approvers and invite items join with their stories. |
| 2026-11-25 | E2E database | Dev database | E2E uses its own `leavo_e2e` database, created before the server starts | `setup.spec.ts` resets the instance to a fresh install; that must never wipe a developer's dev data. |
| 2026-11-25 | Test containers | Testcontainers cleans up | `testkit` removes its container in a process-exit hook (`dtor`) | Rust testcontainers has no reaper by default; every test binary run left a PostgreSQL container running (97 had piled up and new ones began timing out). |
| 2026-11-25 | S3 test server | MinIO testcontainer | SeaweedFS S3 gateway (`chrislusf/seaweedfs`) with an identity file | MinIO stopped publishing container images (Docker Hub and Quay refuse pulls). SeaweedFS is Apache-2.0, S3-compatible, and with identities configured it verifies signatures, so the tamper check is real. |
| 2026-11-25 | Future-year projection | Inside `v_balances` | `v_balances` = ledger + pending; `leaves_repo::projected_allotment` adds a not-yet-started year's quota | Projection needs the org's "today" and the core pro-rating rule; duplicating it in SQL would drift. A year is projected only while it is in the future and has no allotment rows (the rollover writes them). Preview, create and `/me/balances?year=` share it. |
| 2026-11-25 | Booked-day index | `(user_id, day, coalesce(half,'full'))` | `(user_id, day, half) NULLS NOT DISTINCT WHERE active` | PostgreSQL rejects the enum-to-text cast in an index expression (not immutable). Same guarantee; needs PostgreSQL 15+, which the plan already requires. |
| 2026-11-25 | Submit locking | `lock_balance(user, type)` | Also a per-employee `requests:<user>` lock | The balance lock is per type, so a full day of one type and a half day of another on the same date could race past the engine's overlap check. The concurrency test covers it. |
| 2026-11-25 | Idempotency | Tower middleware | `middleware::idempotency` helpers called inside the handler's transaction | The key row is claimed in the same transaction as the request, so a failed attempt frees the key and a concurrent retry waits and replays. Keys older than 24 h are treated as free. |
| 2026-11-25 | Preview reason | — | Preview ignores an empty reason (no `REASON_REQUIRED`) | The preview runs while dates are picked, before a reason is typed; create still requires it. |
| 2026-11-25 | Notify suggestions | — | `/me` gains `last_notify` (from `users.last_notify_list`) | "Last-used suggestions" (T108) need the list on the client; additive field. |
| 2026-11-25 | Local downloads | "Signed URLs served by the API" | `GET /attachments/{id}/download?o=&exp=&sig=` with an HMAC over org, id and expiry | Opens in a new tab without the session cookie being involved; 5-minute expiry checked against the server clock. S3 uses real pre-signed URLs. |
| 2026-11-25 | Request email buttons | Approve/Reject action links | Both open `/approvals?request=…&decision=…` | One-click action tokens are US4 (T119); the template already has the two buttons. |
| 2026-11-25 | After submitting | — | The apply page stays, confirms, clears the form and refreshes balances | The dashboard cards are US5; until then the type list shows the new "available". |
| 2026-11-25 | Cleanup job | `cleanup` deletes idempotency keys and unattached uploads after 24 h | Not built: no task covers it | Correctness does not depend on it (expired keys are ignored); storage grows until it exists. Needs a task (worker job + scheduler entry). |
| 2026-11-25 | Calendar invite | `icalendar` crate | `leavo_email::ics::leave_event`, written by hand | One all-day VEVENT needs only escaping, CRLF and line folding; output is exact and unit-tested. The unused dependency was dropped. |
| 2026-11-25 | Action links | Token = base64url(fields) + HMAC; link opens `/approvals/{id}?decision=…&t=…` | Token = `<nonce>.<expiry>.<hmac>` (fields live in the `action_tokens` row and are covered by the HMAC); `GET /api/v1/approvals/action/{token}` → `303 /approvals?request=…&decision=…&t=…`, or `?link=used\|expired\|invalid` | Shorter links. Opening a link never decides (link scanners pre-click); a decision retires every link of the request, which makes "works once" hold even for the unused button. |
| 2026-11-25 | Scheduler and RLS | — | `job_org_ids()` SECURITY DEFINER (migration 0007) lists org ids; each periodic job then reads inside an org-scoped transaction | `organizations` is RLS-protected and the scheduler has no tenant. |
| 2026-11-25 | Reminders | "pending > 48 h, not again within 24 h" | Dedupe key `approval_reminder:<org>:<request>:<n>` where n = completed 24 h windows past `reminder_after_hours`; the handler re-checks the request is still pending | Idempotent across instances and restarts; a decision between scheduling and sending suppresses the email. |
| 2026-11-25 | Forbidden transitions | — | Every forbidden state change returns `ALREADY_DECIDED` | One catalogue code clients already handle; the 409 details say what the request is now. |
| 2026-11-26 | Visibility rule | Checks spread across handlers | `leavo_core::visibility::can_see_details(viewer, subject)`; callers pass the request's `routed_group_id` (request level) or the person's current group (person level). The approvals inbox, `/away` and `/team/calendar` all use it | One tested rule for FR-058. A demoted employee keeps no access through stale group membership: the role decides. |
| 2026-11-26 | Who's away ranges | "today / this week / this month" | `week` = Monday–Sunday containing today; `month` = calendar month; both in the org time zone. Items = approved (and cancel-requested) requests overlapping the range; `count_today` = people with a booked day or half day today | Matches how people read a wall calendar; counting booked days means a weekend inside a range never counts as "away today". |
| 2026-11-26 | Team scope | Same approver group OR department | Also the groups the viewer approves, so an approver's "My team" is the people they approve | Approvers usually have no group of their own. |
| 2026-11-26 | Team calendar shape | "Same projection as /away, per day" | `{ month, days: [{ date, people: [{ user, half, leave_type?, reason? }] }] }` from booked days; days with nobody away are omitted | Exact half days and no weekends/holidays without client-side calendar logic. |
| 2026-11-26 | History filters | `year`, `type`, `status` | `year` = leave year (a request matches every leave year one of its days is in); `type` = leave type id; newest first by start date; keyset cursor (base64url of start date + id) | Stable paging while new requests arrive. |
| 2026-11-26 | History CSV | Columns per csv-formats.md §7 | Dates in org time zone; `half_day` = "first half"/"second half" for a single day, else "first day: …; last day: …"; `notified` joined with "; "; values starting with `= + - @` are prefixed with `'` | Same fields as the table; guards against spreadsheet formula injection. |
| 2026-11-26 | `/holidays` read side | US8 (T163) | `GET /holidays?year=` (current or next leave year, for every role until US8 widens it for admins) shipped with US5 for the dashboard's "next 3 holidays"; `holidays_test.rs` started here, US8 extends it | The dashboard needs it now; admin CRUD and imports remain in US8. |
| 2026-11-26 | Reconciliation test | Red first | `balances_reconcile_test.rs` passed on first run: balances were built in US1/US3. It now guards SC-010 with seeded random scenarios (and asserts the scenarios really create and decide requests) | A guard test, not new behaviour. |
| 2026-11-26 | "Started" leave | FR-020 "before its start date" | Approved leave counts as started from its first day (org time zone): cancelling on the first day needs an approver. A pending request is always withdrawn directly, even after its start date | The first day is already under way; no approval was given yet for pending requests, so there is nothing to undo |
| 2026-11-26 | Cancel endpoint | Idempotency on mutating POSTs | `POST /leaves/{id}/cancel` takes no `Idempotency-Key`: the status check makes a repeat a `409 ALREADY_DECIDED`, never a double reversal. Accept/decline reuse `POST /approvals/cancel/{id}/decide` (keyed, versioned) | A cancel has no body worth replaying; reusing decide keeps one inbox, one dialog and one race rule |
| 2026-11-26 | Cancellation emails | T142: `leave_cancelled`, `cancel_requested` | Added a third template, `cancel_declined`, for the employee when an approver keeps the leave. Notified members get the existing minimal `leave_decided_fyi` with status `cancelled` (FR-059). No calendar update is sent on cancellation | FR-046 lists cancellations for all parties; a declined cancellation must reach the employee. Calendar removal (METHOD:CANCEL, or a shortened event for partial returns) is not required by FR-046 and is left for later |
| 2026-11-26 | Approver UI for cancellations | Not in T143 | The approvals page shows a "Cancellation requests" section (`kind=cancel`) with Accept cancellation / Keep leave, reusing the decision dialog | Without it approvers could only accept through the API; covered by tests in `approvals.test.tsx` |
| 2026-11-26 | Comp-off claimable dates | data-model says "weekend day or a holiday" in one place, "mandatory" in another | Only past weekend days (org mask) and **mandatory** holidays; optional holidays are working days (FR-031, data-model §holidays) | FR-031 says an optional holiday is a working day |
| 2026-11-26 | Comp-off credit bookkeeping | `comp_off_credits.days_used/days_reserved`, `comp_off_usages(credit, request, days)` | Added `days_expired` and `warned_at` to credits; `comp_off_usages` keeps `days_reserved` and `days_used` per (credit, request). Submit reserves soonest-expiry first (expired credits excluded); approval turns that request's reservation into use (topping up FIFO if a manual adjustment left it short); reject/withdraw releases it; cancellation returns days to the request's credits latest-expiry first, so the soonest-expiring credit keeps the days already taken. Released or returned days on a credit past `expires_on` expire immediately with `comp_off_expired` (FR-060) | Partial expiry and the 7-day warning need per-credit state; per-request rows make release and partial return exact |
| 2026-11-26 | Comp-off expiry date | "expires after the org's expiry period" | `expires_on = worked_date + comp_off_expiry_days`; days expire at the start of that date (org time zone). Job `comp_off_expiry` runs once per org per local day from 00:15; the warning goes once, when `expires_on` is within 7 days | One unambiguous rule for the UI ("expires on …") and the job |
| 2026-11-26 | Expiry test location | T147: `crates/worker/tests/comp_off_expiry_test.rs` | `crates/api/tests/comp_off_expiry_test.rs`, driving the real scheduler (`TestApp::tick`) and runner | Credits are created through the API instead of hand-written SQL; the job and its registration are both exercised |
| 2026-11-26 | Comp-off emails | T155: `comp_off_requested_action`, `comp_off_decided`, `comp_off_expiring` | Added `comp_off_fyi` for notified members (request and decision; name, dates, days, status only — FR-059). Approvers get one-use Approve/Reject links like leave. No confirmation email to the employee on submit, and no email on withdrawal | The leave FYI templates say "leave on …"; notified members still need the minimal copy |
| 2026-11-26 | Admin view of credits | Revoke needs a credit id | `GET /comp-offs?user=` for admins. The admin UI for revoking comes with the employee admin pages (US10) | Keeps one read model for both audiences |
| 2026-11-26 | Migration number | T149: `0007_comp_off.sql` | `0008_comp_off.sql` (0007 was taken by approvals in US4). `crates/db/build.rs` now rebuilds when a migration is added | Migrations are embedded at compile time |
| 2026-11-26 | Holiday side effects scope | FR-035 | Applied per date whose *mandatory* status changed (several holidays may share a date). Pending, approved and cancel-requested requests are affected; cancelled/rejected ones are not. A re-booked edge day takes the request's half (`start_half` on `from`, `end_half` on `to`); weekends are never booked; a day already booked by another request is left alone. `leave_requests.days` follows the booked days (kept unchanged if it would reach 0). Comp-off leave also reserves/uses or releases credit for the day | One rule for add, edit (old and new date), delete and import |
| 2026-11-26 | Holiday change emails | FR-035: employee and admins on re-booking | `holiday_changed_leave` to the employee for approved leave in both directions; admins only when a day is charged again. Pending requests are recounted silently | Admins asked for the re-booking warning; a freed day is good news only the employee needs |
| 2026-11-26 | Holiday CSV errors | csv-formats.md §5 codes | Added `MISSING_VALUE` (empty or over-long name) and `INVALID_KIND`; duplicates compare names ignoring case. Import dry run reports rows already in the calendar as `skipped` (not errors) | Re-importing a pack or file is safe and idempotent |
| 2026-11-26 | Holiday admin and list | T163/T165 | Admins may read any leave year (2000–2200) and the admin page shows previous, current and next year; employees still see current and next. Month view = 12 Monday-first month grids; the admin home links to the page and the "Add this year's holidays" checklist item is ticked on first visit | Admins need last year to copy from; the checklist item existed since US2 |
| 2026-11-26 | Rollover "unused" | data-model §rollover_runs | Unused = the closing year's ledger balance; pending requests are not subtracted. Only active users are rolled over. Inactive types still lapse but get no new quota. Restricted types are allotted at the full quota to active grantees only; other types pro-rated by joining date; no allotment where an `allotment`/`prorate` row for the new year already exists | Matches the balance employees see; avoids double allotment for people who joined on day one. Known gap: a pending request in the old year approved after year end lands in a closed year |
| 2026-11-26 | Rollover schedule | T170 "daily 00:30 org time" | Due from 00:30 on the leave year's first day (or any later tick, FR-070); one job per org and year (`rollover:<org>:<year>`); `rollover_runs.leave_year` is the year being **opened**; an org advisory lock plus the `rollover_runs` row make reruns no-ops. Orgs created after a leave year started are never rolled into it | Without the creation-date guard the first scheduler tick of a new install would roll over a year the org never had |
| 2026-11-26 | Rollover tests location | T167 in `crates/worker/tests` | `crates/api/tests/year_end_rollover_test.rs` with T168, through the real scheduler and runner | Balances come from real requests and the preview/job comparison needs both |
| 2026-11-26 | Carry-forward report | `[{ user, unused, carry, lapse }]`, CSV `email,full_name,unused,carry,lapse` | Added `leave_type` to JSON and CSV (rows are per carry-forward type); after year end the report is read back from the ledger and equals the preview | Keeps rows unambiguous if a second carry-forward type is ever enabled; FR-039 asks for the results before and after |
| 2026-11-26 | Year-end emails | T172 | `rollover_employee` lists each touched type as "Casual Leave: 17 days (12 allotted, 5 carried, 3 lapsed)", sent to every active user (admins included); `rollover_admin_summary` gives totals and attaches the CSV | FR-038 acceptance scenario 4 |
| 2026-11-26 | Invites | FR-042 | Invited users get this year's allotment at invite (activation at first sign-in does not allot). Emails must be on one of the org's domains and new to the instance (emails are instance-unique). CSV invites skip existing people and create missing groups (warning `GROUP_WILL_BE_CREATED`); added row codes `INVALID_EMAIL`, `INVALID_ROLE`, `MISSING_VALUE`, `EMPLOYEE_CODE_TAKEN` | Allotment had no other trigger for invited users |
| 2026-11-26 | Role and status changes | FR-006, FR-014, FR-021 | `LAST_ADMIN` when the change would leave no *active* admin. Demoting to employee removes approver-group memberships. Reactivating someone who never signed in keeps them `invited`. Admins act as approvers everywhere (no membership needed) | Members must be approvers (FR-021) |
| 2026-11-26 | Single-person read | not in contract | Added `GET /admin/users/{id}` for the employee page | Avoids loading the whole list |
| 2026-11-26 | Audit timestamps | — | `audit_log.created_at` is the database's real time, not the app clock; tests filter dates loosely. Recovery-login audit is covered with that command in US12 (T204) | The audit trail records when things really happened |
| 2026-11-26 | Anonymisation scope | FR-045 | Replaces name/email in `users`, notification rows and audit JSON (exact JSON-escaped text), removes `reason`/`comment`/`cancel_reason` from audit entries about the person's own requests, sets reasons to `[removed]` and clears comments, deletes attachment rows then stored objects (after commit), ends sessions. Ledger notes stay (append-only table). New error `ALREADY_ANONYMISED` (409) | Totals stay correct; the ledger's immutability is a stronger guarantee than scrubbing a revoke note |
| 2026-11-26 | Dashboard checklist | US2 client-only checklist | Server facts (`holidays`, `approvers`, `invite`) tick items alongside the pages the admin has opened | A checklist item is done when the work is done, not only when the page was visited |
| 2026-11-26 | Keka column names | No sample export available | Columns matched by a list of usual Keka header names (case-insensitive) and several date formats, in both `importers::keka` and `scripts/keka_import.py`; only `Approved` history is imported; the balance export's leave year is chosen by the admin | **To verify with a real Aktivolabs export before migration**; adding an alias is a one-line change in each place |
| 2026-11-26 | Opening balance semantics | "import row = available_days; compensating row on re-import" | The `import` entry is `available_days − current ledger balance` for that type and year (comp-off: whole pool), so the balance equals the file even for people who already had an allotment; imported people get no allotment. Imported comp-off balances are not backed by credits, so they never expire and FIFO spending finds no credit for them | FR-043 acceptance "balance matches the source file"; comp-off credits need a worked date |
| 2026-11-26 | Import history rows | §3 | Approved requests with booked working days (weekends and mandatory holidays excluded; a `.5` day count books the first half of the single or last day); `decided_by` = `approved_by_email` if that person exists; `decision_comment` "Imported"; no ledger rows. Added `UNKNOWN_USER` and `MISSING_VALUE` errors | Blocks overlaps and shows in history and who's away |
| 2026-11-26 | Import transaction | R20 "batches of 500" | One transaction for the whole apply after a zero-error plan | Simpler and truly all-or-nothing; v1 imports are hundreds of rows |
| 2026-11-26 | Migration failures | FR-069 | `migrate::run` returns `MigrationFailure::Failed { version }` ("migration N failed: …") or `Newer(version)` when the database has a migration the binary doesn't know (a downgrade), with restore-the-backup guidance; `serve` exits before binding and `/readyz` reports pending migrations | Proven by a broken fixture migration and by running the real binary against a "newer" database |
| 2026-11-26 | Catch-up | T209 "back-fill in scheduler.rs" | No scheduler change: every periodic job computes what is due from the clock and dedupes per period (reminder windows, expiry by date, rollover by `rollover_runs`), so the first tick after downtime does exactly the missed work once; a test jumps 3 days across New Year | The design already gives FR-070's "same result as on time, no duplicate emails" |
| 2026-11-26 | Secret rotation | FR-063 | Sessions are stored as plain SHA-256, so a new key alone would not end them. Added `instance_settings.secret_fingerprint` (migration 0009); on start a changed key revokes sessions, consumes unused codes and marks unused action links used | Codes and links were already HMAC-keyed; sessions needed an explicit step |
| 2026-11-26 | Recovery login | FR-067 | Link `/api/v1/auth/recovery/<code id>.<token>`, stored as an HMAC in `login_codes` (purpose `recovery`), 15 min, single use; using it re-checks the admin is still active, starts a normal (short) session and redirects to `/admin` | Reuses the code table; no new secret material |
| 2026-11-26 | Metrics | `metrics` + `metrics-exporter-prometheus` crates | A small per-instance registry rendered as Prometheus text (the crates were removed): a global recorder breaks tests that run many servers in one process. Queue, email-failure and migration figures are read from the database at scrape time | Simpler and testable |
| 2026-11-26 | Update check | FR-052 | `update_check` periodic job only registered when `LEAVO_UPDATE_CHECK=true`; one anonymous GET to GitHub's latest-release API, result in `instance_settings`. The repository URL (`releases::LATEST_RELEASE_URL`, and `ghcr.io/leavo-hq/leavo` in the compose file and docs) is a placeholder to set when the product name and repository are final | Product rename is still open (task file) |
| 2026-11-26 | Packaging | T214/T215 | Image built from `node:lts` + `rust:slim` (cargo-chef) into `distroless/cc-debian12:nonroot` (59 MB, uid 65532), both binaries; compose pins `postgres:18` (the dev stack uses `latest`, now 18). Verified locally: image builds, install smoke test (shipped compose → signed-in admin) took 31 s, upgrade script passes image-to-image | Release workflow (multi-arch, cosign, Trivy) and docs publishing are written but only run on GitHub |
| 2026-11-26 | Upgrade test | T217 | `scripts/ci/upgrade_test.sh` skips (exit 0) until a first release exists; it seeds data with SQL and a known session, then compares `/admin/users/{id}/balances` before and after | There is no previous image yet |
| 2026-11-26 | Instance page extras | — | Added "Sign everyone out…" (FR-063) to the Instance page; the web app had no button for `POST /admin/org/sign-out-all` | Closes a gap from US10 |
| 2026-11-27 | Dependency versions | `object_store` 0.12, `testcontainers` 0.24 era | `object_store` 0.14 (methods moved to the `ObjectStoreExt` trait), `testcontainers` 0.27 / `testcontainers-modules` 0.15 | `cargo audit` advisories in the older trees; latest-versions policy |
| 2026-11-27 | Audit ignores | — | `.cargo/audit.toml` ignores RUSTSEC-2023-0071 (`rsa`) and RUSTSEC-2026-0459 (`libsqlite3-sys`) | Both reach `Cargo.lock` only through SQLx's MySQL/SQLite drivers, which are never enabled or compiled; see SECURITY.md |
| 2026-11-27 | Cross-org matrix | T226 "every route in the OpenAPI document" | `cross_org_matrix_test.rs` reads `ApiDoc::openapi()` and fails when an operation has no matrix entry or exemption; 24 ID-bearing operations are probed with org B's IDs, 4 are exempt (public tokens, self), and 27 list routes are scanned for org B's IDs | A new route cannot ship without being classified |
| 2026-11-27 | i18n check | T229 "every UI string from en.json" | `web/scripts/check-i18n.mjs` also fails on literal JSX text and literal `aria-label`/`title`/`placeholder`/`alt`; it runs in CI (`npm run check:i18n`) | Key-set checks alone missed hard-coded strings (two were found and fixed: the nav label and weekday initials) |
| 2026-11-27 | 360 px layout | — | Table wrappers are `relative overflow-x-auto` | An absolutely positioned `sr-only` header escaped the scroll box and widened the page to 386 px |
| 2026-11-27 | Load check | T225 "seed 1,000 employees" | `leavo-demo-seed --employees N` seeds the organization; `scripts/perf/run.sh` adds a session per person (`perf-<email>`) and runs k6 (local or the `grafana/k6` image). Full profile on a dev laptop: employee dashboard p95 133 ms, admin dashboard p95 37 ms, 0 failed of 6,997 requests (50 VUs, 60 s). 1 vCPU / 1 GB profile (`run.sh small`, 300 employees, 30 VUs): employee p95 197 ms, admin p95 494 ms, 0 failed of 4,204. Sessions are created after the server starts, because a server started with a different secret key signs everyone out | k6 cannot write to PostgreSQL; known session tokens avoid 1,000 email round trips |
| 2026-11-27 | Quickstart E2E | T227 steps 1–11 | Asha is locked in step 3, so a second colleague (Dev) is the non-approver viewer in step 7 and gets the ML grant in step 10; the leave week skips pack holidays; steps 7–11 call the API the screens use | Keeps the journey repeatable on any date |
| 2026-11-27 | Trivy scope | T230 "Trivy on the built image" | The runtime base `distroless/cc-debian12:nonroot` was scanned; the full image scan runs in `release.yml` | Local Docker disk was full; the base holds every OS package in the image (the rest is our binary, covered by `cargo audit`) |
| 2026-11-27 | Open gaps at release review | — | Not built, no task: an admin screen to revoke comp-off credits (the API `POST /admin/comp-off-credits/{id}/revoke` exists; the UI said it would come with US10 but no task added it); one-click accept/decline of cancellations from email links (the cancellation email opens the approvals page instead); the `cleanup` job (see 2026-11-25) | Listed in `checklists/release.md` for a decision before v1.0.0 |
