# Release Readiness: Leavo v1

**Purpose**: Final review (T231) of the build against the requirement checklists and the spec's success criteria, before the first public release.
**Created**: 2026-11-27
**Feature**: [spec.md](../spec.md) · [tasks.md](../tasks.md)

## Requirement checklists

| Checklist | Items | Done |
|---|---|---|
| [requirements.md](requirements.md) | 16 | 16 |
| [leave-rules.md](leave-rules.md) | 14 | 14 |
| [security.md](security.md) | 12 | 12 |
| [api.md](api.md) | 9 | 9 |
| [ops.md](ops.md) | 11 | 11 |

## Automated suites (2026-11-27)

- [x] Rust workspace: 370 tests pass, `cargo clippy -D warnings` clean, `cargo fmt` clean
- [x] Web unit tests: 102 pass; type check, lint and Prettier clean
- [x] i18n completeness (`npm run check:i18n`): every UI string comes from `locales/en.json` (FR-054)
- [x] End to end: 12 Playwright specs pass, including the quickstart journey and accessibility
- [x] Cross-org access matrix covers every operation in the OpenAPI document (FR-048)
- [x] `contracts/openapi.yaml` regenerated; drift test passes; `http-api.md` lists the same operations
- [x] `cargo audit` and `npm audit`: no open vulnerabilities in shipped code; Trivy findings recorded in `SECURITY.md`

## Success criteria

Criteria that can be built and measured before launch:

- [x] SC-001 Submit leave in under 30 s from opening the form: `web/e2e/quickstart.spec.ts` asserts it (step 5)
- [x] SC-002 Approve in at most two actions: from the email, the confirmation is open after sign-in and one click decides (`quickstart.spec.ts` step 6, `approve.spec.ts`); from the inbox, Approve → Confirm (`a11y.spec.ts`, keyboard only)
- [x] SC-003 Dashboard p95 under 1.5 s at 1,000 employees: `scripts/perf/run.sh full` gave employee p95 133 ms and admin p95 37 ms, with 0 failures out of 6,997 requests (50 VUs, 60 s, dev laptop)
- [ ] SC-004 95% of login codes arrive within 30 s: in tests, codes reach Mailpit in under 2 s. Real delivery depends on the operator's SMTP provider and must be checked on the production instance
- [x] SC-006 No passwords: email codes only; there is no password column or endpoint
- [x] SC-010 Every balance reconciles with its ledger: `balances_reconcile_test.rs` (randomised scenarios) and `quickstart.spec.ts` step 11
- [ ] SC-012 Install in under 30 minutes: the automated install smoke test (`docker compose up` to a signed-in admin) takes 31 s. **Still pending:** a timed run by someone new to the project, following `docs/book` quickstart on a real server with DNS and SMTP. Record the person's role, the date and the time taken below
- [x] SC-013 1 vCPU / 1 GB with 300 employees and 30 users meets SC-003: `scripts/perf/run.sh small` (server container limited to `--cpus 1 --memory 1g`, database on the host) gave employee dashboard p95 197 ms and admin dashboard p95 494 ms, with 0 failures out of 4,204 requests (30 VUs, 60 s)
- [ ] SC-014 Upgrades leave balances unchanged: `scripts/ci/upgrade_test.sh` passes when the old and new images are the same. A real old→new run is only possible once a first release has been published (CI skips until then)
- [x] SC-016 Usable at 360 px and WCAG 2.1 AA: `a11y.spec.ts` covers axe on 22 routes, keyboard-only apply and approve, and no horizontal overflow at 360 px

Outcome criteria measured after launch, not before: SC-005 (availability), SC-007 (onboarding), SC-008 (Keka migration disputes), SC-009 (decision time), SC-011 (spend), SC-015 (adoption).

## Open items before tagging v1.0.0

- [ ] Replace the placeholder repository and image names (`leavo-hq`) in `releases.rs`, `docker-compose.yml`, the docs and the workflows
- [ ] Check Keka column names against a real Keka export (`importers::keka`, `scripts/keka_import.py`)
- [ ] Timed install by someone new (SC-012). Result: _role, date, minutes_
- [ ] Known gaps without a task (see research.md implementation notes): the cleanup job for idempotency keys and unattached uploads; accepting cancellations from email links; an admin screen to revoke comp-off credits
