# Feature Specification: Leavo v1 — Open-Source Leave Management System

**Feature Branch**: `001-leave-management-v1`  
**Created**: 2026-11-24  
**Status**: Draft  
**Input**: User description: "Create specs for future task # Task 001: Leavo v1 — Open-Source Leave Management System" (source: [future-tasks/task-001-leavo-v1-leave-management.md](../../future-tasks/task-001-leavo-v1-leave-management.md), derived from the Leavo BRD, PRD and ERD in [docs/](../../docs/))

## Overview

Leavo is a self-hosted leave management web application. An organization sets its leave policy; employees request leave and compensatory offs (comp-offs), see balances and who is away; approvers decide on requests; and every balance change is recorded so it can be explained. It replaces the leave module of a paid HR suite (Keka) at Aktivolabs, its first user, and is released as open source so any small company can run its own copy at hosting cost only.

v1 covers the leave lifecycle end to end and nothing beyond leave.

## Clarifications

### Session 2026-11-24

- Q: What can a regular employee see about a colleague's leave on "Who's away"? → A: Dates only — name, dates and days with a neutral colour; no leave type (name or colour) and no reason. Approvers see type and reason for their group; admins see everything.
- Q: Should long leave requests need two approvals? → A: No — single level. One approver's decision is final for every request regardless of length; multi-level approval is out of scope for v1.
- Q: Whose absences appear on "Who's away" and the team calendar? → A: Whole organization by default, with a filter to narrow to the viewer's own approver group or department.
- Q: How are supporting documents (e.g. medical notes) handled? → A: Each leave type has an optional "attachment required above N days" setting; a request above that length cannot be submitted without an attachment.
- Q: How is eligibility for restricted leave types (Maternity, Paternity, Child Care) decided? → A: The admin marks a type as "restricted" and explicitly grants it to individual employees; only granted employees see and can request it. No gender or parent-status data is stored.

### Checklist review 2026-11-24

All 46 items in [checklists/](checklists/) (leave rules, security, API, ops) were settled with the defaults recorded in FR-003, 005–008, 010, 012–013, 016–020, 027–035, 043–045, 050, FR-055–FR-070, SC-005, SC-012 and SC-013, and the edge cases. FR-055–FR-070 were added after the original FR-054 and are grouped with related requirements, so they appear out of numeric order.

### Consistency analysis 2026-11-24

Resolved `/speckit-analyze` findings: back-dating is a per-leave-type setting (FR-011, FR-017); approver eligibility for a request is fixed at submission (FR-022, FR-058); the Instance page is limited to the org's own status in multi mode (FR-051); SC-002 allows a confirm step; FR-029 now defers to FR-058.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Sign in with an email code (Priority: P1)

An employee whose work email belongs to a whitelisted domain signs up or signs in by entering their email and then a 6-digit code sent to that inbox. There are no passwords.

**Why this priority**: Nothing else is reachable without identity. It is also a business objective (zero password accounts) and the main security boundary.

**Independent Test**: With an organization and whitelisted domain in place, a new person signs up with a code, signs out, and signs back in with a new code; an outsider's email is refused.

**Acceptance Scenarios**:

1. **Given** the org whitelists `acme.com`, **When** `asha@acme.com` enters her email and name on signup and then the code she received, **Then** an Employee account is created in that org and she is signed in.
2. **Given** a domain not on any whitelist, **When** someone tries to sign up, **Then** they see "Your company isn't on Leavo yet. Ask your HR to register." and no account is created.
3. **Given** a code was issued, **When** 10 minutes pass or the code has already been used, **Then** the code is rejected.
4. **Given** a user enters a wrong code five times, **When** they try again, **Then** sign-in for that email is locked for 15 minutes.
5. **Given** any email address (registered or not), **When** a code is requested, **Then** the "code sent" screen looks identical, so nobody can learn who has an account.
6. **Given** a user requests a new code, **When** they enter the earlier code, **Then** it is rejected.
7. **Given** a signed-in user, **When** they choose "sign out of all devices", **Then** every session they hold ends.

---

### User Story 2 - Set up an organization and its leave policy (Priority: P1)

An operator installs Leavo and completes a first-run setup: organization name, time zone, weekend days, leave year start, the admin's name and email, allowed email domains, a test email, an optional holiday pack and default leave types. The Org Admin then sets each leave type's yearly quota and rules, and the carry-forward cap.

**Why this priority**: Employees cannot sign up or request leave until an organization, domain whitelist and leave types exist.

**Independent Test**: On a fresh install with an empty data store, complete setup to a signed-in admin, configure leave types with quotas, and confirm a new employee receives correct balances.

**Acceptance Scenarios**:

