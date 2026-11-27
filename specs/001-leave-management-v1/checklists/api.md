# API Contract Checklist: Leavo v1

**Purpose**: Unit-test whether the spec and the interface contracts are complete, consistent and traceable to each other
**Created**: 2026-11-24
**Feature**: [spec.md](../spec.md) · [contracts/http-api.md](../contracts/http-api.md) · [contracts/csv-formats.md](../contracts/csv-formats.md)
**Audience / timing**: Spec author, before `/speckit-tasks` · **Depth**: Standard

## Requirement Completeness

- [x] CHK001 - Is every functional requirement FR-001…FR-054 traceable to at least one contract operation, file format or CLI/config entry? [Traceability] → **Resolved**: Traceability appendix added to contracts/http-api.md covering FR-001–FR-070.
- [x] CHK002 - Is the error code catalogue complete in one place? `DOMAIN_NOT_REGISTERED`, `DOMAIN_TAKEN`, `PUBLIC_DOMAIN` and `NO_APPROVER_AVAILABLE` appear in the contract or data model but not in the validation-rules table. [Completeness, data-model §Validation rules, contracts/http-api.md] → **Resolved**: Single error catalogue in data-model.md; contract references it.
- [x] CHK003 - Are role-based field projections specified for *every* response that exposes another user's leave (history detail, approvals `team_away`, team calendar), not only `/away`? [Completeness, Spec §FR-027, §FR-029] → **Resolved**: Field rule covers /away, /team/calendar, /leaves/{id}, /approvals, /admin/dashboard (http-api Conventions, FR-058).
- [x] CHK004 - Are duplicate-submission requirements defined for leave and comp-off creation (e.g. a double click or network retry creating two requests)? [Gap, Spec §FR-019] → **Resolved**: Duplicate submissions create one request (FR-019); Idempotency-Key on create/decide endpoints.
- [x] CHK005 - Is an API versioning and deprecation policy stated for `/api/v1`, consistent with the "no breaking changes within a major version" rule? [Gap, Spec §FR-050] → **Resolved**: Additive-only within /api/v1, one-minor-release deprecation notice (FR-050, http-api Conventions).

## Requirement Consistency

- [x] CHK006 - Do the spec and PRD agree on whether admin reports are in v1? FR-044 requires reports, but the spec Assumptions say "full reports" are planned for v1.1. [Conflict, Spec §FR-044, Assumptions] → **Resolved**: Reports stay in v1 (FR-044); only advanced reports deferred to v1.1 (Assumptions).
- [x] CHK007 - Is the "date of joining editable only once" rule in `PATCH /me` backed by a spec requirement, or should it be recorded as an assumption? [Assumption, contracts/http-api.md §Me] → **Resolved**: Date of joining: employee at signup, then admin-only, no auto re-allotment (FR-064).
- [x] CHK008 - Are CSV export column sets aligned with the leave-history fields listed in Story 5 (half day, notified members, approver comment)? [Consistency, Spec §US5, §FR-028, contracts/csv-formats.md §7] → **Resolved**: FR-028 requires CSV fields = history table fields; csv-formats §7 matches.

## Acceptance Criteria Quality

- [x] CHK009 - Are import dry-run outcomes specified measurably (what counts as an error vs a warning; whether one bad row blocks the whole apply)? [Measurability, Spec §FR-043, contracts/csv-formats.md §5] → **Resolved**: Errors vs warnings defined; apply is all-or-nothing (FR-043, csv-formats §5).

## Notes

- All items resolved 2026-11-24; decisions recorded in spec.md (FR-055–FR-070 added) and mirrored in plan.md, research.md, data-model.md and contracts/.
