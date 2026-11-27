# Implementation Plan: Leavo v1 — Open-Source Leave Management System

**Branch**: `001-leave-management-v1` | **Date**: 2026-11-24 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/001-leave-management-v1/spec.md` (clarified 2026-11-24)

## Summary

Leavo is a self-hosted leave tracker: org setup with a domain whitelist, passwordless email-code login, leave policy with restricted types, leave and comp-off requests, single-level approvals, dashboards with privacy-aware "who's away", a holiday calendar, year-end carry forward, Keka import, and an admin console. It ships as **one Rust binary in one Docker image** that serves a JSON API, the embedded React app and the background worker; **PostgreSQL is the only required dependency**. Every balance change is a row in an append-only ledger, tenant isolation is enforced in the app and by Postgres Row-Level Security, and all rules live in a pure, property-tested domain crate. Decisions and alternatives are in [research.md](research.md).

## Technical Context

**Language/Version**: Rust stable ≥ 1.83 (edition 2021); TypeScript 5 / Node 22 LTS for the web build only  
**Primary Dependencies**: Axum 0.8, Tokio, SQLx 0.8, chrono + chrono-tz, lettre 0.11, minijinja, icalendar, object_store, utoipa, tracing, rust-embed; React 19, Vite 6, TanStack Query/Router, Tailwind 4, Radix UI, react-hook-form + zod, i18next  
**Storage**: PostgreSQL 15+ (RLS, advisory locks, `citext`); attachments on local disk or any S3-compatible bucket  
**Testing**: cargo test + proptest (core), sqlx::test + testcontainers (integration, cross-org, concurrency), vitest + Testing Library (web), Playwright + Mailpit + axe (E2E, a11y), image upgrade test and install smoke test in CI  
**Tooling**: latest stable releases of all dependencies at setup time (versions above are minimums; Renovate keeps them current); Mailpit for dev/E2E email; k6 for load checks; Python 3 (stdlib + pytest) for `scripts/` (Keka mapping, holiday-pack validation); mdBook for docs  
**Target Platform**: Linux container (amd64, arm64); evergreen Chrome/Safari/Edge/Firefox, ≥ 360 px wide  
**Project Type**: Web application — Rust API + worker + embedded SPA, single deployable  
**Performance Goals**: Dashboard p95 < 1.5 s for orgs up to 1,000 employees (SC-003); 95% of login codes delivered < 30 s (SC-004); request submit < 30 s end to end (SC-001)  
**Constraints**: Runs on 1 vCPU / 1 GB RAM + Postgres (SC-013); no Redis or other services; no outbound telemetry by default; forward-only migrations, no manual steps within a major version; WCAG 2.1 AA  
**Scale/Scope**: ≤ 1,000 employees per org (SC-003); 300 employees / 30 concurrent users on the 1 vCPU / 1 GB baseline (SC-013); many orgs in multi mode; 12 user stories, 70 FRs, ~20 screens, ~65 endpoints, ~25 tables

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Checked against the ratified [constitution](../../.specify/memory/constitution.md) v1.0.0 (2026-11-24). Principles I–IX correspond to the gates G1–G9 below; Principle X (test-first) is satisfied by the TDD ordering in [tasks.md](tasks.md).

| # | Gate (constitution principle, source) | Pre-research | Post-design |
|---|---|---|---|
| G1 | Postgres is the only required runtime dependency (ERD §1) | ✅ | ✅ jobs, rate limits, sessions all in Postgres (R4, R11) |
| G2 | One deployable image; config via env vars only (ERD §10) | ✅ | ✅ [configuration.md](contracts/configuration.md) |
| G3 | No passwords; codes hashed, single-use, rate-limited (BRD BR-04, BRL-18) | ✅ | ✅ R4, `login_codes`, `rate_limits` |
| G4 | Tenant data isolated in app **and** database (ERD §5, FR-048) | ✅ | ✅ R3, RLS on every tenant table |
| G5 | Balances are explainable: append-only ledger (BRD BR-12, SC-010) | ✅ | ✅ R6, `balance_ledger` INSERT/SELECT-only grant |
| G6 | Domain rules are pure and unit/property-tested (ERD §6) | ✅ | ✅ `leavo-core` has no I/O |
| G7 | Data minimisation / DPDP (BRD C3, clarification Q5) | ✅ | ✅ no gender/parent fields; export + anonymise endpoints; log redaction (R17) |
| G8 | Leave-only scope; company specifics are settings, not code (BRD §9) | ✅ | ✅ all Aktivolabs values are seeds/config; leave rules (back-dating, notice, negative limit, attachment threshold) are per-type fields, never keyed on type codes |
| G9 | Safe upgrades: forward-only migrations, upgrade test in CI (ERD §10–11) | ✅ | ✅ R2, R19 |
| G10 | Test-first: failing test before implementation (Principle X, user requirement) | ✅ | ✅ every phase in tasks.md orders tests first |

**Result**: PASS on all ten principles (G10 / Principle X: every task phase in tasks.md lists failing tests before implementation).

## Project Structure

### Documentation (this feature)

```text
specs/001-leave-management-v1/
├── spec.md
├── plan.md              # this file
├── research.md          # Phase 0
├── data-model.md        # Phase 1
├── quickstart.md        # Phase 1
├── contracts/
│   ├── http-api.md      # JSON API (OpenAPI generated at build into contracts/openapi.yaml)
│   ├── configuration.md # CLI commands + environment variables
│   └── csv-formats.md   # import/export + holiday pack formats
├── checklists/requirements.md
└── tasks.md             # Phase 2 (/speckit-tasks — not created here)
```

### Source Code (repository root)

```text
Cargo.toml                      # workspace
crates/
├── core/                       # leavo-core: pure domain, no I/O
│   └── src/
│       ├── calendar.rs         # working_days, weekend mask, leave year, tz "today"
│       ├── policy.rs           # leave type rules, validation codes
│       ├── balance.rs          # ledger math, pro-rating, carry forward, comp-off FIFO
│       ├── approval.rs         # routing, state machine
│       ├── permissions.rs      # policy::can(user, action, resource)
│       └── visibility.rs       # who's-away projections by role
├── db/                         # leavo-db: SQLx repositories, tx helpers (SET LOCAL app.org_id, advisory locks)
├── email/                      # leavo-email: EmailSender trait, SMTP + memory impls, templates/, ics
├── storage/                    # leavo-storage: object_store wrapper, sniffing, signed URLs
├── worker/                     # leavo-worker: job runner, scheduler, jobs/{send_email,reminder,comp_off_expiry,rollover,cleanup,update_check}
├── importers/                  # leavo-importers: Keka + generic CSV parsing, dry-run diff
└── api/                        # leavo-api (binary `leavo`): Axum routes, auth middleware, CSRF, rate limiting, OpenAPI, SPA embedding, CLI (serve/worker/migrate)
    ├── src/bin/demo_seed.rs    # demo instance data generator
    └── tests/                  # integration: per-endpoint, cross-org matrix, concurrency