1. **Given** a fresh install with no organization, **When** anyone opens any page, **Then** they are sent to the setup wizard, which requires a one-time setup token shown only to the operator.
2. **Given** setup is complete, **When** the admin finishes the wizard, **Then** they are signed in, their email domain is whitelisted, and a setup checklist (leave types, holidays, approvers, invite employees) is shown.
3. **Given** the admin adds a domain, **Then** it is stored lower-case without `@`, must be a valid domain, cannot be a public email provider (gmail.com, outlook.com, yahoo.com, etc.) and cannot already belong to another organization.
4. **Given** default leave types (Casual, Sick, Child Care, Maternity, Paternity, Comp-off) are seeded, **When** the admin edits a type, **Then** they can set name, short code, colour, yearly quota, half-day allowed, negative balance allowed, negative limit, max days per request, minimum notice, maximum back-dating days, attachment required above N days, availability (all / restricted) and active flag.
5. **Given** Maternity Leave is restricted, **When** the admin grants it to an employee, **Then** only that employee (not others) sees it in Apply Leave and on their dashboard, with the type's yearly quota allotted.
6. **Given** the admin changes a quota mid-year, **When** saving, **Then** they are asked whether it applies from now or from the next leave year.
7. **Given** an employee joins mid-year, **When** their account is created, **Then** each quota is pro-rated by remaining months and rounded to the nearest half day.
8. **Given** the instance runs in single-organization mode (default), **Then** public organization registration is disabled after setup; in multi-organization mode, anyone can register a new organization by verifying their email with a code.

---

### User Story 3 - Request leave and notify colleagues (Priority: P1)

An employee picks a leave type, a date range (with optional half day on the first or last day), a reason, optional attachment and people to notify. Before submitting they see how many working days the request uses and their balance afterwards.

**Why this priority**: Requesting leave is the core job the product replaces.

**Independent Test**: An employee with a known balance submits a request spanning a weekend and a holiday and sees the correct working-day count, pending days on their balance, and emails sent to approvers and notified members.

**Acceptance Scenarios**:

1. **Given** a range Mon–Mon containing a weekend and a company holiday, **When** the employee selects it, **Then** the form shows the working days only and lists the excluded dates.
2. **Given** the request would exceed the available balance, **When** the type does not allow negative balance, **Then** submission is blocked with a clear message.
3. **Given** the range overlaps another pending or approved leave of the same employee (including half days on the same date), **Then** submission is blocked.
4. **Given** a range with zero working days or a To date before the From date, **Then** submission is blocked.
5. **Given** a leave type with minimum notice or max days per request, **Then** those limits are enforced; a type with back-dating days (Sick Leave: 7 by default) may start that many days in the past without minimum notice; a type with 0 cannot start before today.
6. **Given** the employee types 2+ characters in "Notify members", **Then** matching colleagues in their organization are suggested; a valid external email can also be added; at most 20 members.
7. **Given** a valid submission, **Then** the days show as "pending" on the balance, and approvers, the Org Admin and notified members are emailed.
8. **Given** an attachment, **Then** only PDF or image files up to 5 MB are accepted.
9. **Given** Sick Leave requires an attachment above 2 days, **When** the employee requests 3 days of sick leave without one, **Then** submission is blocked with a message asking for the document; 2 days or fewer can be submitted without it.
10. **Given** a Casual Leave request from 29 Dec to 2 Jan, **Then** the December days are charged to this leave year and the January days to the next, and each part must fit that year's available balance.

---

### User Story 4 - Approve or reject requests (Priority: P1)

Approvers chosen by the Org Admin see pending requests in an inbox (oldest first) with the employee's balance and who else on the team is away on those dates. They approve, or reject with a required comment, from the inbox or from buttons in the email.

**Why this priority**: Without approvals, requests never become leave; it is also the audit trail the business needs.

**Independent Test**: Two approvers receive the same request; one approves; the other sees it decided by the first; balances and emails update accordingly.

**Acceptance Scenarios**:

1. **Given** an employee is in an approver group, **When** they request leave, **Then** approvers in that group and the Org Admin receive it; if they have no group, all approvers receive it.
2. **Given** a request, **When** the first approver decides, **Then** the decision is final, the request leaves other inboxes and they see e.g. "Approved by Priya on 3 Dec".
3. **Given** two approvers decide at the same moment, **Then** exactly one decision is recorded and the other is told who decided.
4. **Given** an approver (or the admin) requests their own leave, **Then** it never goes to themselves; it goes to another approver.
5. **Given** an approver rejects, **Then** a comment is required, and the reserved days return to the employee's available balance.
6. **Given** approval, **Then** the days are deducted, and the employee and notified members are emailed the decision with a calendar attachment.
7. **Given** an approver clicks Approve or Reject in the email, **Then** the app opens with the decision pre-filled and they confirm after signing in; the link expires after 72 hours and works once.
8. **Given** a request still pending after 48 hours, **Then** its approvers are reminded (at most once every 24 hours).

---

### User Story 5 - See balances, history and who is away (Priority: P2)

The employee dashboard shows a card per leave type available to them (all-employee types plus restricted types granted to them) (available days in large type; allotted, carried forward, taken, pending), a comp-off card with the next expiry, who is away today / this week / this month, their pending and upcoming requests, and the next 3 holidays. A leave history page lists every request with type, dates, days, half day, status, requested on, decided by, reason, approver comment and notified members, filterable by year, type and status, and exportable to CSV.

**Why this priority**: Visibility of balances and absences is the main daily use after requesting, but it depends on stories 1–4.

