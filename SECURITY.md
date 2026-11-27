# Security policy

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately with
[GitHub Security Advisories](../../security/advisories/new) for this repository. Include what is
affected, how to reproduce it, and the impact you see. We aim to acknowledge within 3 working
days and to agree on a fix and disclosure date with you.

## Supported versions

| Version | Security fixes |
|---|---|
| Latest minor of the current major (e.g. 1.4.x) | Yes |
| Older minors of the current major | Upgrade to the latest minor |
| Previous major | 6 months after the next major's release |

Upgrading within a major is a pull and restart; see the operator guide.

## How Leavo protects data

- Every organization's rows are isolated by PostgreSQL row-level security, and a test calls
  every route with another organization's ids (`cross_org_matrix_test.rs`).
- No passwords: one-time email codes (HMAC-stored, rate-limited, locked after 5 wrong tries) and
  HTTP-only, SameSite cookies with server-side sessions.
- Logs never contain codes, tokens, leave reasons or attachment names.
- Attachments are type-checked by content, size-limited, and served through short-lived signed
  links.
- No telemetry; the optional update check makes one anonymous request a day.
- Releases are signed with cosign and scanned with Trivy.

## Dependency and image audits

Findings from `cargo audit`, `npm audit` and Trivy at each release are recorded below.

| Date | Tool | Finding | Status |
|---|---|---|---|
| 2026-11-27 | cargo audit | 0 vulnerabilities in compiled code. `object_store` raised to 0.14 and `testcontainers` to 0.27 to clear advisories. | Fixed |
| 2026-11-27 | cargo audit | RUSTSEC-2023-0071 (`rsa`, Marvin attack) and RUSTSEC-2026-0459 (`libsqlite3-sys`): in `Cargo.lock` only through SQLx's optional MySQL/SQLite drivers, which Leavo does not enable, so they are never compiled. Ignored in `.cargo/audit.toml`. | Not affected |
| 2026-11-27 | cargo audit | Unmaintained warnings: `chrono`, `chrono-tz`, `humantime`, `paste`, `astral-tokio-tar` (test-only, via testcontainers). No known vulnerability; no maintained drop-in replacement yet. | Watching |
| 2026-11-27 | npm audit | 0 vulnerabilities. | Clean |
| 2026-11-27 | Trivy (runtime base `distroless/cc-debian12:nonroot`) | 80 OS-package findings (0 critical, 1 high, 47 medium, 26 low, 6 unknown). The high one is CVE-2026-84782 in `libssl3` 3.0.20, fixed in 3.0.22; the base image has not been rebuilt with it yet. Leavo uses rustls and never loads OpenSSL. The rest are Debian packages with no fix released. | Not exploitable; the next release build picks up the fixed base (the Dockerfile follows the `nonroot` tag) |
