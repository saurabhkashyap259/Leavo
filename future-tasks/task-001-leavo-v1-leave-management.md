# Task 001: Leavo v1 — Open-Source Leave Management System

**Status**: Planned  
**Priority**: HIGH (New Product / Cost Reduction)  
**Effort**: ~800 hours (2 engineers × 10 weeks: 8 weeks core build + 2 weeks self-hosting packaging)  
**Date Identified**: 2026-11-24  
**Related Tasks**: None yet (sub-tasks for each phase can be split out as Task 002+)

---

## Problem Statement

Aktivolabs is cancelling its Keka subscription to cut costs. The team mainly used Keka's **leave module**, so paying per employee for a full HRMS is no longer justified. Without a replacement the company loses:

- One place for employees to see leave balances and history
- A formal request → approval flow with an audit trail of who approved what
- Visibility of who is on leave today / this week / this month
- Comp-off tracking for weekend and holiday work
- Year-end carry forward of unused casual leave
- A shared holiday calendar

Falling back to email and spreadsheets would cause balance disputes, missed approvals and no audit trail.

The same gap exists at many small companies: they pay per-seat for an HR suite only to track leave, or use spreadsheets. Existing open-source options are either heavy HR suites or unmaintained leave-only tools. Releasing Leavo as open source gives them a modern, self-hostable leave tracker and gives Aktivolabs outside contributors and testing.

**Business objectives** (from BRD §3):

| # | Objective | Target |
|---|-----------|--------|
| O1 | Remove HRMS subscription cost | Hosting cost only |
| O2 | Move everyone off Keka | 100% onboarded within 2 weeks of launch |
| O3 | Accurate migrated balances | 0 unresolved disputes after 30 days |
| O4 | Fast approvals | Median request → decision < 24 h |
| O5 | Password-free access | 0 accounts with a password |
| O6 | Easy to self-host | `git clone` → working instance < 30 min |
| O7 | Cheap to run | 1 vCPU, 1 GB RAM + PostgreSQL |
| O8 | Adoption | 10 external production deployments in 12 months |
| O9 | Healthy project | 15 outside contributors with a merged PR in 12 months |

---

## Current State

### Existing Implementation

- **No code exists yet.** The repository contains only the Spec Kit scaffolding (`.specify/`, `.claude/skills/`) and the requirement documents in [docs/](../docs/).
- Leave is currently managed in **Keka** (SaaS), which is being cancelled. Contract end date is still to be confirmed (BRD open question).
- Keka data (employees, balances, current-year leave history) must be exported **before** the subscription ends.

**Limitations**:
- Hard deadline tied to the Keka contract end date (constraint C2)
- Leave policy numbers (quotas, carry-forward cap, comp-off expiry) not yet confirmed by HR (dependency D2)
- Product name "Leavo" clashes with an existing commercial product — must be renamed before any public repo, domain or Docker image is created
- Licence (AGPL-3.0 vs MIT) not yet decided

---

## Proposed Solution

### High-Level Approach

Build a self-hosted web app that covers the **leave lifecycle end to end and nothing beyond leave**, shipped as **one Docker image** (Rust binary serving the API, the embedded React app and the job worker) with **PostgreSQL as the only required dependency**.

Key design decisions (ERD):

| Area | Decision | Why |
|------|----------|-----|
| API | Rust — Axum + Tokio, SQLx (compile-time checked SQL) | Team already uses Rust; small footprint, cheap hosting |
| Database | PostgreSQL 15+ | Only required service; transactions, RLS, JSONB for audit |
| Web | React + TypeScript (Vite), TanStack Query, Tailwind; embedded via `rust-embed` | One image, no separate web server or CORS |
| Jobs | Same binary; Postgres `jobs` table with `FOR UPDATE SKIP LOCKED` (outbox pattern) | No Redis; emails never sent for rolled-back changes |
| Email | Any SMTP via `lettre`, behind an `EmailSender` trait | Works with whatever the company already has |
| Storage | Local disk (default) or any S3-compatible bucket | Small installs need no bucket |
| Auth | 6-digit email OTP → HttpOnly session cookie. No passwords, no JWTs in the browser | BR-04 / O5 |
| Balances | Append-only `balance_ledger`; balance = `SUM(delta)` | Every change is explainable and auditable |
| Tenancy | `org_id` on every table + Postgres Row-Level Security | A missed filter returns nothing instead of leaking data |
| Modes | Single-org (default) or multi-org via `LEAVO_MODE` | Self-hosters vs. hosting for several companies |