**Independent Test**: With seeded leave data, an employee's dashboard numbers match their ledger, and the "who's away" list shows colleagues on approved leave without revealing type or reason.

**Acceptance Scenarios**:

1. **Given** approved leave for colleagues, **When** an employee views "Who's away → Today", **Then** they see name, dates and days with a count badge, shown in a neutral colour, and neither the leave type (name or colour) nor the reason.
2. **Given** the default "Who's away" view lists the whole organization, **When** the employee applies the "My team" filter, **Then** only colleagues in their approver group or department are shown.
3. **Given** an approver or admin, **Then** they also see leave type and reason (approvers for their group only, admins for everyone).
4. **Given** a casual leave balance of 12 allotted + 3 carried, **Then** the card shows both lines separately.
5. **Given** a pending request, **Then** it appears under "My requests" with a Cancel action.

---

### User Story 6 - Cancel leave (Priority: P2)

An employee withdraws a pending request at any time, or cancels approved leave before it starts. Cancelling after start requires an approver to accept.

**Why this priority**: Plans change; without cancellation balances become wrong and need manual fixing.

**Independent Test**: Cancel an approved future leave and confirm the balance is restored and everyone notified is emailed.

**Acceptance Scenarios**:

1. **Given** a pending request, **When** withdrawn, **Then** the reserved days are released.
2. **Given** approved leave starting in the future, **When** cancelled, **Then** the days are returned and the employee, approvers and notified members are emailed.
3. **Given** a 5-day approved leave that started 2 days ago, **When** the employee cancels today and an approver accepts, **Then** the 3 days from today onward are returned and the 2 earlier days stay taken.

---

### User Story 7 - Earn and spend comp-offs (Priority: P2)

An employee who worked on a past weekend day or company holiday requests comp-off credit (full or half day per date, with a reason). It follows the same approval flow. Once approved, the credit appears as a "Comp-off" balance that can be spent like any other leave, oldest credit first, and expires after a set period.

**Why this priority**: Comp-offs are a business must-have but are used less often than regular leave.

**Independent Test**: Request comp-off for last Saturday, approve it, apply comp-off leave, and confirm credit usage and expiry behaviour.

**Acceptance Scenarios**:

1. **Given** a working day, a future date, or a date already claimed, **When** comp-off is requested, **Then** it is rejected.
2. **Given** an approved comp-off of 1 day, **Then** the comp-off balance increases by 1.
3. **Given** several credits, **When** comp-off leave is taken, **Then** the soonest-expiring credit is used first.
4. **Given** a credit 7 days from expiry, **Then** the employee is emailed a warning; when it expires, unused days are removed from the balance and shown as "Expired".

---

### User Story 8 - Holiday calendar (Priority: P2)

Everyone sees company holidays for the current and next year as a list and a month calendar, with past holidays dimmed and the next one highlighted with a countdown. Admins add, edit, delete, import from CSV, copy last year's list, or import a country holiday pack.

**Why this priority**: Holidays drive day counting and comp-off eligibility; employees need them to plan.

**Independent Test**: Admin imports the India pack; a leave request across a listed holiday excludes it from the day count.

**Acceptance Scenarios**:

1. **Given** a holiday inside a leave range, **Then** it is not counted.
2. **Given** a mandatory holiday, **Then** it is a valid comp-off date; an optional holiday is not, and does not reduce the day count of a leave request.
3. **Given** an already approved leave, **When** the admin adds a holiday inside it, **Then** that day is given back to the employee and recorded.
4. **Given** next year's list is empty on 1 November, **Then** the admin sees a reminder banner.

---

### User Story 9 - Year-end carry forward (Priority: P2)

On the first day of the new leave year, every employee gets their new quotas; unused casual leave up to the org-wide cap is carried forward and the rest lapses. Before it runs, the admin can preview and download each employee's carry forward.

**Why this priority**: Must be correct by the first year end after launch, not on day one.

**Independent Test**: With a cap of 5 and an employee holding 8 unused casual days, run year end and confirm 12 + 5 next year and 3 lapsed, and that running it twice changes nothing.

**Acceptance Scenarios**:

1. **Given** unused casual leave above the cap, **Then** only the cap carries forward and the remainder lapses.
2. **Given** other leave types and comp-offs, **Then** nothing carries forward.
3. **Given** the year-end process runs twice for the same year, **Then** balances are not changed twice.
4. **Given** year end completes, **Then** employees are emailed their new balances and the admin receives a summary.

---

### User Story 10 - Manage people, approvers and balances (Priority: P2)

The Org Admin invites employees (single or CSV), edits profiles, changes roles (Employee / Approver / Admin), creates approver groups and assigns employees to them, deactivates leavers, views any employee's balance history, and makes manual balance adjustments with a required reason. The admin also sees an org dashboard (away today, pending count, requests waiting over 48 hours, upcoming holidays), reports (leave by type, employee, month; CSV export) and an audit log.

**Why this priority**: Needed to run the org day to day; invite and roles are needed before company-wide launch.

**Independent Test**: Admin invites a user, makes them an approver in a group, adjusts a balance with a reason, and finds each action in the audit log.

**Acceptance Scenarios**:

1. **Given** an admin tries to remove the last active Org Admin, **Then** it is blocked.
2. **Given** an employee is deactivated, **Then** they can no longer sign in and their history is kept.
3. **Given** a manual adjustment without a reason, **Then** it is rejected.
4. **Given** any request, decision or balance change, **Then** the audit log shows who, what, when, and before/after values.

---

### User Story 11 - Migrate from Keka (Priority: P2)

The admin uploads a CSV export of employees, opening balances per leave type and optional current-year history. A dry run shows users to create, balances to set and rows with errors; applying creates invited users, sets opening balances and imports history as approved leave. A generic CSV template supports other HR tools.

**Why this priority**: Required before company-wide launch at Aktivolabs and to meet the "zero disputes" objective.

**Independent Test**: Import a sample Keka CSV in dry-run mode, fix reported errors, apply, and confirm each employee's balance matches the source file.

**Acceptance Scenarios**:

1. **Given** a row whose email domain is not whitelisted, **Then** the dry run reports it as an error and it is not applied.
2. **Given** a dry run, **Then** no data is changed.
3. **Given** an applied import, **Then** each imported balance is recorded as an import entry traceable in the balance history.
4. **Given** imported history rows, **Then** they appear as approved leave in history and block overlapping requests, but do not reduce the imported balance again.
5. **Given** a dry run with any error row, **Then** the import cannot be applied until the errors are fixed; warning rows do not block it.

---

### User Story 12 - Self-host, upgrade and operate (Priority: P3)

A developer at a small company deploys Leavo on their own cloud by following the docs, with only a database and an email service as dependencies, and upgrades to new versions without manual data changes. An Admin → Instance page shows the running version, whether a newer one exists, email status with a "send test email" button, storage in use and data migration status.

**Why this priority**: Essential for the open-source goal, but Aktivolabs' own launch can proceed before public packaging is polished.

**Independent Test**: On a clean server, follow the documented quick start and reach the setup wizard within 30 minutes; upgrade to the next version and confirm balances are unchanged.

**Acceptance Scenarios**:

1. **Given** the shipped deployment files, **When** started on a fresh server, **Then** the setup wizard is reachable with no other steps.
2. **Given** an upgrade within the same major version, **Then** data changes are applied automatically on start-up and no manual steps are needed.
3. **Given** default settings, **Then** the instance sends no usage data anywhere; the update check can be turned off.

---

### Edge Cases

- A leave range made entirely of weekends/holidays → rejected as zero working days.
- Half day on a single-day request, or half day falling on a weekend/holiday → only working days are counted.
- Two requests submitted at the same time that together exceed the balance → only one succeeds.
- An employee changes approver group while a request is pending → the request stays with the approvers it was sent to, who keep seeing its type and reason; the new group's approvers see the employee's future requests and person-level details (FR-022, FR-058).
- An approver is deactivated while holding pending requests → other approvers and the admin can still decide them.
- Admin is the only approver and requests leave → must be decided by another admin or approver; setup checklist warns when the admin is the sole approver.
- A domain is removed from the whitelist → new signups blocked; existing users keep access until deactivated.
- A leave type is deactivated → it can no longer be requested; history and balances remain visible.
- A restricted-type grant is revoked while a request of that type is pending → the pending request can still be decided; no new requests of that type are allowed.
- An existing "all employees" type is switched to restricted → employees with past usage keep their history; only granted employees can make new requests.
- A mandatory holiday is deleted (or moved) that was inside an approved leave → the day becomes a booked working day again and is deducted with a recorded entry, even if this makes the balance negative; the employee and admins are emailed (FR-035).
- Login emails are delayed → user can request a resend after 30 seconds (max 5 per hour).
- Email service is down → nobody can receive codes; an operator with server access can issue a one-time admin sign-in (FR-067), and the Instance page shows the email failure.
- Dates near midnight → "today" and the leave year are evaluated in the organization's time zone, not the server's.
- Year end runs late or twice → result is identical to a single on-time run.
- External notify email bounces → request still succeeds; failure is visible to operators.
- A request spans the leave-year boundary (e.g. 24 Dec – 2 Jan) → each day is charged to the leave year it falls in, and each year's part must fit that year's balance (FR-055).
- A comp-off credit reaches its expiry date while a pending comp-off leave request is counting on it → the reserved days do not expire until the request is decided; if it is then rejected or withdrawn they expire immediately (FR-060).
- An admin changes weekend days or the time zone after leave exists → existing requests keep their day counts; the change applies to requests submitted afterwards and to scheduling from that moment (FR-062).
- An upgrade's data migration fails → the instance does not start serving, data stays at the last fully applied step, and the log says what failed (FR-069).
- The instance was stopped or scaled to zero across a reminder, expiry or year-end time → the missed work runs when it next starts, with the same result as running on time (FR-070).

## Requirements *(mandatory)*

### Functional Requirements

**Access and identity**