# each crate also has its own tests/: core (unit + proptest), db (RLS, ledger, migrate), worker (jobs, scheduler), importers (CSV/Keka), email (templates)
migrations/                     # SQLx forward-only migrations (0001–0007)
web/
├── src/
│   ├── routes/                 # /login /signup /setup /dashboard /leave/* /comp-offs /holidays /team /approvals /admin/*
│   ├── components/             # balance cards, who's away, date range picker, notify picker, tables
│   ├── api/                    # generated OpenAPI types + TanStack Query hooks
│   └── i18n/
├── tests/                      # vitest
└── e2e/                        # Playwright (+ axe)
locales/en.json
holidays/IN/2026.csv, IN/2027.csv
scripts/
├── keka_import.py
├── validate_holiday_packs.py
├── tests/                      # pytest for the Python scripts
├── ci/upgrade_test.sh          # previous-release → new-image balance check
└── perf/dashboard_load.js      # k6 load profile (SC-003, SC-013)
deploy/demo/                    # demo instance compose override + nightly reset
docs/                           # mdBook site (BRD/PRD/ERD .docx move to docs/requirements/)
Dockerfile, docker-compose.yml, compose.dev.yml, .env.example
.github/workflows/{ci.yml,release.yml}, ISSUE_TEMPLATE/, PULL_REQUEST_TEMPLATE.md
README.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md, SECURITY.md, LICENSE (before public release)
```

**Structure Decision**: Single Cargo workspace with a pure `core` crate and thin I/O crates, plus a `web/` SPA embedded into the `api` binary. This keeps one deployable (G2) while letting the leave engine be tested without a database (G6). Separate `storage` and `importers` crates keep optional concerns out of the request path and make contributor scope clear.

## Delivery Phases (input for `/speckit-tasks`)

Mapped to spec priorities; each phase ends in a demonstrable increment.

| Phase | Scope | Stories / FRs | Est. (2 eng) |
|---|---|---|---|
| 0 Foundations | Workspace, CI, migrations framework, RLS + tx helpers, error model, OpenAPI, SPA embedding, dev compose | — | 1 wk |
| 1 Auth & setup (P1) | Codes, sessions (90-day cap), rate limits, CSRF, setup wizard + token rules, single/multi mode, domains, sign-out-all, recovery login | US1, US2 · FR-001–010, 014, 063, 067 | 1.5 wk |
| 2 Policy & people (P1) | Leave types (negative limit), restricted grants + revoke lapse, pro-rating, quota change, users, roles, approver groups, holidays + India 2026/2027 packs, org-setting change rules | US2, US8, US10 · FR-011–013, 021, 033–036, 042, 056, 062, 064 | 1 wk |
| 3 Engine, requests, approvals (P1) | Day counting (mandatory holidays only), per-leave-year charging, negative limits, back-dating rules, idempotency keys, ledger, preview/create/cancel (partial return after start), attachments, notify, approvals inbox, action links, outbox emails incl. minimal FYI content | US3, US4, US6 · FR-015–025, 040, 046–047, 055, 057, 059 | 2 wk |
| 4 Visibility & comp-offs (P2) | Dashboard, eligible-approver field rule on every surface, who's away + team filter, team calendar, history + CSV, comp-off earn/spend/expiry (reservation-aware, year-independent), credit revoke | US5, US7 · FR-026–032, 058, 060, 061 | 1.5 wk |
| 5 Jobs, carry forward, import, admin (P2) | Scheduler with catch-up, reminders, rollover (incl. negative carry) + preview, all-or-nothing Keka/CSV import, reports, audit log, adjustments, self + admin data export, anonymisation incl. audit, instance page | US9–US11 · FR-037–045, 051, 070 | 1 wk |
| 6 Hardening & pilot | Concurrency/cross-org suites, a11y, perf check at 1,000 employees and on the 1 vCPU / 1 GB baseline, logs/metrics/alerts, pilot fixes | SC-003–005, 013, 016 · FR-065 | 1 wk |
| 7 Self-hosting & OSS packaging (P3) | Multi-arch image, release-please, API deprecation policy, atomic migrations + failure behaviour, upgrade + install smoke tests, docs site, cloud guides, backup/restore guide, nightly-reset demo instance | US12 · FR-050–054, 066, 068, 069 | 2 wk |

Total ≈ 11 weeks; Phase 0 overlaps the BRD's design weeks, matching the ~10-week plan in the task doc.

## Key Risks Addressed by Design

- **Double spending / double booking** → advisory lock per (user, type) + partial unique index + optimistic decision versioning (R6).
- **Lost or phantom emails** → transactional outbox; retries; dead jobs visible on instance page (R11).
- **Cross-tenant leaks** → RLS + automated cross-org test matrix over every endpoint (R3, R18).
- **Privacy of absences** → server-side role projections; employees never receive type/reason fields (R10).
- **Year-end mistakes** → pure carry-forward function shared by preview and job; `rollover_runs` idempotency (R12).

## Complexity Tracking

No constitution violations to justify. Two deliberate choices recorded for reviewers:

| Choice | Why needed | Simpler alternative rejected because |
|---|---|---|
| Postgres RLS in addition to app-level `org_id` filters | Defence in depth for a multi-tenant OSS product run by strangers | App-only filtering leaks data on a single missed `WHERE` |
| Seven crates instead of one | Pure `core` must compile and test without DB; optional storage/import code isolated | A single crate makes the engine depend on SQLx and slows contributor tests |