### Technical Design

**Repository layout**

```
leavo/
  crates/
    api/      # Axum routes, handlers, middleware
    core/     # domain: leave engine, policies, ledger (pure, no I/O)
    db/       # SQLx queries
    worker/   # scheduled + queued jobs
    email/    # templates + EmailSender trait
  web/        # React app
  scripts/    # keka_import.py (developer tool)
  migrations/ # SQLx migrations
  holidays/   # country holiday packs, e.g. IN/2027.csv
  locales/    # en.json first
```

**Leave engine (pure function in `crates/core`)**

```rust
pub fn working_days(from: NaiveDate, to: NaiveDate,
                    start_half: Option<Half>, end_half: Option<Half>,
                    weekend: &[Weekday], holidays: &HashSet<NaiveDate>) -> Vec<BookedDay> {
    from.iter_days().take_while(|d| *d <= to)
        .filter(|d| !weekend.contains(&d.weekday()) && !holidays.contains(d))
        .map(|d| {
            let half = if d == from { start_half } else if d == to { end_half } else { None };
            BookedDay { day: d, portion: if half.is_some() { 0.5 } else { 1.0 }, half }
        })
        .collect()
}
```

**No double-spending** — submit / approve / cancel each run in one transaction:
1. `pg_advisory_xact_lock(hashtext(user_id || leave_type_id))` serialises one employee's balance of one type.
2. Recompute `available = ledger sum − pending days`; reject if too low (unless negative allowed).
3. Insert request + `leave_request_days`; unique index `(user_id, day, coalesce(half,'full')) WHERE active` is the overlap backstop.
4. Approvals use optimistic locking on `version`; 0 rows updated → `409` with who already decided.

**Ledger events**: `allotment`, `prorate`, `carry_forward`, `lapse`, `leave_taken`, `leave_reversed`, `comp_off_credit`, `comp_off_expired`, `manual_adjustment`, `import`.

**Trade-offs**:
- **Pros**: one container + Postgres keeps hosting cheap and install simple; pure-function engine is easy to test; ledger makes every balance explainable; RLS gives defence in depth for multi-tenancy.
- **Cons**: in-process jobs stop if a serverless platform scales to zero (documented: keep ≥1 instance or run `leavo worker` separately); Rust raises the bar slightly for outside contributors; email delivery is critical-path for login.

---

## Implementation Steps

Weeks follow the BRD rollout plan and ERD effort estimate (2 engineers).

### Phase 0: Pre-build decisions (Weeks 1–2, overlaps design)
- [ ] Confirm Keka monthly cost and contract end date
- [ ] HR confirms quotas per leave type, carry-forward cap, comp-off expiry (90 days?), half-day in v1
- [ ] Pick and verify a new product name (Leavo is taken)
- [ ] Decide licence: AGPL-3.0 (recommended) or MIT
- [ ] Resolve PRD open questions (leave-type visibility to colleagues, optional holidays, two-level approvals, SSO in v1, languages)
- [ ] Export all Keka data early (employees, balances, current-year history)
- [ ] UI design for all screens in PRD §2

### Phase 1: Foundations (1.5 weeks)
- [ ] Cargo workspace + `web/` Vite app, `compose.dev.yml` (Postgres + MailHog)
- [ ] CI: clippy `-D warnings`, `cargo test`, eslint, vitest
- [ ] SQLx migrations for core schema (organizations, org_domains, users, leave_types, leave_requests, leave_request_days, request_notifications, approvals, comp_off_requests/credits, balance_ledger, holidays, login_codes, sessions, audit_log, jobs, approver_groups) and `v_balances` view
- [ ] Row-Level Security policies + `SET LOCAL app.org_id` per transaction
- [ ] Startup migration runner behind an advisory lock
- [ ] Email OTP auth: `POST /auth/code`, `POST /auth/verify`, `POST /auth/logout` (HMAC-stored codes, constant-time compare, 10-min expiry, single use, 5-try / 15-min lock, rate limits, identical response for unknown emails)
- [ ] Sessions: 32-byte token, SHA-256 stored, `leavo_sid` cookie (HttpOnly, Secure, SameSite=Lax), 7 / 30-day, sliding, sign-out-everywhere
- [ ] CSRF check (`X-Requested-With`), `policy::can(user, action, resource)` RBAC in `core`