- **FR-001**: System MUST authenticate all users only with a single-use 6-digit code sent to their email; it MUST NOT store passwords.
- **FR-002**: Codes MUST expire 10 minutes after issue, become invalid when a newer code is issued, and lock the email for 15 minutes after 5 wrong attempts.
- **FR-003**: System MUST limit code requests to 1 per 30 seconds and 5 per hour per email, and 20 per hour per network origin (IP address). Responses MUST NOT reveal whether a *person* has an account: a login code request gets the same response whether or not the email belongs to a user. Whether a *company domain* is registered on the instance is not secret, so signup with an unregistered domain is refused immediately with the message in US1-AS2.
- **FR-004**: System MUST allow self-signup only for emails whose domain is on an organization's whitelist, and MUST place the user in that organization.
- **FR-005**: Sessions MUST last 7 days, or 30 days if "keep me signed in" is chosen, and are extended by activity, but no session may last more than 90 days from sign-in. Users MUST be able to sign out of all devices.
- **FR-006**: Deactivated users MUST NOT be able to sign in, and deactivation MUST end all their active sessions immediately; their records MUST be kept.
- **FR-063**: Org Admins MUST be able to sign out every user in their organization at once (e.g. after a suspected compromise). Rotating the instance secret key MUST invalidate all sessions, unused codes and email action links on the instance; this effect MUST be documented for operators.

**Organization and policy**

- **FR-007**: System MUST provide a first-run setup wizard, protected by a one-time setup token, that creates the first organization and Org Admin and sends a test email. The token MUST be generated on first start and shown only in the server log, stay valid until setup completes, be consumed by completing setup, be regenerable by an operator with server access, be rate-limited against guessing (5 wrong tries per 15 minutes), and stop working once setup is complete.
- **FR-008**: System MUST support single-organization mode (default; public registration off after setup) and multi-organization mode (public registration with email-code verification). There is no instance-wide "super admin" in v1: in both modes each organization is managed only by its own Org Admins, and instance operation is done through server configuration and commands.
- **FR-009**: Org Admins MUST be able to manage whitelisted domains; domains MUST be unique across organizations and public email providers MUST be refused.
- **FR-010**: Each organization MUST have a time zone, weekend days (default Saturday and Sunday), leave year start month (default January), carry-forward cap and comp-off expiry period (default 90 days).
- **FR-062**: Changing weekend days or time zone MUST apply only to requests submitted after the change and to scheduling from that moment; existing requests keep their booked days and stored dates. The leave year start month MUST be changeable only while the organization has no leave requests and no balance entries other than initial allotments.
- **FR-011**: Org Admins MUST be able to define leave types with: name, short code, colour, yearly quota (one number for all employees), half-day allowed, negative balance allowed and negative limit (FR-057), max days per request, minimum notice, maximum back-dating days (default 0), attachment required above N days (optional), availability (all employees, or restricted) and active flag. Default types MUST be seeded, with Maternity, Paternity and Child Care seeded as restricted and Sick Leave seeded with 7 back-dating days.
- **FR-012**: Org Admins MUST be able to grant and revoke a restricted leave type for individual employees. A restricted type MUST be hidden from, and not requestable by, employees without a grant. On grant, the employee MUST receive that type's full yearly quota for the current leave year (not pro-rated) as a recorded allotment. Revoking MUST stop new requests, keep existing requests and history, leave days reserved by pending requests in place, and remove the remaining available balance with a recorded lapse entry. System MUST NOT collect gender or parent status for eligibility.
- **FR-013**: Quotas for mid-year joiners MUST be pro-rated by remaining months in the leave year and rounded to the nearest half day. The joining month counts as a full month if the employee joins on or before the 15th, otherwise counting starts from the next month. Employees who joined before the leave year started receive the full quota.
- **FR-056**: When an admin changes a quota mid-year with "apply from now", every affected employee MUST receive a recorded adjustment of (new quota − old quota) × remaining months ÷ 12, rounded to the nearest half day, with remaining months counted from the month of the change using the FR-013 rule. With "apply from next leave year", the new quota MUST be used from the next allotment and current balances MUST NOT change.
- **FR-064**: Date of joining MAY be entered by the employee at signup; after that only an Org Admin may change it. Changing it MUST NOT recalculate existing allotments automatically; the admin uses a manual adjustment if needed.
- **FR-014**: An organization MUST always have at least one active Org Admin.

**Leave requests**

- **FR-015**: Employees MUST be able to request leave with type, from/to dates, optional half day (first or second half) on the first or last day, a required reason (max 500 characters), optional attachment (PDF or image, max 5 MB) and up to 20 notify members (colleagues or any valid email).
- **FR-016**: System MUST show working days and resulting balance before submission, excluding weekends and **mandatory** company holidays. Optional holidays are working days for leave counting in v1.
- **FR-017**: System MUST reject requests with zero working days, overlapping the employee's own pending/approved leave, exceeding the available balance (beyond the type's negative limit, FR-057), breaking notice / max-days rules, or exceeding the type's "attachment required above N days" threshold without an attachment. Back-dating: a request MAY start up to the type's maximum back-dating days before today; a type whose maximum is 0 MUST start today or later. Minimum notice applies only to requests starting today or later, so a back-dated request is exempt from it. (Seeded defaults: Sick Leave 7 days, all other types 0.)
- **FR-018**: Submitted days MUST be reserved as pending; approval MUST deduct them; rejection or cancellation MUST release them.
- **FR-019**: No combination of simultaneous actions MUST be able to spend more than the available balance or book the same day twice. Submitting the same leave or comp-off request twice by accident (double click, network retry) MUST create only one request.
- **FR-020**: Employees MUST be able to withdraw pending requests and cancel approved leave before its start date (all days returned). Cancelling leave that has already started MUST require approver acceptance; on acceptance, only days dated on or after the date the cancellation was requested are returned, and earlier days remain taken.
- **FR-055**: Each booked day MUST be charged to the leave year it falls in. A request that spans two leave years MUST fit each year's available balance separately. For a leave year that has not started yet, the available balance is that year's quota (pro-rated or granted as applicable) minus days already pending or approved in it; carry forward is added only when the year-end process runs.
- **FR-057**: A leave type that allows negative balance MUST have a negative limit (default 5 days) beyond which requests are refused. At year end a negative balance of any type MUST be carried into the new leave year as a recorded deduction; only positive unused balances lapse or carry forward.

