<!--
Sync Impact Report
- Version: template → 1.0.0 (initial ratification)
- Principles added: I–X (from Leavo BRD/ERD gates G1–G9 in specs/001-leave-management-v1/plan.md, plus test-first at the maintainer's request)
- Sections added: Additional Constraints, Development Workflow, Governance
- Templates: plan-template.md ✅ (Constitution Check reads gates from this file), spec-template.md ✅ (no change needed), tasks-template.md ✅ (TDD now mandatory: tests are no longer optional for this project), checklist-template.md ✅
- Follow-up TODOs: product name and licence are pending pre-release decisions (BRD §11); they do not affect these principles.
-->

# Leavo Constitution

## Core Principles

### I. PostgreSQL Is the Only Runtime Dependency
Leavo MUST run with nothing but its own binary and a PostgreSQL database. Queues, rate limits, sessions and scheduled work MUST live in PostgreSQL. Email (any SMTP) and attachment storage (local disk or any S3-compatible service) are pluggable, never mandatory extra services.
*Rationale*: small companies must be able to self-host at hosting cost only.

### II. One Deployable, Configured by Environment
Each release MUST ship as one image containing the API, the web app and the job worker. All configuration MUST come from environment variables, which are a versioned public contract; renaming or removing one is a breaking change.

### III. Passwordless and Abuse-Resistant Access
Leavo MUST NOT store passwords. Sign-in codes and tokens MUST be stored only as hashes, be single-use, short-lived and rate-limited, and responses MUST NOT reveal whether a person has an account.

### IV. Tenant Isolation in Two Layers
Every tenant table MUST carry `org_id`, every query MUST filter by the tenant from the session, and PostgreSQL Row-Level Security MUST back this up. Every endpoint MUST be covered by an automated cross-organization access test.

### V. Explainable Balances
Leave balances MUST be derived from an append-only ledger. Every change MUST record who, when, why and its cause, so any displayed number can be reconciled entry by entry. Ledger rows are never updated or deleted.

### VI. Pure, Tested Domain Rules
Leave rules (day counting, validation, pro-rating, carry forward, comp-off allocation, permissions, visibility) MUST live in a crate with no I/O and MUST have unit and property tests.

### VII. Data Minimisation and Privacy
Collect only what leave management needs; no gender, health or family-status fields. Leave reasons and attachments are visible only to the employee, their eligible approvers and admins. Logs MUST NOT contain codes, tokens, reasons or attachments. Export and anonymisation MUST be supported. No telemetry leaves an instance unless the operator enables it.

### VIII. Leave-Only Scope; Settings, Not Special Cases
Leavo manages leave and nothing else (no payroll, attendance or other HR modules). Company-specific behaviour MUST be expressed as configuration or per-type settings, never as code keyed on a company, leave-type code or name.

### IX. Safe Upgrades
Database migrations MUST be forward-only and atomic per step, MUST run automatically on start-up, and MUST NOT change balances. Within a major version the API and configuration change only by addition. Every release MUST pass an upgrade test from the previous release.

### X. Test-First (NON-NEGOTIABLE)
Every behaviour change starts with a failing test at the lowest level that proves it (core unit/property → database/API integration → UI component → end-to-end). Implementation is written only to make that test pass; refactoring happens only on green. Task lists MUST order tests before implementation.

## Additional Constraints

- Latest stable dependency releases are used at setup and kept current (Renovate); deviations are recorded in the feature's research.md.
- Accessibility: WCAG 2.1 AA for colour contrast and keyboard use; usable at 360 px width.
- All user-facing text comes from translation files.
- Times and leave years are evaluated in the organization's time zone.

## Development Workflow

- Every change goes through a pull request with CI green: formatting, lint with warnings as errors, all tests, dependency audit, OpenAPI drift check.
- Specs, plans and tasks for a feature live in `specs/<NNN-feature>/`; `/speckit-analyze` must report no CRITICAL findings before implementation.
- Contributions carry a Developer Certificate of Origin sign-off.

## Governance

This constitution overrides other practice documents. Amendments require a pull request that updates this file, explains the change, bumps the version (MAJOR: principle removed or redefined; MINOR: principle or section added; PATCH: wording), and updates affected templates. Reviewers MUST check new specs, plans and tasks against these principles; any exception must be recorded in the plan's Complexity Tracking table with its justification.

**Version**: 1.0.0 | **Ratified**: 2026-11-24 | **Last Amended**: 2026-11-24