### Phase 2: Org setup, policy, users, holidays (1 week)
- [ ] First-run setup wizard (`/setup`) with one-time setup token from server log; test email step; holiday pack + default leave types
- [ ] Multi-org registration (`/register`, `POST /orgs`) when `LEAVO_MODE=multi`
- [ ] Domain whitelist: lower-case, no `@`, block public domains, unique across orgs
- [ ] Leave policy (`/admin/policy`): seeded types (CL, SL, CCL, ML, PL, CO), quota, colour, half-day, negative, max days, notice, eligibility, active; mid-year quota change prompt
- [ ] Employee signup with domain match, pro-rated allotment (rounded to 0.5); admin single + CSV invite
- [ ] User admin: roles, approver groups, deactivate (history kept, login blocked), "≥1 active admin" rule
- [ ] Holidays: CRUD, CSV import, copy last year, India holiday pack, configurable weekend days

### Phase 3: Leave engine, requests, approvals, ledger (2 weeks)
- [ ] `working_days` + validation in `core` (zero-day, overlap, balance, notice, max days, sick leave back-dating 7 days)
- [ ] `POST /leaves/preview` and `POST /leaves` with notify members (colleague search + external emails, max 20)
- [ ] Attachments (PDF/image ≤ 5 MB, type sniffed, private, 5-min signed URLs)
- [ ] Cancel: pending anytime; approved until start date; after start needs approver
- [ ] Approvals inbox, approve/reject (comment required to reject), first decision final, never self-approve, optimistic locking
- [ ] Signed single-use email action links (72 h) that pre-fill but still require a session
- [ ] Ledger writes for every balance event; manual adjustment with required note; audit log

### Phase 4: Dashboard, history, who's away, comp-offs (1.5 weeks)
- [ ] Dashboard: balance cards (available, allotted, carried, taken, pending, progress ring), comp-off card with next expiry
- [ ] Who's away (Today / Week / Month), type hidden for non-approvers, count badge
- [ ] My requests, upcoming holidays widget
- [ ] Leave history page with filters + CSV export
- [ ] Team calendar (`/team`)
- [ ] Comp-offs: request for past weekend/holiday dates only, full/half per date, approval via same flow, FIFO spending, credits table

### Phase 5: Jobs, emails, carry forward, Keka import (1 week)
- [ ] Worker: `send_email` (retry ×5 with backoff), `approval_reminder` (48 h, once/24 h), `comp_off_expiry` (+7-day warning), `year_end_rollover` (idempotent per org/year), `cleanup`
- [ ] Email templates (HTML + plain text): code, invite, requested (approver + FYI), decision, cancellation, reminder, comp-off expiry, rollover summary; `.ics` attachment on approval
- [ ] Carry-forward preview + download (`/admin/carry-forward/preview`)
- [ ] Holiday-inside-approved-leave refund to ledger
- [ ] Keka importer: dry run diff → apply (users as invited, `import` ledger rows, history as approved requests); generic CSV template
- [ ] Admin console extras: org dashboard, reports, audit log viewer, Admin → Instance page

### Phase 6: Hardening, testing, pilot (1 week)
- [ ] Concurrency tests, cross-org access tests for every endpoint, Playwright E2E with MailHog
- [ ] Observability: JSON logs (no codes/tokens/reasons), `/metrics`, alerts, optional Sentry
- [ ] Pilot with engineering team; fix issues

### Phase 7: Open-source packaging (2 weeks)
- [ ] Multi-arch image (amd64, arm64) to GHCR on tagged release; `docker-compose.yml` + `.env.example`
- [ ] release-please, conventional commits, CHANGELOG with "Upgrading" section, signed images
- [ ] CI gates: migration upgrade test, install smoke test (compose + wizard), cargo/npm audit, Trivy scan
- [ ] LICENSE, README, CONTRIBUTING (DCO), CODE_OF_CONDUCT, SECURITY, issue/PR templates, Renovate
- [ ] Docs site + install guides: VPS, AWS (ECS/RDS/SES), GCP (Cloud Run/Cloud SQL), Azure, DigitalOcean; backup guide
- [ ] Public demo instance with nightly reset
- [ ] Update-check against GitHub releases (opt-out via `LEAVO_UPDATE_CHECK`)

