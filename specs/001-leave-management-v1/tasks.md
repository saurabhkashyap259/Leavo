---
description: "Task list for Leavo v1 — Open-Source Leave Management System"
---

# Tasks: Leavo v1 — Open-Source Leave Management System

**Input**: Design documents from `specs/001-leave-management-v1/` — [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: **TDD is mandatory** (user request; [constitution](../../.specify/memory/constitution.md) Principle X). In every phase, test tasks come first and must be written, run, and seen to **fail (red)** before the matching implementation task starts. An implementation task is done only when the tests it names pass (green) and `cargo clippy -- -D warnings` / `npm run lint` are clean. Refactor only on green.

**Dependencies policy**: Use the **latest stable release** of every dependency at the time the task is executed. Add Rust crates with `cargo add <crate>` (no version pin beyond what `cargo add` writes) and JS packages with `npm install <pkg>@latest`; use the latest stable Rust toolchain and the latest LTS Node, and the latest major tags of `postgres` and `axllent/mailpit` (maintained successor to MailHog, same SMTP port 1025 / UI 8025). Version numbers named in plan.md/research.md are minimums, not pins. If a latest major has breaking API changes against plan.md, follow the new API and note it in research.md. Renovate (T009) keeps them current afterwards.

**Organization**: Grouped by user story (US1–US12 from spec.md) so each story can be built and demonstrated on its own after Phase 2.

## Format: `- [ ] [ID] [P?] [Story?] Description with file path`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[USn]**: user story from spec.md
- Paths follow plan.md: `crates/{core,db,email,storage,worker,importers,api}`, `migrations/`, `web/`, `holidays/`, `scripts/`, `docs/`

## Conventions used by all tasks

- Rust integration tests live in `crates/api/tests/*.rs` and use the shared harness from T036 (Postgres via testcontainers, `FixedClock`, `MemoryEmailSender`, `MemoryStorage`, factories).
- Every new tenant table gets an RLS policy `org_id = current_setting('app.org_id')::uuid` and a row in the cross-org test matrix (T226).
- Every error returned must be a code from the [error code catalogue](data-model.md#error-code-catalogue).
- Every state change writes an `audit_log` row (helper from T040) and enqueues emails through the outbox in the same transaction (helper from T044).
- Web tests: `web/src/**/*.test.tsx` (vitest + Testing Library); E2E: `web/e2e/*.spec.ts` (Playwright + Mailpit).

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Repository skeleton, toolchains, latest dependencies, CI.

- [X] T001 Create Cargo workspace `Cargo.toml` with members `crates/core`, `crates/db`, `crates/email`, `crates/storage`, `crates/worker`, `crates/importers`, `crates/api` (package names `leavo-*`, binary `leavo` in `leavo-api`), and `rust-toolchain.toml` set to `stable` with `clippy`, `rustfmt`
- [X] T002 [P] Scaffold each crate with `src/lib.rs` (and `crates/api/src/main.rs`) and inter-crate path dependencies per plan.md (core has no I/O deps; db→core; email→core; worker→db,email,core; importers→core; api→all)
- [X] T003 Add latest Rust dependencies with `cargo add`: api — `axum`, `tokio` (full), `tower`, `tower-http`, `clap` (derive), `serde`, `serde_json`, `utoipa`, `utoipa-axum`, `rust-embed`, `tracing`, `tracing-subscriber` (json, env-filter), `metrics`, `metrics-exporter-prometheus`, `uuid` (v7), `cookie`, `axum-extra`, `ipnet`; db — `sqlx` (postgres, runtime-tokio, tls-rustls, uuid, chrono, json, migrate, ipnetwork, rust_decimal), `rust_decimal`; core — `chrono`, `chrono-tz`, `rust_decimal`, `thiserror`, `hmac`, `sha2`, `subtle`, `rand`, `serde`; email — `lettre`, `minijinja`, `icalendar`; storage — `object_store` (aws), `infer`; importers — `csv`; dev — `proptest`, `testcontainers`, `testcontainers-modules` (postgres), `reqwest`, `tokio-test`, `insta`; commit `Cargo.lock`
- [X] T004 [P] Scaffold `web/` with `npm create vite@latest web -- --template react-ts`, then `npm install @latest`: `@tanstack/react-query`, `@tanstack/react-router`, `tailwindcss`, `@tailwindcss/vite`, `@radix-ui/react-*` (dialog, popover, select, tabs, toast, checkbox), `react-hook-form`, `zod`, `@hookform/resolvers`, `i18next`, `react-i18next`, `date-fns`, `date-fns-tz`; dev: `openapi-typescript`, `vitest`, `@testing-library/react`, `@testing-library/user-event`, `jsdom`, `@playwright/test`, `@axe-core/playwright`, `eslint`, `prettier`; commit `web/package-lock.json`
- [X] T005 [P] Add `rustfmt.toml`, `clippy.toml`, `.editorconfig`, and `web/eslint.config.js` + `web/.prettierrc` with npm scripts `lint`, `test`, `e2e`, `gen:api`, `build`
- [X] T006 [P] Create `compose.dev.yml` with services `db` (`postgres:latest` major, user/db `leavo`) and `mail` (`axllent/mailpit:latest`, ports 1025/8025), and `.env.example` with the five required variables from contracts/configuration.md
- [X] T007 [P] Update `.gitignore` (target/, web/node_modules, web/dist, .env*, !.env.example, playwright-report/)
- [X] T008 [P] Create `.github/workflows/ci.yml`: jobs `rust` (fmt --check, clippy -D warnings, `cargo test --workspace` with Docker, `cargo sqlx prepare --check`), `web` (npm ci, lint, test, build), `audit` (`cargo audit`, `npm audit --audit-level=high`)
- [X] T009 [P] Add `renovate.json` (group Rust and npm minor/patch weekly, majors as separate PRs, auto-merge patch when CI green)
- [X] T010 [P] Create `locales/en.json` and `web/src/i18n/index.ts` loading it; add a CI check script `web/scripts/check-i18n.ts` that fails on missing keys
- [X] T011 [P] Move requirement docs `docs/*.docx` and the plan image into `docs/requirements/` and create `docs/book/book.toml` (mdBook skeleton, `src/SUMMARY.md`)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared domain types, schema core, tenancy, ledger, outbox, auth extractor, API skeleton, test harness. **No user story starts before this checkpoint.**

### Tests first (write, run, see red)

- [X] T012 [P] Write failing property/unit tests for leave-year and calendar primitives in `crates/core/tests/calendar_primitives_test.rs`: `leave_year_of(date, start_month)` for all 12 start months, `today_in(tz, instant)` across midnight in Asia/Kolkata vs UTC, `WeekendMask` bit ops and "≥1 working day" validation (FR-010, FR-049)
- [X] T013 [P] Write failing tests for `round_half` and pro-rating in `crates/core/tests/prorate_test.rs`: join on 15th counts the month, 16th starts next month, joined before leave year → full quota, results are multiples of 0.5 (FR-013)
- [X] T014 [P] Write failing tests for `Clock` in `crates/core/tests/clock_test.rs` (`FixedClock` advance, `SystemClock` monotonic sanity)
- [X] T015 [P] Write failing table test for the permissions matrix in `crates/core/tests/permissions_test.rs` covering every (role × action) row of ERD §5 plus "never approve own request" (FR-022)
- [X] T016 [P] Write failing test that every `ErrorCode` serialises to `{"error":{"code","message"}}` with the HTTP status from the catalogue in `crates/api/tests/error_shape_test.rs`
- [X] T017 [P] Write failing tests for config loading in `crates/api/tests/config_test.rs`: missing required var names it, `LEAVO_SECRET_KEY` < 64 chars rejected, defaults per contracts/configuration.md
- [X] T018 [P] Write failing RLS isolation test in `crates/db/tests/rls_test.rs`: as role `leavo_app` with `app.org_id=A`, rows of org B are invisible in every tenant table created in this phase; with no `app.org_id`, queries return zero rows (FR-048)
- [X] T019 [P] Write failing ledger tests in `crates/db/tests/ledger_test.rs`: `UPDATE`/`DELETE` on `balance_ledger` denied for `leavo_app`; `v_ledger_balances` allotted/carried/taken/adjusted/balance sums per leave year; comp-off type balance ignores `leave_year` (FR-040, FR-032). Pending/available are tested with requests in T093
- [X] T020 [P] Write failing job runner tests in `crates/worker/tests/runner_test.rs`: two runners never process the same job (`SKIP LOCKED`), exponential backoff, status `dead` after 5 attempts, `dedupe_key` uniqueness, jobs enqueued in a rolled-back tx never run (FR-047)
- [X] T021 [P] Write failing email tests in `crates/email/tests/sender_test.rs`: `MemoryEmailSender` captures messages; every template renders HTML and plain-text parts; SMTP sender builds from `SMTP_URL` (smtp, smtps, `?tls=required`)
- [X] T022 [P] Write failing CSRF test in `crates/api/tests/csrf_test.rs`: non-GET without `X-Requested-With: leavo` → `403 CSRF`; GET allowed
- [X] T023 [P] Write failing session extractor tests in `crates/api/tests/session_extractor_test.rs`: missing/expired/revoked cookie → 401; sliding extension inside last 24 h; never beyond `absolute_expires_at` (sign-in + 90 d); deactivated user → 401 (FR-005, FR-006)
- [X] T024 [P] Write failing log-redaction test in `crates/api/tests/log_redaction_test.rs`: fields named `code`, `token`, `reason`, `email` local part never appear in captured JSON logs (FR-065)

### Implementation (make the tests above green)

- [X] T025 [P] Implement `Clock`, `SystemClock`, `FixedClock` in `crates/core/src/clock.rs` (T014)
- [X] T026 [P] Implement `LeaveYear`, `WeekendMask`, `leave_year_of`, `today_in` in `crates/core/src/calendar.rs` (T012)
- [X] T027 [P] Implement `round_half`, `prorate(quota, doj, leave_year_start)` in `crates/core/src/balance.rs` (T013)
- [X] T028 [P] Implement `Role`, `Action`, `policy::can` in `crates/core/src/permissions.rs` (T015)
- [X] T029 [P] Implement `ErrorCode` enum with every code in data-model.md §Error code catalogue in `crates/core/src/error.rs` and `ApiError: IntoResponse` in `crates/api/src/error.rs` (T016)
- [X] T030 Implement config in `crates/api/src/config.rs` per contracts/configuration.md (T017)
- [X] T031 Create migration `migrations/0001_extensions_roles.sql`: `citext`, role `leavo_app` (no owner rights), default privileges
- [X] T032 Create migration `migrations/0002_tenancy.sql`: `organizations` (incl. `weekend_mask`, `reminder_after_hours`), `org_domains` (global unique `domain`), `users` (no gender fields; `last_notify_list`, `anonymised_at`), `approver_groups`, `approver_group_members`, `instance_settings` singleton; RLS on tenant tables
- [X] T033 Create migration `migrations/0003_auth.sql`: `login_codes` (purposes incl. `recovery`), `sessions` (incl. `absolute_expires_at`), `rate_limits`, `action_tokens`
- [X] T034 Create migration `migrations/0004_policy.sql`: `leave_types` (incl. `negative_limit`, `max_backdate_days`, `attachment_required_above_days`, `availability`, `pending_yearly_quota`, one `is_comp_off` per org), `leave_type_grants` (partial unique active), `holidays`; RLS
- [X] T035 Create migration `migrations/0005_ledger_audit_jobs.sql`: `balance_ledger` (reason enum per data-model.md, `INSERT, SELECT` only for `leavo_app`), view `v_ledger_balances` (ledger-only sums, comp-off year-independent; `v_balances` with pending comes in T099), `rollover_runs`, `audit_log`, `jobs`; RLS
- [X] T036 Create integration test harness `crates/api/tests/common/mod.rs` + `crates/api/tests/common/factories.rs`: start Postgres container once per test binary, run migrations as owner, connect app as `leavo_app`, build app with `FixedClock`/`MemoryEmailSender`/`MemoryStorage`, helpers `create_org`, `create_user(role, group, doj)`, `login_as(user) -> Client` (cookie + CSRF header), `run_jobs()`
- [X] T037 Implement org-scoped transactions in `crates/db/src/tx.rs`: `begin_org_tx(pool, org_id)` with `SET LOCAL app.org_id`, `lock_balance(tx, user, type)` via `pg_advisory_xact_lock(hashtextextended(...))`, `lock_org(tx, org)` (T018)
- [X] T038 Implement migration runner in `crates/db/src/migrate.rs`: session advisory lock, embedded `sqlx::migrate!`, each migration in its own transaction
- [X] T039 Implement ledger repository in `crates/db/src/ledger.rs`: `insert_entry`, `balances(user, year)`, `balance_by_year(user, type, years)` over `v_ledger_balances` (T019); extended to `v_balances` pending/available and the future-year projection in T103
- [X] T040 [P] Implement audit helper `write_audit(tx, actor, action, entity, before, after, ip)` in `crates/db/src/audit.rs`
- [X] T041 [P] Implement `EmailSender` trait, `SmtpSender`, `MemoryEmailSender` in `crates/email/src/lib.rs` and template engine with base layout in `crates/email/src/render.rs`, `crates/email/templates/base.html.j2`, `base.txt.j2` (T021)
- [X] T042 Implement job queue in `crates/db/src/jobs.rs` (`enqueue(tx, kind, payload, run_at, dedupe_key)`) and runner in `crates/worker/src/runner.rs` (claim with `FOR UPDATE SKIP LOCKED`, backoff, dead-letter) (T020)
- [X] T043 Implement `send_email` job in `crates/worker/src/jobs/send_email.rs` (render + send + retry)
- [X] T044 [P] Implement outbox helper `notify(tx, template, to, ctx, attachments)` in `crates/db/src/outbox.rs` that enqueues `send_email`
- [X] T045 Implement scheduler skeleton in `crates/worker/src/scheduler.rs`: 1-minute tick, registry of periodic jobs keyed `kind:org:period`, records `last_tick_at` (catch-up behaviour completed in T209)
- [X] T046 Implement Axum app in `crates/api/src/app.rs`: `/api/v1` router, request-id, JSON tracing with redaction layer in `crates/api/src/telemetry.rs` (T024), `/healthz`, `/readyz`, and a `Deprecation` response-header layer driven by an operation attribute in `crates/api/src/middleware/deprecation.rs` (FR-050)
- [X] T047 Implement CSRF middleware in `crates/api/src/middleware/csrf.rs` (T022)
- [X] T048 Implement session extractor `CurrentUser { id, org_id, role }` in `crates/api/src/auth/extractor.rs` and session repo in `crates/db/src/sessions.rs` (T023)
- [X] T049 Implement OpenAPI aggregation in `crates/api/src/openapi.rs` and CLI command `leavo openapi` writing `specs/001-leave-management-v1/contracts/openapi.yaml`; add CI steps failing on drift and on any operation or response field removed compared with the latest release's `openapi.yaml` (`scripts/ci/openapi_compat.py`, FR-050)
- [X] T050 Implement SPA embedding with fallback to `index.html` in `crates/api/src/spa.rs`
- [X] T051 Implement CLI in `crates/api/src/main.rs` with `clap`: `serve` (migrate → serve → optional in-process worker), `worker`, `migrate`, `openapi`, `--version`; stubs for `setup-token`, `recovery-login`
- [X] T052 [P] Implement web API client in `web/src/api/client.ts` (base `/api/v1`, `X-Requested-With: leavo`, `Idempotency-Key` helper, error-code → i18n message mapping) and `npm run gen:api` producing `web/src/api/schema.d.ts`
- [X] T053 [P] Implement web shell in `web/src/routes/__root.tsx`, `web/src/components/Layout.tsx` (left nav by role per PRD §2), auth guard redirecting to `/login`, neutral colour token in `web/src/styles/theme.css`

**Checkpoint**: `cargo test --workspace` and `npm test` green; `leavo serve` starts, migrates, serves `/healthz` and the empty SPA.

---

## Phase 3: User Story 1 — Sign in with an email code (P1) 🎯 MVP

**Goal**: Passwordless signup/login with 6-digit codes, sessions, rate limits, sign-out everywhere.
**Independent test**: With an org + whitelisted domain created by factory, a new person signs up with a code, signs out, signs in again; outsider domain refused; lockout after 5 wrong codes.

### Tests (red first)

- [X] T054 [P] [US1] Write failing tests for OTP primitives in `crates/core/tests/otp_test.rs`: always 6 digits incl. leading zeros, HMAC over `lower(email):code`, constant-time verify
- [X] T055 [P] [US1] Write failing tests for `POST /auth/code` in `crates/api/tests/auth_code_test.rs`: login → identical 202 body for active, unknown and deactivated emails (email sent only to active); signup with unregistered domain → `422 DOMAIN_NOT_REGISTERED`; 1/30 s and 5/h per email and 20/h per IP → `429 RATE_LIMITED` + `Retry-After`; new code consumes the previous one; signup for an email that already has an account returns the same 202 and sends a login code instead (FR-001–FR-004)
- [X] T056 [P] [US1] Write failing tests for `POST /auth/verify` in `crates/api/tests/auth_verify_test.rs`: success sets `leavo_sid` (HttpOnly, Secure, SameSite=Lax), 7 d vs 30 d with `keep_signed_in`; expired after 10 min (FixedClock); single use; 5 wrong → `429 LOCKED` for 15 min; signup creates `employee` in the domain's org (optional `date_of_joining` in the signup payload is stored) with pro-rated allotment ledger rows for every `availability=all` type (FR-002, FR-005, FR-013)
- [X] T057 [P] [US1] Write failing tests for `POST /auth/logout` in `crates/api/tests/logout_test.rs`: current session revoked; `?all=true` revokes all of the user's sessions
- [X] T058 [P] [US1] Write failing web tests in `web/src/routes/login.test.tsx` and `web/src/routes/signup.test.tsx`: "code sent" screen identical for any email, resend disabled for 30 s, error message for `DOMAIN_NOT_REGISTERED`, lock message

### Implementation

- [X] T059 [US1] Implement code generation/HMAC/verify in `crates/core/src/otp.rs` (T054)
- [X] T060 [US1] Implement unscoped auth repository in `crates/db/src/auth_repo.rs` (login codes, attempts/lock, domain lookup) with a doc comment explaining why it bypasses tenant scope
- [X] T061 [US1] Implement Postgres-backed rate limiter with trusted-proxy IP extraction in `crates/api/src/auth/rate_limit.rs`
- [X] T062 [US1] Implement user creation + pro-rated allotments on signup in `crates/db/src/users_repo.rs` (uses `prorate`, `ledger::insert_entry` reason `prorate`/`allotment`)
- [X] T063 [US1] Implement routes `/auth/code`, `/auth/verify`, `/auth/logout` in `crates/api/src/routes/auth.rs` (T055–T057)
- [X] T064 [P] [US1] Create templates `crates/email/templates/login_code.html.j2` / `.txt.j2` with subject `{{code}} is your Leavo code`
- [X] T065 [US1] Implement pages `web/src/routes/login.tsx`, `web/src/routes/signup.tsx`, shared `web/src/components/CodeEntry.tsx`, and "Sign out of all devices" in `web/src/routes/profile.tsx` (T058)
- [X] T066 [US1] Write and pass E2E `web/e2e/auth.spec.ts`: signup via Mailpit code, logout, login, wrong code ×5 lock

**Checkpoint**: US1 independently demonstrable.

---

## Phase 4: User Story 2 — Set up an organization and its leave policy (P1)

**Goal**: First-run wizard, single/multi mode, domains, leave types incl. restricted grants, quota changes, holiday pack import.
**Independent test**: Fresh DB → `/setup` with token → admin signed in → configure types → new employee gets correct balances; ML visible only to grantee.

### Tests (red first)

- [X] T067 [P] [US2] Write failing tests in `crates/api/tests/setup_test.rs`: `/setup/status` true on empty DB; wrong token ×5 → `429 LOCKED`; `/setup/test-email` returns SMTP error text; `/setup/complete` creates org, admin, domain, seeded types (CL 12, SL 6 attachment>2 max_backdate_days 7, CCL/ML/PL restricted, CO), selected pack; token cleared afterwards and reuse → `403 SETUP_TOKEN_INVALID`; single mode `/orgs` → 404 (FR-007, FR-008)
- [X] T068 [P] [US2] Write failing tests in `crates/api/tests/orgs_multi_test.rs` for `POST /orgs` in `LEAVO_MODE=multi` with verified ticket; `DOMAIN_TAKEN`, `PUBLIC_DOMAIN`
- [X] T069 [P] [US2] Write failing tests in `crates/core/tests/domains_test.rs`: normalise (trim, lower, strip `@`), `INVALID_DOMAIN`, public-provider blocklist (FR-009)
- [X] T070 [P] [US2] Write failing tests in `crates/api/tests/domains_test.rs` for `/admin/org/domains` add/remove; removed domain blocks new signups but existing users keep access
- [X] T071 [P] [US2] Write failing tests in `crates/core/tests/quota_change_test.rs` for `(new − old) × remaining_months / 12` rounded to 0.5 using the 15th rule (FR-056)
- [X] T072 [P] [US2] Write failing tests in `crates/api/tests/leave_types_test.rs`: create/patch all fields (negative_limit, max_backdate_days, attachment threshold, availability, active); patch quota without `apply_from` → `QUOTA_APPLY_FROM_REQUIRED`; `now` writes `quota_change` rows for every affected user; `next_year` sets `pending_yearly_quota` only (FR-011, FR-056)
- [X] T073 [P] [US2] Write failing tests in `crates/api/tests/grants_test.rs`: grant on restricted type writes full-year `allotment`; type appears in grantee's `/me/balances` only; non-grantee gets `TYPE_NOT_AVAILABLE` on preview; revoke writes `lapse −(balance − pending)` and keeps history (FR-012)
- [X] T074 [P] [US2] Write failing tests in `crates/api/tests/org_settings_test.rs`: patch time zone/weekend; `leave_year_start_month` change after any request/ledger beyond initial allotments → `409 LEAVE_YEAR_LOCKED` (FR-010, FR-062)
- [X] T075 [P] [US2] Write failing web tests `web/src/routes/setup/setup.test.tsx` (4-step wizard validation) and `web/src/routes/admin/policy.test.tsx` (quota change dialog forces now/next-year choice)

### Implementation

- [X] T076 [P] [US2] Implement domain normalisation + blocklist in `crates/core/src/domains.rs` (T069)
- [X] T077 [P] [US2] Implement `quota_change` in `crates/core/src/policy.rs` (T071)
- [X] T078 [US2] Implement setup token lifecycle in `crates/api/src/setup/token.rs` (generate on first `serve`, log once, hash in `instance_settings`, 5 tries/15 min, clear on completion) and CLI `leavo setup-token` in `crates/api/src/main.rs`
- [X] T079 [P] [US2] Add India national holiday packs `holidays/IN/2026.csv`, `holidays/IN/2027.csv` (`date,name,kind`) and embed them via `crates/api/src/holiday_packs.rs`; CI validator `scripts/validate_holiday_packs.py` wired into `.github/workflows/ci.yml` (FR-034)
- [X] T080 [US2] Implement default leave type seeding in `crates/db/src/seed.rs`
- [X] T081 [US2] Implement routes `/setup/*` in `crates/api/src/routes/setup.rs` (T067)
- [X] T082 [US2] Implement `POST /orgs` (multi mode only) in `crates/api/src/routes/orgs.rs` (T068)
- [X] T083 [US2] Implement `/admin/org` and `/admin/org/domains` in `crates/api/src/routes/admin_org.rs` (T070, T074)
- [X] T084 [US2] Implement `/admin/leave-types` and grants in `crates/api/src/routes/admin_leave_types.rs` with repo `crates/db/src/leave_types_repo.rs` (T072, T073)
- [X] T085 [US2] Implement `GET /me`, `PATCH /me`, `GET /me/balances` in `crates/api/src/routes/me.rs` (only types available to the user; restricted only with active grant)
- [X] T086 [US2] Implement setup wizard pages `web/src/routes/setup/index.tsx` (token), `org.tsx`, `admin.tsx`, `email-test.tsx`, `defaults.tsx`, and `web/src/routes/register.tsx` (multi mode) (T075)
- [X] T087 [US2] Implement admin pages `web/src/routes/admin/org.tsx` (settings + domains) and `web/src/routes/admin/policy.tsx` (types table, edit drawer, quota-change dialog, grants list) and `web/src/components/SetupChecklist.tsx`
- [X] T088 [US2] Write and pass E2E `web/e2e/setup.spec.ts` (quickstart steps 1–2, 10)

**Checkpoint**: US1 + US2 give a working, signed-in org with policy.

---

## Phase 5: User Story 3 — Request leave and notify colleagues (P1)

**Goal**: Day counting, validation, per-year charging, preview, create with notify + attachments, idempotency, outbox emails.
**Independent test**: Request spanning weekend + mandatory holiday shows correct days, balance shows pending, approvers/admin/notified emailed; cross-year request split.

### Tests (red first)

- [X] T089 [P] [US3] Write failing property tests in `crates/core/tests/working_days_test.rs`: never counts weekend or mandatory holiday; optional holiday counted; half day only at range edges; single-day half = 0.5; half on non-working day dropped; each `BookedDay` carries correct `leave_year` (FR-016, FR-055)
- [X] T090 [P] [US3] Write failing tests in `crates/core/tests/leave_validation_test.rs`, one case per code: `INVALID_RANGE`, `ZERO_WORKING_DAYS`, `HALF_DAY_NOT_ALLOWED`, `INSUFFICIENT_BALANCE` (per leave year incl. future-year projection), `NEGATIVE_LIMIT_EXCEEDED`, `OVERLAP` (full vs half same date), `BACKDATE_NOT_ALLOWED` (start earlier than today − type's `max_backdate_days`; type with 0 cannot start in the past), `NOTICE_TOO_SHORT` (only for starts ≥ today; back-dated requests exempt; a renamed Sick Leave keeps its behaviour and a custom type with `max_backdate_days=3` can back-date — rules never depend on type code), `MAX_DAYS_EXCEEDED`, `ATTACHMENT_REQUIRED`, `TYPE_NOT_AVAILABLE`, `TOO_MANY_NOTIFY`, `REASON_REQUIRED`, `REASON_TOO_LONG` (FR-017, FR-055, FR-057)
- [X] T091 [P] [US3] Write failing tests in `crates/core/tests/routing_test.rs`: group approvers ∪ admins − requester; no group → all approvers ∪ admins; empty → `NO_APPROVER_AVAILABLE` (FR-022)
- [X] T092 [P] [US3] Write failing tests in `crates/api/tests/leaves_preview_test.rs`: `excluded` lists weekends and mandatory holidays with names; 29 Dec–2 Jan returns two `by_leave_year` entries; errors array uses catalogue codes; a restricted type without an active grant → `TYPE_NOT_AVAILABLE` (moved here from T073: preview arrives in US3)
- [X] T093 [P] [US3] Write failing tests in `crates/api/tests/leaves_create_test.rs`: 201 + pending in `/me/balances`; `v_balances` pending/available per leave year incl. future-year projection = quota − pending (FR-055); jobs enqueued: confirmation (employee), action (eligible approvers + admin), FYI (notified) where FYI body has name/dates/days/status only (no type, reason, attachment, action link) (FR-059); `routed_group_id` stored; same `Idempotency-Key` → same response, different body → `409 IDEMPOTENCY_MISMATCH` (FR-019)
- [X] T094 [P] [US3] Write failing concurrency tests in `crates/api/tests/leaves_concurrency_test.rs`: 10 parallel requests competing for the last day → exactly one succeeds; two overlapping requests in parallel → one (FR-019)
- [X] T095 [P] [US3] Write failing tests in `crates/api/tests/attachments_test.rs`: PDF/PNG/JPEG/WebP/HEIC accepted by content sniffing; 5 MB + 1 byte → `413 FILE_TOO_LARGE`; `.exe` renamed `.pdf` → `415`; signed URL valid 5 min and only for owner/eligible approver/admin
- [X] T096 [P] [US3] Write failing S3 integration test in `crates/storage/tests/s3_compat_test.rs` using a testcontainers SeaweedFS S3 gateway with signature checking (MinIO no longer publishes images; see research.md implementation notes) — upload, sniffing, 5 MB cap and 5-minute signed URL behave as with local storage (`tests/storage_test.rs`) (FR-053)
- [X] T097 [P] [US3] Write failing tests in `crates/api/tests/users_search_test.rs`: q < 2 chars → empty; same org, active users only; max 10
- [X] T098 [P] [US3] Write failing web test `web/src/routes/leave/apply.test.tsx`: live day count + excluded dates text, type dropdown shows "(N available)", notify picker accepts colleague or valid external email, blocks 21st member, attachment required hint

### Implementation

- [X] T099 [US3] Create migration `migrations/0006_requests.sql`: `leave_requests` (status enum incl. `cancel_requested`, `routed_group_id`, `version`), `leave_request_days` (with `leave_year`, partial unique index), `attachments`, `request_notifications`, `idempotency_keys`, and view `v_balances` = `v_ledger_balances` + pending days by leave year + `available` + future-year projection (data-model.md); RLS. Then switch the US2 stand-ins to it: `/me/balances` `pending`/`available` (routes/me.rs), the grant-revocation lapse `−(balance − pending)` (routes/admin_leave_types.rs `revoke`), and add "no leave requests" to the `LEAVE_YEAR_LOCKED` check (routes/admin_org.rs)
- [X] T100 [US3] Implement `working_days` returning `BookedDay { day, portion, half, leave_year }` in `crates/core/src/calendar.rs` (T089)
- [X] T101 [US3] Implement `validate_leave_request` in `crates/core/src/policy.rs` (T090)
- [X] T102 [US3] Implement `route_approvers` in `crates/core/src/approval.rs` (T091)
- [X] T103 [US3] Implement leaves repository in `crates/db/src/leaves_repo.rs`: load context (holidays, balances by year, active days), insert request + days + notifications under `lock_balance`, `pending_by_year`
- [X] T104 [P] [US3] Implement storage in `crates/storage/src/lib.rs` (`LocalFileSystem` / S3 via `object_store`, sniffing with `infer`, streaming 5 MB cap, signed URLs) and `MemoryStorage` for tests
- [X] T105 [US3] Implement idempotency middleware in `crates/api/src/middleware/idempotency.rs` using `idempotency_keys` (24 h)
- [X] T106 [US3] Implement routes `/leaves/preview`, `POST /leaves`, `GET /leaves/{id}` in `crates/api/src/routes/leaves.rs`; `/attachments` in `crates/api/src/routes/attachments.rs`; `/users/search` in `crates/api/src/routes/users.rs` (T092–T097)
- [X] T107 [P] [US3] Create templates `crates/email/templates/leave_requested_confirm`, `leave_requested_action` (Approve/Reject buttons placeholder), `leave_requested_fyi` (minimal content) as `.html.j2` + `.txt.j2`
- [X] T108 [US3] Implement apply page `web/src/routes/leave/apply.tsx` with `web/src/components/DateRangePicker.tsx`, `NotifyPicker.tsx` (last-used suggestions), `AttachmentUpload.tsx`, live preview via `/leaves/preview`, `Idempotency-Key` per form submission (T098)
- [X] T109 [US3] Write and pass E2E `web/e2e/apply-leave.spec.ts` (quickstart step 5)

**Checkpoint**: Employees can request leave; days pending; emails queued.

---

## Phase 6: User Story 4 — Approve or reject requests (P1)

**Goal**: Approvals inbox, single-level decision with optimistic locking, ledger deduction per leave year, email action links, decision emails with `.ics`, 48 h reminders.
**Independent test**: Two approvers receive a request; one approves; the other sees "Approved by X"; balance and emails update.

### Tests (red first)

- [X] T110 [P] [US4] Write failing state-machine table test in `crates/core/tests/approval_state_test.rs` for every allowed/forbidden transition of `leave_requests.status` (data-model.md state diagram)
- [X] T111 [P] [US4] Write failing tests in `crates/api/tests/approvals_inbox_test.rs`: only eligible approvers/admins see an item, oldest first, includes requester balance and `team_away` with type/reason only for eligible viewers; after the requester moves to another group, the original group's approvers still see and can decide the pending request and the new group's approvers cannot (FR-022, FR-058)
- [X] T112 [P] [US4] Write failing tests in `crates/api/tests/approvals_decide_test.rs`: approve writes `leave_taken` per leave year; reject without comment → `COMMENT_REQUIRED`, with comment releases pending; requester → `403 SELF_APPROVAL`; non-eligible → `403 NOT_ELIGIBLE_APPROVER`; stale `version` → `409 ALREADY_DECIDED` with `decided_by`; decision emails to employee + notified (FYI minimal) with `.ics` attachment on approval; pending request whose restricted grant was revoked can still be decided; when one eligible approver is deactivated, the remaining approvers and admin can still decide (FR-023, FR-046, Edge Cases)
- [X] T113 [P] [US4] Write failing concurrency test in `crates/api/tests/approvals_concurrency_test.rs`: two approvers decide simultaneously → one 200, one 409
- [X] T114 [P] [US4] Write failing tests in `crates/api/tests/action_token_test.rs`: link valid 72 h, single use, redirect pre-fills decision, deciding still requires session (FR-024)
- [X] T115 [P] [US4] Write failing tests in `crates/worker/tests/approval_reminder_test.rs`: pending > 48 h reminded, not again within 24 h, none after decision (FR-025)
- [X] T116 [P] [US4] Write failing web test `web/src/routes/approvals.test.tsx`: reject requires comment; 409 shows "Approved by X on D"

### Implementation

- [X] T117 [US4] Implement status transitions in `crates/core/src/approval.rs` (T110)
- [X] T118 [US4] Implement decide in `crates/db/src/approvals_repo.rs`: optimistic `UPDATE … WHERE status='pending' AND version=$v`, ledger rows per `leave_year` group, audit, outbox
- [X] T119 [US4] Implement action tokens in `crates/api/src/auth/action_token.rs` (HMAC, nonce in `action_tokens`, 72 h)
- [X] T120 [US4] Implement routes `/approvals`, `/approvals/{kind}/{id}/decide`, `/approvals/action/{token}` in `crates/api/src/routes/approvals.rs` (T111–T114)
- [X] T121 [P] [US4] Implement `.ics` builder in `crates/email/src/ics.rs` and templates `leave_decided`, `leave_decided_fyi`, `approval_reminder`; wire real action links into `leave_requested_action`
- [X] T122 [US4] Implement job `crates/worker/src/jobs/approval_reminder.rs` and register hourly in scheduler (T115)
- [X] T123 [US4] Implement inbox `web/src/routes/approvals.tsx` (+ `?decision=` prefill landing), `web/src/components/DecisionDialog.tsx` (T116)
- [X] T124 [US4] Write and pass E2E `web/e2e/approve.spec.ts` (quickstart step 6)

**Checkpoint**: P1 complete — **internal MVP** (sign in, set up, request, approve).

---

## Phase 7: User Story 5 — Balances, history and who is away (P2)

**Goal**: Dashboard cards, privacy-safe who's away with team filter, team calendar, history + CSV.
**Independent test**: Seeded data → dashboard numbers equal ledger; employee sees colleagues' dates without type/reason.

### Tests (red first)

- [X] T125 [P] [US5] Write failing tests in `crates/core/tests/visibility_test.rs`: `can_see_details(viewer, subject)` true for admin, for approvers of subject's group, for every approver when subject has no group, false otherwise; request-level checks use the request's `routed_group_id`, person-level checks use the current group (FR-058)
- [X] T126 [P] [US5] Write failing tests in `crates/api/tests/away_test.rs`: today/week/month in org time zone; approved only, sorted by `from`; `scope=team` = same approver group OR department; `leave_type`/`reason` keys **absent** for non-eligible viewers; `count_today`; `/team/calendar?month=` uses the same projection (FR-027, FR-058)
- [X] T127 [P] [US5] Write failing tests in `crates/api/tests/history_test.rs`: filters year/type/status, pagination, CSV columns exactly as contracts/csv-formats.md §7 and same fields as JSON (FR-028)
- [X] T128 [P] [US5] Write failing reconciliation test in `crates/api/tests/balances_reconcile_test.rs`: for random scenarios, every `/me/balances` number equals the sum of the user's ledger rows (SC-010)
- [X] T129 [P] [US5] Write failing web tests `web/src/routes/dashboard.test.tsx`: neutral colour for colleagues, "12 allotted + 3 carried" line, "4 people away today" badge, My team toggle

### Implementation

- [X] T130 [US5] Implement `crates/core/src/visibility.rs` (T125)
- [X] T131 [US5] Implement `/away` and `/team/calendar` in `crates/api/src/routes/away.rs` with repo `crates/db/src/away_repo.rs` (T126)
- [X] T132 [US5] Implement `/me/leaves` and `/me/leaves.csv` in `crates/api/src/routes/me.rs` (T127)
- [X] T133 [US5] Implement dashboard `web/src/routes/dashboard.tsx` with `web/src/components/BalanceCard.tsx`, `WhosAway.tsx`, `MyRequests.tsx`, `UpcomingHolidays.tsx` (T129)
- [X] T134 [P] [US5] Implement history page `web/src/routes/leave/history.tsx` (filters, CSV download)
- [X] T135 [P] [US5] Implement team calendar `web/src/routes/team.tsx`
- [X] T136 [US5] Write and pass E2E `web/e2e/privacy.spec.ts` (quickstart step 7)

---

## Phase 8: User Story 6 — Cancel leave (P2)

**Goal**: Withdraw pending, cancel approved before start, cancel-after-start via approver with partial return.
**Independent test**: Cancel approved future leave → balance restored and everyone emailed.

### Tests (red first)

- [X] T137 [P] [US6] Write failing tests in `crates/api/tests/cancel_test.rs`: pending withdraw releases days; approved before start → `leave_reversed` for all days; started → `cancel_requested`, appears in `/approvals?kind=cancel`; accept returns only days dated ≥ request date and deactivates only those day rows; decline keeps `approved`; emails to employee, approvers and notified (FR-020)
- [X] T138 [P] [US6] Write failing web test `web/src/components/MyRequests.test.tsx`: Cancel label/confirm text differs for pending, future, started

### Implementation

- [X] T139 [US6] Add cancel transitions to `crates/core/src/approval.rs` and partial-return calculation `days_to_return(days, requested_on)` (T137)
- [X] T140 [US6] Implement cancel/accept/decline in `crates/db/src/leaves_repo.rs` and `crates/db/src/approvals_repo.rs` (ledger, day rows, audit, outbox)
- [X] T141 [US6] Implement `POST /leaves/{id}/cancel` in `crates/api/src/routes/leaves.rs` and `kind=cancel` handling in `crates/api/src/routes/approvals.rs`
- [X] T142 [P] [US6] Create templates `leave_cancelled` and `cancel_requested` in `crates/email/templates/`
- [X] T143 [US6] Implement cancel UI in `web/src/components/MyRequests.tsx` and `web/src/components/CancelDialog.tsx` (T138)

---

## Phase 9: User Story 7 — Earn and spend comp-offs (P2)

**Goal**: Comp-off earn requests, credits with expiry, FIFO spending with reservations, year independence, admin revoke.
**Independent test**: Comp-off for last Saturday approved → CO balance 1 → spend → expiry behaviour.

### Tests (red first)

- [X] T144 [P] [US7] Write failing tests in `crates/core/tests/comp_off_test.rs`: valid dates = past weekend or mandatory holiday; optional holiday/working day/future → `COMP_OFF_DATE_INVALID`; FIFO by `expires_on`; expirable = days − used − reserved (FR-031, FR-032, FR-060)
- [X] T145 [P] [US7] Write failing tests in `crates/api/tests/comp_offs_test.rs`: request multiple dates with half days; duplicate claim rejected; withdraw pending; approval creates credits + `comp_off_credit` ledger; balance spans leave years (Dec credit usable in Jan) (FR-030, FR-032)
- [X] T146 [P] [US7] Write failing tests in `crates/api/tests/comp_off_spend_test.rs`: comp-off leave reserves credits on submit, consumes FIFO on approval (`comp_off_usages`), restores on reject/cancel
- [X] T147 [P] [US7] Write failing tests in `crates/worker/tests/comp_off_expiry_test.rs`: expires unused unreserved days with `comp_off_expired`; reserved days survive until decision, then expire immediately if past `expires_on`; 7-day warning sent once (FR-032, FR-060)
- [X] T148 [P] [US7] Write failing tests in `crates/api/tests/comp_off_revoke_test.rs`: unused credit revoked with reason → `comp_off_revoked`; used or reserved → `422 CREDIT_IN_USE`; missing reason → `NOTE_REQUIRED` (FR-061)

### Implementation

- [X] T149 [US7] Create migration `migrations/0007_comp_off.sql`: `comp_off_requests`, `comp_off_request_dates` (unique active claim), `comp_off_credits` (`days_reserved`, revoke fields), `comp_off_usages`; RLS
- [X] T150 [US7] Implement `crates/core/src/comp_off.rs` (date validation, FIFO allocation, expirable days) (T144)
- [X] T151 [US7] Implement `crates/db/src/comp_off_repo.rs` (requests, credits, reservations, usages, revoke)
- [X] T152 [US7] Integrate comp-off type into leave submit/approve/cancel paths in `crates/db/src/leaves_repo.rs` and `crates/db/src/approvals_repo.rs` (T146)
- [X] T153 [US7] Implement routes `/comp-offs`, `/comp-offs/{id}/cancel`, `/admin/comp-off-credits/{id}/revoke` in `crates/api/src/routes/comp_offs.rs`; add `kind=comp_off` to approvals (T145, T148)
- [X] T154 [US7] Implement job `crates/worker/src/jobs/comp_off_expiry.rs` (daily 00:15 org time) (T147)
- [X] T155 [P] [US7] Create templates `comp_off_requested_action`, `comp_off_decided`, `comp_off_expiring` in `crates/email/templates/`
- [X] T156 [US7] Implement page `web/src/routes/comp-offs.tsx` (balance, next expiry, credits table, request form) and comp-off card in `web/src/components/BalanceCard.tsx`
- [X] T157 [US7] Write and pass E2E `web/e2e/comp-off.spec.ts` (quickstart step 9)

---

## Phase 10: User Story 8 — Holiday calendar (P2)

**Goal**: Holiday list/calendar for current and next year; admin CRUD/import/copy/pack; side effects on booked leave.
**Note**: the holiday-pack import used by the wizard is built in US2 (T079), and the next-year holiday banner (FR-036, US8-AS4) is built with the admin dashboard in US10 (T177/T185).
**Independent test**: Import India pack; request across a mandatory holiday excludes it; adding/removing holidays adjusts balances with ledger entries.

### Tests (red first)

- [X] T158 [P] [US8] Write failing tests in `crates/api/tests/holidays_test.rs`: `/holidays?year=` for current and next year; admin create/patch/delete; CSV import dry run and apply; pack import; copy last year (FR-033, FR-034)
- [X] T159 [P] [US8] Write failing tests in `crates/api/tests/holiday_side_effects_test.rs`: add mandatory inside approved → `leave_reversed` + day row inactive; inside pending → days recounted, no ledger; delete or change to optional inside approved → new active day + `leave_taken` even if balance goes negative + emails to employee and admins (FR-035)
- [X] T160 [P] [US8] Write failing web test `web/src/routes/holidays.test.tsx`: year tabs, past dimmed, "Diwali in 12 days" countdown with FixedClock

### Implementation

- [X] T161 [US8] Implement holiday CSV parsing in `crates/importers/src/holidays.rs`
- [X] T162 [US8] Implement `crates/db/src/holidays_repo.rs` with side-effect service `apply_holiday_change(tx, before, after)` (T159)
- [X] T163 [US8] Implement `/holidays`, `/holiday-packs`, `/admin/holidays`, `/admin/holidays/import` in `crates/api/src/routes/holidays.rs` (T158)
- [X] T164 [P] [US8] Create template `holiday_changed_leave` in `crates/email/templates/`
- [X] T165 [US8] Implement `web/src/routes/holidays.tsx` (list + month view) and `web/src/routes/admin/holidays.tsx` (CRUD, import, copy, pack) (T160)

---

## Phase 11: User Story 9 — Year-end carry forward (P2)

**Goal**: Idempotent rollover with cap, lapse, negative carry, restricted grants, pending quotas; preview + CSV.
**Independent test**: Cap 5, 8 unused CL → 12 + 5 next year, 3 lapsed; second run changes nothing.

### Tests (red first)

- [X] T166 [P] [US9] Write failing tests in `crates/core/tests/rollover_test.rs`: CL carry = min(unused, cap), rest lapses; other types lapse; negative balance → `lapse +|d|` old year and `negative_carry −|d|` new year; comp-off untouched; restricted types allotted only to active grantees; `pending_yearly_quota` applied (FR-037, FR-057, FR-056)
- [X] T167 [P] [US9] Write failing tests in `crates/worker/tests/year_end_rollover_test.rs`: runs on first day of org leave year in org time zone; second run no-op via `rollover_runs`; employee balance emails + admin summary CSV (FR-038)
- [X] T168 [P] [US9] Write failing tests in `crates/api/tests/carry_forward_preview_test.rs`: preview equals what the job writes; `.csv` columns per csv-formats §7 (FR-039)

### Implementation

- [X] T169 [US9] Implement `plan_rollover(balances, policy) -> Vec<LedgerEntry>` in `crates/core/src/rollover.rs` (T166)
- [X] T170 [US9] Implement job `crates/worker/src/jobs/year_end_rollover.rs` (daily 00:30 org time, org advisory lock, `rollover_runs`) (T167)
- [X] T171 [US9] Implement `/admin/carry-forward/preview` (+ `.csv`) in `crates/api/src/routes/admin_leave_types.rs` (T168)
- [X] T172 [P] [US9] Create templates `rollover_employee`, `rollover_admin_summary` in `crates/email/templates/`
- [X] T173 [US9] Implement preview UI in `web/src/routes/admin/carry-forward.tsx`

---

## Phase 12: User Story 10 — Manage people, approvers and balances (P2)

**Goal**: Users, roles, approver groups, deactivation, adjustments, dashboard, reports, audit log, data export/anonymise, sign-out-all.
**Independent test**: Invite user, make approver in a group, adjust balance with reason, see all in audit log.

### Tests (red first)

- [X] T174 [P] [US10] Write failing tests in `crates/api/tests/admin_users_test.rs`: invite single (invited status, invite email); CSV invite dry run/apply; role change; group assignment; demoting/deactivating last active admin → `LAST_ADMIN`; deactivate revokes sessions immediately; admin can change `date_of_joining` without re-allotment; employee `PATCH /me` cannot (FR-014, FR-006, FR-042, FR-064)
- [X] T175 [P] [US10] Write failing tests in `crates/api/tests/approver_groups_test.rs`: CRUD, members must be approvers/admins, employee in ≤ 1 group (FR-021)
- [X] T176 [P] [US10] Write failing tests in `crates/api/tests/adjustments_test.rs`: missing note → `NOTE_REQUIRED`; `manual_adjustment` row with creator; balances + ledger view (FR-041)
- [X] T177 [P] [US10] Write failing tests in `crates/api/tests/admin_dashboard_test.rs`: away today, pending, pending > 48 h, upcoming holidays, `next_year_holidays_missing` true from 1 Nov when next year empty (FR-036, FR-044)
- [X] T178 [P] [US10] Write failing tests in `crates/api/tests/reports_test.rs`: days taken by type/user/month for a leave year; CSV (FR-044)
- [X] T179 [P] [US10] Write failing tests in `crates/api/tests/audit_log_test.rs`: every request, decision, balance change, policy change, role change, sign-out-all and recovery login creates an entry with before/after; filters work (FR-044)
- [X] T180 [P] [US10] Write failing tests in `crates/api/tests/personal_data_test.rs`: `/me/export` returns own profile, history, ledger; admin export of any user; anonymise only deactivated users, rewrites name/email in users and audit JSON, deletes reasons and attachment objects, keeps dates/days/ledger totals (FR-045)
- [X] T181 [P] [US10] Write failing test in `crates/api/tests/sign_out_all_test.rs`: every session in the org revoked incl. caller; other orgs unaffected (FR-063)

### Implementation

- [X] T182 [US10] Implement `/admin/users`, `/admin/users/import`, `/admin/users/{id}` in `crates/api/src/routes/admin_users.rs` with `crates/db/src/users_repo.rs` additions (T174)
- [X] T183 [US10] Implement `/admin/approver-groups` in `crates/api/src/routes/admin_groups.rs` (T175)
- [X] T184 [US10] Implement `/admin/users/{id}/balances` and `/adjustments` in `crates/api/src/routes/admin_balances.rs` (T176)
- [X] T185 [US10] Implement `/admin/dashboard` in `crates/api/src/routes/admin_dashboard.rs` (T177)
- [X] T186 [US10] Implement `/admin/reports/leaves` (+ `.csv`) in `crates/api/src/routes/admin_reports.rs` (T178)
- [X] T187 [US10] Implement `/admin/audit-log` in `crates/api/src/routes/admin_audit.rs` (T179)
- [X] T188 [US10] Implement export/anonymise service in `crates/db/src/personal_data.rs` and routes `/me/export`, `/admin/users/{id}/export`, `/admin/users/{id}/anonymise` (T180)
- [X] T189 [US10] Implement `/admin/org/sign-out-all` in `crates/api/src/routes/admin_org.rs` (T181)
- [X] T190 [P] [US10] Create template `invite` in `crates/email/templates/`
- [X] T191 [US10] Implement admin pages `web/src/routes/admin/index.tsx` (dashboard + holiday banner), `admin/employees.tsx`, `admin/employee.$id.tsx` (profile, balances, ledger, adjust), `admin/approvers.tsx`, `admin/reports.tsx`, `admin/audit.tsx`, and "Download my data" in `web/src/routes/profile.tsx`

---

## Phase 13: User Story 11 — Migrate from Keka (P2)

**Goal**: Dry-run and all-or-nothing import of employees, opening balances and informational history.
**Independent test**: Sample Keka CSV → dry run → fix errors → apply → balances match the file.

### Tests (red first)

- [X] T192 [P] [US11] Write failing parser tests in `crates/importers/tests/csv_formats_test.rs` with fixtures in `crates/importers/tests/fixtures/`: each error code (`UNKNOWN_DOMAIN`, `UNKNOWN_LEAVE_TYPE`, `INVALID_DATE`, `INVALID_DAYS`, `DUPLICATE_ROW`, `MISSING_COLUMN`, `OVERLAP`) and warning code (`USER_EXISTS`, `GROUP_WILL_BE_CREATED`, `BALANCE_REPLACES_EARLIER_IMPORT`, `HISTORY_OUTSIDE_LEAVE_YEAR`)
- [X] T193 [P] [US11] Write failing tests in `crates/importers/tests/keka_test.rs`: raw Keka columns mapped via configurable leave-type map to canonical rows
- [X] T194 [P] [US11] Write failing tests in `crates/api/tests/import_test.rs`: dry run writes nothing; apply with any error → `422 IMPORT_HAS_ERRORS` and nothing written; apply creates invited users, `import` ledger rows, compensating row on re-import; history rows become approved requests with day rows, block overlaps, and write no `leave_taken` (FR-043)
- [X] T195 [P] [US11] Write failing tests in `scripts/tests/test_keka_import.py` (pytest) for `scripts/keka_import.py` mapping and domain checks

### Implementation

- [X] T196 [US11] Implement canonical CSV parsing + validation in `crates/importers/src/csv.rs` (T192)
- [X] T197 [US11] Implement Keka mapping in `crates/importers/src/keka.rs` (T193)
- [X] T198 [US11] Implement apply service in `crates/db/src/import_repo.rs` (single transaction per batch of 500 within one outer all-or-nothing check) (T194)
- [X] T199 [US11] Implement `/admin/import/{keka|csv}` in `crates/api/src/routes/admin_import.rs`
- [X] T200 [US11] Implement `scripts/keka_import.py` (T195)
- [X] T201 [US11] Implement import UI `web/src/routes/admin/import.tsx` (upload, leave-type mapping, errors/warnings table, apply disabled while errors exist)

---

## Phase 14: User Story 12 — Self-host, upgrade and operate (P3)

**Goal**: Safe migrations and catch-up, recovery login, instance page, metrics, packaging, docs, demo.
**Independent test**: Clean server → quickstart B → wizard in < 30 min; upgrade keeps balances.

### Tests (red first)

- [X] T202 [P] [US12] Write failing test in `crates/db/tests/migrate_failure_test.rs`: a deliberately broken fixture migration rolls back fully, runner returns error naming the version, earlier migrations stay applied, `/readyz` fails, server does not bind (FR-069)
- [X] T203 [P] [US12] Write failing test in `crates/worker/tests/catch_up_test.rs`: with FixedClock jump of 3 days, scheduler enqueues missed reminder, expiry and rollover periods exactly once; restarting again adds nothing; no duplicate emails (FR-070)
- [X] T204 [P] [US12] Write failing test in `crates/api/tests/recovery_login_test.rs`: `leavo recovery-login` for active admin prints link valid 15 min, single use, audit entry `auth.recovery_login`; refuses non-admin/deactivated (FR-067)
- [X] T205 [P] [US12] Write failing tests in `crates/api/tests/instance_test.rs`: `/admin/instance` shows version, update-check flag, email status + last error, storage kind, migrations applied/pending, dead jobs; `/admin/instance/test-email`; in `LEAVO_MODE=multi` the response omits `storage`/`migrations` and email/job figures cover only the caller's org (FR-051)
- [X] T206 [P] [US12] Write failing tests in `crates/api/tests/telemetry_test.rs`: with `LEAVO_UPDATE_CHECK=false` no outbound HTTP; `/metrics` requires bearer when `LEAVO_METRICS_TOKEN` set and exposes the metrics listed in configuration.md (FR-052, FR-065)
- [X] T207 [P] [US12] Write failing test in `crates/api/tests/secret_rotation_test.rs`: restarting with a new `LEAVO_SECRET_KEY` invalidates sessions, unused codes and action links (FR-063)

### Implementation

- [X] T208 [US12] Complete migration-failure behaviour in `crates/db/src/migrate.rs` and start-up ordering in `crates/api/src/main.rs` (T202)
- [X] T209 [US12] Implement catch-up back-fill in `crates/worker/src/scheduler.rs` (T203)
- [X] T210 [US12] Implement `leavo recovery-login <email>` in `crates/api/src/main.rs` + `crates/api/src/auth/recovery.rs` (T204)
- [X] T211 [US12] Implement `/admin/instance` (mode-aware field set, org-scoped email/job figures in multi mode) and test-email in `crates/api/src/routes/admin_instance.rs` and job `crates/worker/src/jobs/update_check.rs` (daily, GitHub releases, respects flag) (T205, T206)
- [X] T212 [US12] Implement `/metrics` with optional bearer in `crates/api/src/metrics.rs` and instrument request latency, codes sent/verified, email failures, job depth/age (T206)
- [X] T213 [P] [US12] Implement `web/src/routes/admin/instance.tsx`
- [X] T214 [P] [US12] Create multi-stage `Dockerfile` (node latest LTS build → Rust latest stable with cargo-chef → distroless cc runtime, non-root, `/data/uploads` volume)
- [X] T215 [P] [US12] Create `docker-compose.yml` (leavo pinned to major tag + `postgres:<latest major>`, volumes `uploads`, `pgdata`) per contracts/configuration.md
- [X] T216 [US12] Create `.github/workflows/release.yml`: release-please, buildx multi-arch (amd64, arm64), push to GHCR `:X.Y.Z`, `:X.Y`, `:X`, cosign signing, Trivy scan, attach compose + `.env.example` to release
- [X] T217 [US12] Add CI job `upgrade-test` in `.github/workflows/ci.yml` with script `scripts/ci/upgrade_test.sh`: run previous release image, load sample data, upgrade to new image, assert `/admin/users/{id}/balances` unchanged (SC-014)
- [X] T218 [US12] Add CI job `install-smoke` with `web/e2e/install-smoke.spec.ts`: start shipped `docker-compose.yml`, read setup token from logs, complete wizard automatically; fail if `docker compose up` → signed-in admin takes more than 5 minutes of machine time (budget for SC-012)
- [X] T219 [P] [US12] Write mdBook docs in `docs/book/src/`: `quickstart.md`, `install/vps.md` (Compose + Caddy), `install/aws.md` (ECS Fargate + RDS + SES), `install/gcp.md` (Cloud Run min 1 instance + Cloud SQL), `install/azure.md`, `install/digitalocean.md`, `operate/upgrade.md` (backup first, no downgrades), `operate/backup-restore.md`, `operate/secret-rotation.md`, `operate/recovery-login.md`, `operate/monitoring.md` (alert thresholds); publish via `.github/workflows/docs.yml` to GitHub Pages (FR-065, FR-067, FR-068)
- [X] T220 [P] [US12] Create demo instance assets: seed generator `crates/api/src/bin/demo_seed.rs` and `deploy/demo/` (compose override + nightly reset cron/workflow `.github/workflows/demo-reset.yml`) (FR-066)
- [X] T221 [US12] Write and pass E2E `web/e2e/upgrade-and-instance.spec.ts` (quickstart steps 12–13)

---

## Phase 15: Polish & Cross-Cutting Concerns

- [X] T222 [P] Write `README.md` (what, screenshot, 3-command quick start, links), `CONTRIBUTING.md` (dev setup, tests, TDD expectation, DCO sign-off), `CODE_OF_CONDUCT.md` (Contributor Covenant), `SECURITY.md` (private advisories, supported versions)
- [X] T223 [P] Add `.github/ISSUE_TEMPLATE/{bug.yml,feature.yml,holiday-pack.yml}` and `.github/PULL_REQUEST_TEMPLATE.md` (checklist incl. cross-org test row and tests-first)
- [X] T224 [P] Write and pass accessibility suite `web/e2e/a11y.spec.ts` (axe on every route, keyboard-only flows for apply/approve, 360 px viewport) and fix findings in `web/src/` (SC-016)
- [X] T225 [P] Write performance check `scripts/perf/dashboard_load.js` (k6 latest): seed 1,000 employees, assert dashboard API p95 < 1.5 s; second profile on 1 vCPU/1 GB container with 300 employees / 30 VUs (SC-003, SC-013)
- [X] T226 Write and pass cross-org access matrix `crates/api/tests/cross_org_matrix_test.rs`: for every route in the OpenAPI document, a user of org A requesting org B's resource IDs gets 404 and no data (FR-048)
- [X] T227 [P] Write and pass full-journey E2E `web/e2e/quickstart.spec.ts` mirroring quickstart.md section C steps 1–11, asserting the apply-leave form can be completed and submitted within 30 s of opening (SC-001) and an email approval takes ≤ 2 actions after sign-in (SC-002)
- [X] T228 Regenerate `contracts/openapi.yaml` and reconcile any differences with `contracts/http-api.md`; update `research.md` with any latest-version API deviations recorded during setup
- [X] T229 [P] Add i18n completeness check to CI and confirm every UI string comes from `locales/en.json` (FR-054)
- [X] T230 Run `cargo audit`, `npm audit`, Trivy on the built image; fix or document findings in `SECURITY.md`
- [ ] T231 Final review against `specs/001-leave-management-v1/checklists/*.md` and spec success criteria; including a manual timed install by someone new to the project following docs/book quickstart (SC-012 < 30 min); record results in `specs/001-leave-management-v1/checklists/release.md` — *Review done (see release.md); the timed install by someone new is still pending.*

---

## Dependencies & Execution Order

### Phase dependencies

```text
Phase 1 Setup ──▶ Phase 2 Foundational ──▶ US1 ─┐
                                         └▶ US2 ─┴▶ US3 ──▶ US4 ──┬▶ US5
                                                                  ├▶ US6
                                                                  ├▶ US7 (needs US6 cancel paths for usage reversal)
                                                                  ├▶ US8
                                                                  ├▶ US9
                                                                  ├▶ US10
                                                                  └▶ US11
                     US12 tests/infra can start after Phase 2; T217/T218/T221 need US1–US4.
                     Phase 15 after all desired stories.
```

### Story dependencies

| Story | Depends on | Why |
|---|---|---|
| US1 Sign in | Phase 2 | sessions, users, ledger for allotments (orgs created by factory) |
| US2 Setup & policy | Phase 2 (US1 for UI login only) | API tests use factories; can run in parallel with US1 |
| US3 Request leave | US2 (leave types, holidays tables, me/balances) | |
| US4 Approvals | US3 | needs requests |
| US5 Visibility | US4 | approved leave to show |
| US6 Cancel | US4 | approved state |
| US7 Comp-offs | US4, US6 | approval flow; cancel reverses usages |
| US8 Holidays | US3 (side effects on booked days), US4 for approved cases | pack import itself is in US2 |
| US9 Year end | US4 | taken/pending figures |
| US10 Admin | US2, US4 | roles/groups, audit of decisions |
| US11 Import | US2, US3 | users, types, day rows |
| US12 Operate | Phase 2 (most), US1–US4 for E2E/upgrade tests | |

### Within each story

1. Test tasks (all `[P]`) → run → **red**.
2. Migrations → core (pure) → db repos → routes → templates → web → E2E.
3. Story checkpoint: all its tests green, lint clean.

---

## Parallel Execution Examples

**Phase 2 tests** — all of T012–T024 in parallel (different files).

**Phase 2 implementation** — T025, T026, T027, T028, T029 in parallel (separate core modules); then T031–T035 sequential (migration order); T040, T041, T044 in parallel.

**US1** — T054, T055, T056, T057, T058 together; then T059 ∥ T064, then T060 → T061 → T062 → T063 → T065 → T066.

**US3** — T089–T098 together; T104 (storage) ∥ T107 (templates) alongside T099 → T100 → T101 → T102 → T103 → T105 → T106.

**After US4** — with three developers: Dev A US5 → US6, Dev B US7 (after US6 T140) / US8, Dev C US10 → US11; US9 by whoever finishes first.

**US12** — T202–T207 together; T213, T214, T215, T219, T220 in parallel.

---

## Implementation Strategy

### MVP first (internal pilot)

1. Phase 1 + Phase 2 (foundation, all green).
2. US1 → US2 → US3 → US4 (P1). **Stop and validate** with quickstart C steps 1–6. This is a usable leave request/approval system for Aktivolabs' engineering pilot.

### Incremental delivery

3. US5 + US6 (dashboard, cancellation) → pilot feedback.
4. US7, US8, US9 (comp-offs, holidays, year end) → full leave lifecycle.
5. US10 + US11 (admin, Keka import) → company-wide launch and parallel run with Keka.
6. US12 + Phase 15 → public open-source release.

### TDD rules for every task

- Never write implementation for behaviour that has no failing test.
- Keep tests at the lowest level that proves the rule: pure rules in `crates/core` tests; SQL/locking/RLS in `crates/db` or `crates/api` integration tests; UI behaviour in vitest; journeys in Playwright.
- A task referencing tests (e.g. "(T090)") is complete only when those tests pass.

### Totals

- 231 tasks: Setup 11 · Foundational 42 · US1 13 · US2 22 · US3 21 · US4 15 · US5 12 · US6 7 · US7 14 · US8 8 · US9 8 · US10 18 · US11 10 · US12 20 · Polish 10
