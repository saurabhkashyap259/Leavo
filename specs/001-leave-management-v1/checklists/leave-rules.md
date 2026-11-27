# Leave & Balance Rules Checklist: Leavo v1

**Purpose**: Unit-test the *requirements* for leave counting, balances, approvals, comp-offs and carry forward before task generation
**Created**: 2026-11-24
**Feature**: [spec.md](../spec.md) · [data-model.md](../data-model.md)
**Audience / timing**: Spec author, before `/speckit-tasks` · **Depth**: Standard

## Requirement Completeness

- [x] CHK001 - Does the spec define which leave year's balance is charged when a request spans the leave-year boundary (e.g. 24 Dec – 2 Jan, or any boundary when the year starts in a month other than January)? [Gap, Spec §FR-010, §FR-018] → **Resolved**: Each day charged to its own leave year; each part must fit that year (FR-055, US3-AS10).
- [x] CHK002 - Does the spec state which days are returned when cancellation of already-started leave is accepted (all days, or only days on/after the cancellation date)? The plan decides this, the spec does not. [Gap, Spec §FR-020, data-model §leave_requests] → **Resolved**: Only days on/after the cancellation request date are returned (FR-020, US6-AS3).
- [x] CHK003 - Are limits on negative balances specified (how far below zero, and what year-end rollover does with a negative casual balance)? [Gap, Spec §FR-017, §FR-037] → **Resolved**: Per-type negative limit, default 5; negative balances carried as a deduction at year end (FR-057).
- [x] CHK004 - Does the spec say what happens to an employee's remaining balance of a restricted type when the grant is revoked? [Gap, Spec §FR-012] → **Resolved**: Remaining available balance lapses with a recorded entry; pending days stay reserved (FR-012).
- [x] CHK005 - Is the effect of imported Keka history on balances specified (whether history rows deduct again or are informational only)? [Gap, Spec §FR-043, Story 11] → **Resolved**: History is informational: shown and blocks overlaps, no second deduction (FR-043, US11-AS4).

## Requirement Clarity

- [x] CHK006 - Is "pro-rated by remaining months" defined precisely enough to compute (how the joining month itself counts)? [Clarity, Spec §FR-013] → **Resolved**: Joining month counts if joined on/before the 15th; earlier joiners get the full quota (FR-013).
- [x] CHK007 - Is the "apply from now" option for a mid-year quota change defined with a calculable result for existing employees? [Clarity, Spec §US2-AS6] → **Resolved**: Formula (new − old) × remaining months ÷ 12, rounded to 0.5 (FR-056).
- [x] CHK008 - Is it clear whether *Optional* holidays are excluded from the leave day count like *Mandatory* ones, given that optional-holiday booking is deferred to v1.1? [Ambiguity, Spec §FR-016, §FR-033, Assumptions] → **Resolved**: Only mandatory holidays are excluded; optional holidays are working days in v1 (FR-016, FR-031, Assumptions).
- [x] CHK009 - Is the interaction between "minimum notice" and the 7-day sick-leave back-dating allowance stated unambiguously (does back-dating override notice for Sick Leave only)? [Clarity, Spec §FR-017] → **Resolved**: Per-type `max_backdate_days` (Sick Leave seeded 7, others 0); back-dated requests are exempt from notice (FR-011, FR-017; revised after /speckit-analyze U1).

## Requirement Consistency

- [x] CHK010 - Are comp-off credits (expiry based on worked date, not leave year) consistent with balances being tracked per leave year, including credits earned in December and spent in January? [Conflict, Spec §FR-032, §FR-040] → **Resolved**: Comp-off balance is year-independent; credits stay valid across the boundary until expiry (FR-032).
- [x] CHK011 - Do the "deleted holiday does not re-deduct" assumption and the "added holiday refunds the day" rule (§FR-035) leave balances explainable under SC-010? [Consistency, Spec Assumptions, §FR-035, §SC-010] → **Resolved**: Deleting a holiday now re-books and deducts with a recorded entry; assumption removed (FR-035).

## Scenario & Edge Case Coverage

- [x] CHK012 - Are requirements defined for a comp-off credit that expires while a pending leave request is counting on it? [Edge Case, Gap, Spec §FR-032] → **Resolved**: Reserved credit days don't expire while the request is pending (FR-060).
- [x] CHK013 - Are the effects on existing approved/pending leave specified when an admin changes weekend days or the leave-year start month after data exists? [Edge Case, Gap, Spec §FR-010] → **Resolved**: Weekend/time-zone changes apply to new requests only; leave-year start is locked once data exists (FR-062).
- [x] CHK014 - Is approved comp-off *credit* cancellation (not just pending requests) addressed, including credit already spent? [Coverage, Gap, Spec §US7] → **Resolved**: Employees withdraw pending requests; admins revoke unused credits only (FR-030, FR-061).

## Notes

- All items resolved 2026-11-24; decisions recorded in spec.md (FR-055–FR-070 added) and mirrored in plan.md, research.md, data-model.md and contracts/.