### Phase 8: Rollout (Weeks 9–10 and after)
- [ ] Company-wide launch at Aktivolabs, 2-week parallel run with Keka
- [ ] Employees verify balances; disputes fixed via manual adjustments
- [ ] Switch off Keka
- [ ] Make repository public with docs, image and demo

---

## Success Criteria

### Performance Targets
- [ ] Dashboard p95 load < 1.5 s for orgs up to 1,000 employees
- [ ] Login codes delivered < 30 s
- [ ] 99.5% monthly uptime
- [ ] Runs on 1 vCPU / 1 GB RAM + PostgreSQL
- [ ] Employee can submit a leave request in < 30 s; approver decides in one click

### Business Validation
- [ ] 100% of Aktivolabs employees onboarded within 2 weeks of launch
- [ ] 0 unresolved balance disputes 30 days after migration
- [ ] Median request → decision < 24 h
- [ ] Fresh install via docs reaches a working instance in < 30 min

### Technical Validation
- [ ] All PRD acceptance criteria (§2a, 3–10) pass
- [ ] All BRD business rules BRL-01 … BRL-19 enforced and tested
- [ ] Cross-org access attempts fail for every endpoint
- [ ] No double-spend under concurrent requests / concurrent approvals
- [ ] Upgrade from previous release leaves balances unchanged
- [ ] No telemetry leaves an install unless enabled

### Code Quality
- [ ] `cargo clippy -D warnings` and eslint clean
- [ ] Permissions table and leave engine fully unit-tested (incl. proptest for date ranges)
- [ ] WCAG 2.1 AA contrast and keyboard use; usable at 360 px width

---

## Files Modified

**New Files** (all new — greenfield project):
- `Cargo.toml`, `crates/{api,core,db,worker,email}/` — backend workspace
- `web/` — React + TypeScript app
- `migrations/` — SQLx migrations
- `scripts/keka_import.py` — Keka mapping/cleanup developer tool
- `holidays/IN/2026.csv`, `holidays/IN/2027.csv` — India holiday packs
- `locales/en.json` — interface strings
- `Dockerfile`, `docker-compose.yml`, `compose.dev.yml`, `.env.example`
- `.github/workflows/` — CI, release, image publish
- `LICENSE`, `README.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`
- `docs/` site source (mdBook or Docusaurus) — note: current `docs/` holds the BRD/PRD/ERD; decide whether to move them

**Configuration** (environment variables):
- Required: `DATABASE_URL`, `LEAVO_BASE_URL`, `LEAVO_SECRET_KEY`, `SMTP_URL`, `EMAIL_FROM`
- Optional: `LEAVO_MODE` (`single`|`multi`), `LEAVO_RUN_JOBS`, `STORAGE` (+ `S3_*`), `LEAVO_UPDATE_CHECK`, `SENTRY_DSN`

---

## Testing Requirements

### Unit Tests (`cargo test`, `proptest`)
```rust
#[test]
fn excludes_weekends_and_holidays() {
    // Setup: Mon 2026-12-21 .. Mon 2026-12-28, holiday 2026-12-25, Sat/Sun weekend
    // Action: working_days(...)
    // Assert: 5 booked days, 25/26/27 excluded
}
```
- Day counting incl. half days at range edges
- Pro-rating for mid-year joiners (rounded to 0.5)
- Carry forward: `min(unused, cap)` + lapse of remainder
- Permissions table: every (role, action, resource) case

### Integration Tests (`sqlx::test`, testcontainers)
- Every endpoint against real Postgres, including cross-org reads/writes
- OTP: expiry, single use, resend invalidates old code, lockout, rate limits, identical responses
- Year-end rollover idempotency (run twice → one set of ledger rows)
- Keka import dry run vs apply

### Concurrency Tests
- Two parallel requests spending the last available day → exactly one succeeds
- Two approvers deciding at once → one wins, other gets `409` with decider

### End-to-End (Playwright + MailHog)
- Signup with code → apply → approve → dashboard balance updates
- Install smoke test: shipped compose file → setup wizard completed automatically

---

## Expected Impact

### Quantitative Improvements
- **Leave tooling spend**: Keka per-employee subscription → fixed hosting cost only
- **Break-even**: build cost ÷ (monthly Keka cost − monthly Leavo cost) — fill in once Keka invoice is attached

### Qualitative Benefits
- ✅ Single source of truth for balances with a full, explainable ledger
- ✅ Passwordless, phishing-resistant-by-design login
- ✅ Company owns its data (DPDP Act 2023 tooling: export, anonymisation, audit log)
- ✅ Reusable by other small companies; outside contributors improve it

