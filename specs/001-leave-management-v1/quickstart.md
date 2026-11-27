# Quickstart: Run and Validate Leavo v1

Two paths: **contributor** (from source) and **operator** (from the published image). Each ends with a validation walkthrough that exercises the P1 user stories end to end. Expected results reference spec IDs.

## Prerequisites

- Docker 24+ with Compose v2
- For contributors: Rust stable (≥ 1.83), Node 22 LTS, `sqlx-cli` (`cargo install sqlx-cli --no-default-features -F postgres,rustls`)

---

## A. Contributor setup (target: < 10 minutes)

```bash
git clone <repo> leavo && cd leavo
docker compose -f compose.dev.yml up -d          # Postgres :5432, Mailpit UI :8025 / SMTP :1025
cp .env.example .env.dev                          # uncomment DATABASE_URL; SMTP_URL=smtp://localhost:1025 etc.
cargo run -p leavo-api -- serve                   # API on :8080, runs migrations, prints setup token
(cd web && npm ci && npm run dev)                 # Vite on :5173, proxies /api to :8080
```

Run tests:

```bash
cargo test --workspace                            # unit + sqlx::test integration (needs Docker)
cargo clippy --workspace -- -D warnings
(cd web && npm run lint && npm test)
npx playwright test                               # E2E against a running stack + Mailpit
```

## B. Operator install (target: < 30 minutes, SC-012)

```bash
mkdir leavo && cd leavo
curl -LO https://<repo>/releases/latest/download/docker-compose.yml
curl -LO https://<repo>/releases/latest/download/.env.example && mv .env.example .env
# Fill: DB_PASSWORD, LEAVO_BASE_URL, LEAVO_SECRET_KEY (openssl rand -hex 32), SMTP_URL, EMAIL_FROM
docker compose up -d
docker compose logs leavo | grep "Setup token"
```

Put a TLS reverse proxy (e.g. Caddy: `leave.acme.com { reverse_proxy localhost:8080 }`) in front, then open `https://leave.acme.com/setup`.

---

## C. Validation walkthrough

Use Mailpit (dev) or a real inbox (operator). Dates assume an org with Sat/Sun weekend.

1. **First-run setup** (Story 2, FR-007)
   - Open any URL → redirected to `/setup`. Enter the setup token.
   - Org "Acme", time zone Asia/Kolkata, weekend Sat+Sun, year starts January.
   - Admin `admin@acme.test`; request code, enter it.
   - Send test email → success shown. Pick holiday pack `IN/<year>`, keep default leave types.
   - ✅ Admin is signed in; setup checklist visible; `acme.test` is whitelisted; `/register` returns 404 (single mode).

2. **Policy** (Story 2, FR-011, FR-012)
   - `/admin/policy`: CL quota 12, SL 6 with attachment above 2 days; ML is restricted.
   - Set carry-forward cap 5.
   - ✅ Seeded types present; ML shows "Restricted — 0 employees granted".

3. **Employee signup** (Story 1, FR-001–FR-004)
   - Sign out. `/signup` with `asha@gmail.com` → ✅ "Your company isn't on Leavo yet…".
   - `/signup` with `asha@acme.test`, name "Asha". Enter a wrong code 5 times → ✅ locked 15 min (use a second user `ravi@acme.test` to continue).
   - Ravi signs up correctly → ✅ dashboard with CL/SL cards pro-rated to the current month (rounded to 0.5); no ML card.

4. **Approver setup** (Story 4, FR-021)
   - As admin: create group "Engineering", make `lead@acme.test` (sign them up first) an approver in it, assign Ravi to it.

5. **Request leave** (Story 3, FR-015–FR-017)
   - As Ravi: Apply leave, CL, a Thursday → following Tuesday, last day second half → ✅ preview shows 3.5 days, excludes Sat/Sun (and any pack holiday).
   - Notify: type "le" → lead suggested; also add `client@example.com`.
   - Try SL for 3 days without attachment → ✅ `ATTACHMENT_REQUIRED`.
   - Submit CL → ✅ CL card shows pending 3.5; Mailpit has emails for lead + admin (action) and client (FYI).
   - Submit an overlapping request → ✅ `OVERLAP`.

6. **Approve** (Story 4, FR-022–FR-024)
   - Open lead's email, click Approve → sign in → confirm. ✅ Request approved; admin's inbox shows "Approved by Lead on <date>"; Ravi + client get decision email with `.ics`.
   - Repeat decision as admin via API with the old `version` → ✅ `409 ALREADY_DECIDED`.

7. **Who's away privacy** (Story 5, FR-027, Q1/Q3)
   - As Asha (no approver): `/dashboard` "This week/month" shows Ravi with dates in neutral colour, **no** type or reason. Toggle "My team" → Ravi hidden if Asha isn't in Engineering/same department.
   - As lead: sees type and reason. ✅

8. **Cancel** (Story 6, FR-020) — cancel the approved future leave → ✅ CL balance restored; ledger shows `leave_reversed +3.5`.

9. **Comp-off** (Story 7, FR-030–FR-032) — request comp-off for last Saturday; lead approves → ✅ CO balance 1, expires in 90 days. Request comp-off for last Wednesday → ✅ `COMP_OFF_DATE_INVALID`.

10. **Restricted grant** (Q5) — admin grants ML to Asha → ✅ only Asha sees ML with full-year quota.

11. **Ledger reconciliation** (SC-010) — `/admin/users/{ravi}/balances` → ✅ every card number equals the sum of the listed ledger rows.

12. **Year end** (Story 9) — in dev, run `cargo test -p leavo-worker rollover` or set org leave-year start to next month and invoke the job with a fixed clock → ✅ carry = min(unused, 5), lapse rest; second run changes nothing.

13. **Upgrade** (Story 12, SC-014) — `docker compose pull && docker compose up -d` to a newer tag → ✅ `/admin/instance` shows new version, migrations applied, balances unchanged.