**Approvals**

- **FR-021**: Org Admins MUST be able to mark employees as Approvers, create approver groups and assign each employee to at most one group. The Org Admin is always an approver.
- **FR-022**: A request MUST go to the approvers of the employee's group (or all approvers if none) and the Org Admin, never to the requester. The set of eligible approvers for a request is fixed when it is submitted (using the employee's group at that moment); later group changes do not move existing requests.
- **FR-023**: Approval is single-level: the first decision by any eligible approver MUST be final, regardless of request length or leave type; others MUST see who decided and when. Rejection MUST require a comment.
- **FR-024**: Approval emails MUST include Approve/Reject actions that pre-fill the decision, expire after 72 hours, work once, and still require the approver to be signed in.
- **FR-025**: Requests pending over 48 hours MUST trigger a reminder to their approvers, at most once per 24 hours.

**Visibility**

- **FR-026**: Employees MUST see per-type balances showing available, allotted, carried forward (casual), taken and pending.
- **FR-027**: All users MUST see who in their organization is on approved leave today, this week and this month with dates and days, and MUST be able to filter the list and team calendar to their own approver group or department (default: whole organization).
- **FR-058**: Leave type (name and colour) and reason of another person's leave MUST be shown only to that person's **eligible approvers** — the approvers who can decide their requests under FR-022 (their group's approvers, or every approver when they have no group) — and to Org Admins. Everyone else sees name, dates and days in a neutral colour. For a specific request (approvals inbox, request detail, emails about it), eligibility is that request's fixed approver set (FR-022); for person-level views (who's away, team calendar, dashboards), it is the person's current approver group. This rule MUST apply to every place that shows other people's leave: who's away, team calendar, the "who else is away" panel in the approvals inbox, the admin dashboard, and notification emails.
- **FR-028**: Employees MUST see their full leave history with filters (year, type, status) and CSV export; the CSV MUST contain the same fields as the history table.
- **FR-029**: Attachments MUST follow the same visibility rule as leave reasons (FR-058): only the employee, their eligible approvers and admins can open them.

**Comp-offs**

- **FR-030**: Employees MUST be able to request comp-off credit for one or more past weekend days or mandatory company holidays, full or half day each, with a reason and notify members, through the same approval flow. Employees MAY withdraw a pending comp-off request.
- **FR-031**: A date already claimed, a working day (including an optional holiday), or a future date MUST be rejected.
- **FR-032**: Approved credit MUST add to the comp-off balance, be spent oldest-first, expire after the org's expiry period, and trigger a warning email 7 days before expiry. The comp-off balance is not tied to a leave year: credits stay valid across the leave-year boundary until they expire, and year end neither lapses nor carries them.
- **FR-060**: Days of a comp-off credit reserved by a pending comp-off leave request MUST NOT expire while the request is pending; if the request is then rejected or withdrawn after the credit's expiry date, those days expire at that moment.
- **FR-061**: Org Admins MUST be able to revoke an approved comp-off credit only while none of it has been used, with a required reason and a recorded entry. Used credit can only be corrected by a manual adjustment.

**Holidays**

- **FR-033**: All users MUST see the holiday calendar for the current and next year (list and month view), with holiday type Mandatory or Optional.
- **FR-034**: Org Admins MUST be able to add, edit, delete, import (CSV or country holiday pack) and copy last year's holidays. v1 MUST ship the India national holiday pack for 2026 and 2027; state and other country packs are community contributions.
- **FR-035**: Adding a mandatory holiday inside approved or pending leave MUST release that day (returned to the balance with a recorded entry if approved). Deleting a mandatory holiday, or changing it to optional, inside approved leave MUST book that day again and deduct it with a recorded entry, even if the balance goes negative; the employee and admins MUST be emailed. Pending requests are recounted the same way without ledger entries.
- **FR-036**: The admin MUST be reminded if next year's holiday list is empty on 1 November.

**Carry forward and balances**

- **FR-037**: At the start of each leave year, System MUST allot new quotas, carry forward unused casual leave up to the org-wide cap, and lapse the remainder; other types and comp-offs MUST NOT carry forward.
- **FR-038**: The year-end process MUST produce the same result if run more than once for the same year.
- **FR-039**: Org Admins MUST be able to preview and download carry-forward results before they apply.
- **FR-040**: Every balance change (allotment, pro-rating, leave taken/returned, carry forward, lapse, comp-off credit/expiry, manual adjustment, import) MUST be recorded as an entry with who, when, why and the related request; balances MUST always equal the sum of entries.
- **FR-041**: Org Admins MUST be able to adjust any balance manually with a required reason.

**Administration and migration**

- **FR-042**: Org Admins MUST be able to invite employees (single or CSV), edit profiles, change roles, assign approver groups and deactivate users.
- **FR-043**: Org Admins MUST be able to import employees, opening balances and history from a Keka export or a generic CSV template, with a dry run that reports changes, errors and warnings without saving. A row with an error MUST NOT be applied, and the import MUST be applied only when the dry run has zero errors (all or nothing); warnings (e.g. a user who already exists) do not block it. Imported opening balances MUST be recorded as import entries. Imported history is informational: it appears as approved leave (so it shows in history and blocks overlaps) but MUST NOT deduct from the balance again, because the opening balance already reflects it.
- **FR-044**: Org Admins MUST have an org dashboard, reports (leave taken by type, by employee and by month, for a chosen leave year) with CSV export, and an audit log of every request, decision and balance change with before/after values.
- **FR-045**: Every employee MUST be able to download a copy of their own personal data (profile, leave and comp-off history, balance entries). Org Admins MUST be able to export any user's data and anonymise a deactivated user on request. Anonymisation MUST replace the user's name and email everywhere, including audit records, and delete their leave reasons and attachments, while keeping dates, day counts and balance entries so the organization's totals stay correct.

**Notifications**

- **FR-046**: System MUST email: login/signup codes; request confirmation (employee), action needed (approvers + admin) and FYI (notified members); decisions (employee + notified); cancellations (all parties); 48-hour reminders; comp-off expiry warnings; carry-forward results (employee) and summary (admin). Approved-leave emails MUST include a calendar attachment.
- **FR-047**: Emails MUST NOT be sent for changes that did not complete, and failed sends MUST be retried.
- **FR-059**: FYI emails to notified members (colleagues or external addresses) MUST contain only the requester's name, dates, number of days and status. They MUST NOT contain the leave type, reason, attachment, or links that act on the request.

**Operation and self-hosting**

- **FR-048**: Data of one organization MUST never be visible to another organization's users.
- **FR-049**: All dates, "today" and leave years MUST be evaluated in the organization's time zone.
- **FR-050**: Upgrades within a major version MUST apply data changes automatically on start-up with no manual steps and no balance changes. The programmatic interface MUST stay backward compatible within a major version (additions only); anything to be removed MUST be announced as deprecated in the release notes at least one minor release earlier.
- **FR-069**: Each data migration step MUST apply completely or not at all. If a step fails, the instance MUST NOT start serving, MUST leave data at the last completed step, and MUST log which step failed and why. Going back to an earlier version is supported only by restoring a backup taken before the upgrade; the upgrade guide MUST tell operators to take one.
- **FR-070**: Scheduled work (reminders, comp-off expiry and warnings, year-end process, clean-up) that was due while the instance was stopped MUST run when it next starts, with the same result as an on-time run and without sending duplicate emails.
- **FR-067**: An operator with access to the server MUST be able to issue a one-time, 15-minute sign-in for an existing active Org Admin without email, so the instance can be recovered when the email service fails. Each use MUST be recorded in the audit log.
- **FR-068**: The documentation MUST describe how to back up and restore an instance (the database and stored attachments). Restoring a backup into the same version MUST bring back all balances and history exactly.
- **FR-065**: The instance MUST write structured logs that never contain login codes, session tokens, leave reasons or attachments, and MUST expose operational metrics (request latency, codes sent vs. verified, email failures, background-work backlog and age). The documentation MUST list recommended alert conditions: email failure rate over 5% in 15 minutes, background work waiting over 10 minutes, server error rate over 1%.
- **FR-066**: Before the first public release, the project MUST run a public demo instance with sample data that resets every night.
- **FR-051**: An Admin → Instance page MUST show running version, newer-version notice (can be turned off), email status with a test button, storage in use and data migration status. In multi-organization mode, an Org Admin MUST see only the version, the update notice, and their own organization's email delivery status and failed background work; instance-wide figures (storage, migrations, other organizations' jobs) MUST NOT be shown.
- **FR-052**: The instance MUST NOT send usage data externally unless the operator enables it.
- **FR-053**: The product MUST work with any standard email (SMTP) provider and with either local disk or any S3-compatible storage for attachments.
- **FR-054**: Interface text MUST be kept in translation files; English is the only v1 language.

### Key Entities

- **Organization**: A tenant. Name, time zone, weekend days, leave year start, carry-forward cap, comp-off expiry period.
- **Whitelisted Domain**: An email domain allowed to sign up to one organization.
- **User**: A person in an organization. Name, email, employee ID, department, date of joining, role (Employee / Approver / Admin), status (invited / active / deactivated), approver group.
- **Approver Group**: A named set of approvers; employees are assigned to at most one.
- **Leave Type**: A category with quota and rules (half day, negative balance, notice, max days, attachment-required threshold, availability all/restricted, carry forward, active). Comp-off is a special type with no quota.
- **Leave Type Grant**: Permission for one employee to use one restricted leave type; who granted it and when, and whether revoked.
- **Leave Request**: One request for a date range: type, dates, half-day markers, working days, reason, attachment, status (pending / approved / rejected / cancelled), decider, decision time and comment.
- **Booked Day**: One working day (or half day) reserved by a leave request; used to prevent overlaps and to show who is away.
- **Notify Member**: A colleague or external email attached to a request for FYI notifications.
- **Comp-off Request / Credit**: A request for credit on worked dates; an approved credit with worked date, days, days used and expiry date.
- **Balance Entry**: A single + or − change to a user's balance for one leave type and leave year, with reason and link to its cause.
- **Holiday**: A dated company holiday with name and type (mandatory / optional).
- **Login Code / Session**: A short-lived email code and an active signed-in session.
- **Audit Record**: Who did what, when, with before and after values.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An employee can submit a leave request in under 30 seconds from opening the form.
- **SC-002**: An approver can decide a request in at most two actions: from the email (open link → confirm) or from the inbox (Approve → confirm), plus sign-in if needed.
- **SC-003**: Dashboard loads in under 1.5 seconds for 95% of visits for organizations of up to 1,000 employees.
- **SC-004**: 95% of login codes arrive in the user's inbox within 30 seconds.
- **SC-005**: Aktivolabs' production instance is available 99.5% of each month. (Self-hosters control their own availability.)
- **SC-006**: Zero user accounts hold a password.
- **SC-007**: 100% of Aktivolabs employees are onboarded within 2 weeks of launch.
- **SC-008**: Zero unresolved balance disputes 30 days after the Keka migration.
- **SC-009**: Median time from request to decision is under 24 hours.
- **SC-010**: Every displayed balance can be explained entry-by-entry from its balance history (100% reconciliation).
- **SC-011**: Monthly spend on leave tooling at Aktivolabs falls to hosting cost only.
- **SC-012**: A developer following the docs gets from a server with Docker installed, a DNS name pointing at it and SMTP credentials in hand, to signing in as Org Admin after the setup wizard, in under 30 minutes. DNS propagation and email-domain authentication (SPF/DKIM) are excluded.
- **SC-013**: On a 1 vCPU / 1 GB RAM server plus a database, an organization of 300 employees with 30 people using it at once meets SC-003.
- **SC-014**: Upgrading between releases within a major version leaves all balances unchanged in 100% of release upgrade tests.
- **SC-015**: Within 12 months of public release: 10 known production deployments outside Aktivolabs and 15 outside contributors with a merged change.
- **SC-016**: Usable on a 360 px wide phone and meets WCAG 2.1 AA for colour contrast and keyboard use.

## Assumptions

- **Half-day leave** is included in v1 (set per leave type).
- **Carry forward** applies to Casual Leave only, with one org-wide cap.
- **Comp-off expiry** default is 90 days after the worked date, configurable per organization.
- **Optional (restricted) holidays**: holidays can be marked Optional in v1 and are shown on the calendar, but count as working days. Booking a limited number of optional holidays per employee, bulk approve and advanced reports (beyond FR-044) are planned for v1.1.
- **Leave year** runs January–December by default; one leave policy covers all employees (no per-location policies).
- **Example Aktivolabs policy** (pending HR confirmation): Casual 12, Sick 6, Child Care TBD, Maternity per law, Paternity TBD, Comp-off earned only.
- **Audit retention**: 7 years by default.
- **Languages**: English only in v1; interface text is translatable.
- Every user has a working company inbox to receive codes.
- Each deploying company provides an email service with correct sender authentication for its domain, and is responsible for its own legal compliance (for Aktivolabs, India's DPDP Act 2023).
- Keka can export balances and current-year history as CSV/Excel before the subscription ends.
- Adopting companies have someone able to run a container and a database.
- Audit records are kept 7 years; anonymisation (FR-045) rewrites personal details inside them rather than deleting the records.

### Out of Scope (v1)

- Payroll, salary deduction, loss-of-pay processing, leave encashment
- Attendance, clock-in, timesheets
- Multi-level (sequential) approvals, e.g. manager then HR for long leaves
- Native mobile apps (v1 is a responsive web app)
- SSO (Google, Microsoft) and other HR modules (performance, documents, expenses)
- Slack/Teams notifications and a live calendar feed (v2)
- Separate leave policies by location or grade (v2)
- A paid hosted version or paid support
- An instance-wide administrator across organizations in multi-organization mode
- Back-dated leave beyond each type's configured back-dating days (admins correct older missed leave with manual adjustments)
- Helm chart / Kubernetes packaging

### Dependencies

- HR confirmation of quotas, carry-forward cap, comp-off expiry and holiday list
- Keka data export before the subscription ends
- An email service with sender authentication on the company's domain
- Pre-public-release decisions: new product name (current name clashes with a commercial product) and licence (AGPL-3.0 or MIT) — these do not block the internal build