### Risks Mitigated
- Prevents balance disputes and lost approvals from an email/spreadsheet fallback
- Removes vendor lock-in for leave data

---

## Dependencies

**Required** (must complete first):
- ❌ HR confirmation of leave policy numbers and holiday list (D2)
- ❌ Keka data export (D3)
- ❌ SMTP provider with SPF / DKIM / DMARC on the sending domain (D1)
- ❌ New product name chosen and checked
- ❌ Licence decision

**Recommended**:
- Keka contract end date confirmed to lock the timeline
- Named product and on-call owner after launch

---

## Risk Mitigation

1. **Login emails land in spam or arrive late** (High)
   - **Mitigation**: dedicated sending domain with SPF/DKIM/DMARC, reputable provider, "resend code", alert on email failure rate > 5% / 15 min
2. **Migrated Keka balances are wrong** (High)
   - **Mitigation**: dry-run import with diff, side-by-side check, 2-week parallel run, manual adjustments with reason
3. **Keka ends before Leavo is ready** (High)
   - **Mitigation**: export all Keka data early; keep v1 scope tight
4. **Name clash with existing commercial "Leavo"** (High)
   - **Mitigation**: rename before creating public repo, domain or image
5. **Security flaw affects every deployment** (High)
   - **Mitigation**: SECURITY.md private reporting, cargo/npm audit + Trivy in CI, fast patch releases with upgrade notes
6. **Self-hosters lose data on upgrade** (High)
   - **Mitigation**: forward-only migrations, upgrade test in CI, backup guide, "Upgrading" section in every release
7. **OTP brute force / email enumeration** (Medium)
   - **Mitigation**: rate limits, lockouts, identical responses, short-lived single-use HMAC'd codes
8. **Policy edge cases (half days, holidays inside leave, mid-year joiners)** (Medium)
   - **Mitigation**: rules fixed in BRD §7, pure-function engine with property tests, ledger + audit log
9. **Aktivolabs-specific needs leak into product / maintainer burnout** (Medium)
   - **Mitigation**: company rules become settings; leave-only scope; issue templates and public roadmap

---

## Future Enhancements

Out of scope for v1.0:
- **v1.1**: optional (restricted) holidays, bulk approve, reports
- **v2**: Slack notifications, `.ics` calendar feed, separate policies by location/grade, mobile app
- Helm chart for Kubernetes
- SSO (Google / Microsoft) — open question whether to pull into v1
- Two-level approvals for long leaves
- Explicitly never: payroll / LOP, attendance, timesheets, leave encashment, other HRMS modules

---

## Notes

- Example Aktivolabs policy (pending HR): Casual 12 (receives carry forward), Sick 6, Child Care TBD, Maternity per law, Paternity TBD, Comp-off earned only.
- Privacy rule: colleagues see *that* someone is away and when, never the type or reason. Approvers (their group) and admins see everything.
- "Today" and the leave year are always computed in the org's time zone (`chrono-tz`), never server UTC.
- All IDs are UUID v7; dates are `date`, timestamps `timestamptz`.
- Audit records retained 7 years (default).

---

## References

**Documentation**:
- [Leavo — Business Requirements Document (BRD)](../docs/requirements/Leavo%20—%20Business%20Requirements%20Document%20(BRD).docx) — business need, scope, rules BRL-01…19
- [Leavo — Product Requirements Document (PRD)](../docs/requirements/Leavo%20—%20Product%20Requirements%20Document%20(PRD).docx) — screens, flows, acceptance criteria
- [Leavo — Engineering Requirements Document (ERD)](../docs/requirements/Leavo%20—%20Engineering%20Requirements%20Document%20(ERD).docx) — stack, schema, API, jobs, security, self-hosting
- [Leavo — open-source leave tracker plan](../docs/requirements/Leavo%20—%20open-source%20leave%20tracker%20plan.png) — plan overview

---

## Checklist

Before considering task complete:
- [ ] All implementation steps checked off
- [ ] All success criteria met
- [ ] Tests passing (unit, integration, concurrency, E2E, upgrade, install smoke)
- [ ] Docs site and install guides published
- [ ] Peer review completed
- [ ] Aktivolabs production instance live; Keka switched off
- [ ] Monitoring/alerts configured
- [ ] Repository made public with image and demo
