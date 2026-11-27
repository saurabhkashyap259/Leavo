# Security & Privacy Checklist: Leavo v1

**Purpose**: Unit-test the *requirements* for authentication, access control, tenant isolation and personal-data handling
**Created**: 2026-11-24
**Feature**: [spec.md](../spec.md) · [research.md](../research.md) R3–R5, R10, R17
**Audience / timing**: Spec author, before `/speckit-tasks` · **Depth**: Standard

## Requirement Completeness

- [x] CHK001 - Does the spec require that deactivating a user ends their active sessions immediately, not just blocks future sign-ins? [Gap, Spec §FR-006] → **Resolved**: Deactivation ends all sessions immediately (FR-006).
- [x] CHK002 - Is a maximum session lifetime specified alongside the sliding extension, so a session cannot be extended indefinitely? [Gap, Spec §FR-005] → **Resolved**: 90-day absolute session cap (FR-005).
- [x] CHK003 - Are setup-token requirements defined (lifetime, single use, regeneration, behaviour after setup completes)? [Gap, Spec §FR-007] → **Resolved**: Token lifetime, single use, regeneration, guessing limit and expiry defined (FR-007).
- [x] CHK004 - Does the spec say what content FYI emails to *external* notify members may contain (leave type, reason, attachment link)? [Gap, Spec §FR-029, §FR-046] → **Resolved**: FYI emails carry name, dates, days, status only (FR-059).
- [x] CHK005 - Are personal-data access/export rights specified for *active* employees, not only deactivated ones? [Gap, Spec §FR-045] → **Resolved**: Every employee can export their own data (FR-045).
- [x] CHK006 - Are requirements for an instance-wide operator role in multi-organization mode (who can see or manage all orgs, if anyone) defined or explicitly excluded? [Gap, Spec §FR-008] → **Resolved**: No instance-wide super admin in v1; listed in Out of Scope (FR-008).

## Requirement Clarity

- [x] CHK007 - Is the "per-network-origin limit" on code requests quantified? The plan uses 20/hour, the spec gives no number. [Clarity, Spec §FR-003] → **Resolved**: 20 code requests per hour per IP (FR-003).

## Requirement Consistency

- [x] CHK008 - Is the immediate "company not on Leavo yet" signup message consistent with "MUST respond identically whether or not the email is registered"? (The contract treats domain status as public; the spec should say so.) [Conflict, Spec §FR-003, §US1-AS2, contracts/http-api.md §Auth] → **Resolved**: Spec now says domain status is public and person-level account existence is not (FR-003).
- [x] CHK009 - When an employee has no approver group, all approvers may decide their requests (§FR-022). Is it consistent that only approvers "for their group" may see that employee's leave type and reason (§FR-027)? [Conflict, Spec §FR-022, §FR-027] → **Resolved**: Visibility defined by 'eligible approvers' = whoever can decide under FR-022 (FR-058).
- [x] CHK010 - Is 7-year audit retention (with leave reasons in before/after values) reconciled with the anonymisation requirement? [Conflict, Spec §FR-044, §FR-045, Assumptions] → **Resolved**: Anonymisation rewrites personal details inside audit records, which are kept (FR-045, Assumptions).
- [x] CHK011 - Is the "neutral colour, no type" privacy rule stated consistently for every surface showing others' leave (dashboard, team calendar, approvals "who else is away", admin org dashboard)? [Consistency, Spec §FR-027, §US4] → **Resolved**: One rule for every surface, incl. approvals panel, admin dashboard and emails (FR-058).

## Exception & Recovery Flows

- [x] CHK012 - Are requirements defined for responding to a security incident (e.g. forced sign-out of all users, rotating the secret key and its effect)? [Gap, Exception Flow] → **Resolved**: Admin 'sign out everyone' and secret-key rotation effects defined (FR-063).

## Notes

- All items resolved 2026-11-24; decisions recorded in spec.md (FR-055–FR-070 added) and mirrored in plan.md, research.md, data-model.md and contracts/.
