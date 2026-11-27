# Self-Hosting & Operations Checklist: Leavo v1

**Purpose**: Unit-test the *requirements* for installing, upgrading, running and recovering a Leavo instance
**Created**: 2026-11-24
**Feature**: [spec.md](../spec.md) · [contracts/configuration.md](../contracts/configuration.md) · [quickstart.md](../quickstart.md)
**Audience / timing**: Spec author, before `/speckit-tasks` · **Depth**: Standard

## Requirement Completeness

- [x] CHK001 - Are backup and restore requirements stated in the spec (what must be backed up, restore expectations), rather than only in the ERD? [Gap, Spec §US12] → **Resolved**: Backup/restore requirement added (FR-068; configuration.md).
- [x] CHK002 - Are observability requirements (logs, metrics, alert conditions) specified at requirement level, given the plan adds them but no FR covers them? [Gap, plan.md R17] → **Resolved**: Logging, metrics and alert conditions required (FR-065).
- [x] CHK003 - Is the public demo instance with a nightly reset in scope for this feature, or explicitly excluded? It is in the task doc but not in the spec. [Gap, Scope] → **Resolved**: Demo instance in scope before first public release (FR-066).
- [x] CHK004 - Are the required holiday packs for v1 specified (which countries, regions and years)? [Clarity, Spec §FR-034] → **Resolved**: India national packs for 2026 and 2027 required; others community (FR-034).

## Exception & Recovery Flows

- [x] CHK005 - Are requirements defined for when the email service is down? Login depends on it, so is there an admin recovery path? [Gap, Exception Flow, Spec §FR-001, §FR-053] → **Resolved**: Operator recovery sign-in for an admin when email fails (FR-067).
- [x] CHK006 - Is behaviour specified when a migration fails during an upgrade (instance state, operator guidance, whether rolling back to the previous version is supported)? [Gap, Recovery, Spec §FR-050] → **Resolved**: Atomic migration steps, no serving on failure, rollback via backup (FR-069).
- [x] CHK007 - Are requirements defined for background work on platforms that scale to zero (missed reminders, expiries or rollover catching up when the instance wakes)? [Gap, Spec §FR-025, §FR-032, §FR-038] → **Resolved**: Missed scheduled work runs once on next start (FR-070).
- [x] CHK008 - Is the effect of changing an organization's time zone after data exists specified (meaning of "today", reminders already scheduled)? [Edge Case, Gap, Spec §FR-049] → **Resolved**: Time-zone change applies from that moment; stored dates unchanged (FR-062).

## Acceptance Criteria Quality

- [x] CHK009 - Is SC-012 (install in under 30 minutes) measurable: from which starting point, and does it include DNS, TLS and email-domain setup? [Measurability, Spec §SC-012] → **Resolved**: SC-012 starting point and exclusions defined.
- [x] CHK010 - Is the workload behind SC-013 (1 vCPU / 1 GB) stated, e.g. number of employees and concurrent users? [Measurability, Spec §SC-013] → **Resolved**: SC-013 workload: 300 employees, 30 concurrent users.
- [x] CHK011 - Is SC-005 (99.5% uptime) scoped to Aktivolabs' own instance, since self-hosters control their own availability? [Clarity, Spec §SC-005] → **Resolved**: SC-005 scoped to Aktivolabs' production instance.

## Notes

- All items resolved 2026-11-24; decisions recorded in spec.md (FR-055–FR-070 added) and mirrored in plan.md, research.md, data-model.md and contracts/.
