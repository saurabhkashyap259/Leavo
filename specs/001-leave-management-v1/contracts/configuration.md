# Operator Contract: CLI and Configuration

Leavo is configured only through environment variables (FR-053). Variables are a public, versioned contract: renaming or removing one is a breaking change reserved for a major release.

## Binary commands

| Command | Purpose |
|---|---|
| `leavo serve` | HTTP server (API + web app). Runs migrations under an advisory lock on start. Runs jobs in-process when `LEAVO_RUN_JOBS=true`. |
| `leavo worker` | Jobs only (for larger installs or platforms that scale web to zero). |
| `leavo migrate` | Apply pending migrations and exit. |
| `leavo setup-token` | Regenerate and print the one-time setup token (only while setup is incomplete; the old token stops working) (FR-007). |
| `leavo recovery-login <admin-email>` | Print a one-time sign-in link (`<base>/api/v1/auth/recovery/<id>.<token>`) for an existing active Org Admin, valid 15 minutes, for when email is down; refuses anyone else. Issuing is recorded as `auth.recovery_link`, using it as `auth.recovery_login` (FR-067). |
| `leavo-demo-seed [--reset]` | Fill an empty instance with the demo organization; `--reset` deletes every organization first (demo instance only, FR-066). |
| `leavo --version` | Print version. |

On first start with no organization, `serve` logs: `Setup token: XXXX-XXXX-XXXX (open <LEAVO_BASE_URL>/setup)`. The token is valid until setup completes, allows 5 wrong tries per 15 minutes, and is cleared when setup completes.

## Start-up and upgrade behaviour (FR-050, FR-069)

1. Take the migration advisory lock.
2. Apply each pending migration in its own transaction. If one fails: roll it back, log `migration <version> failed: <error>`, exit with a non-zero code. The instance never serves traffic on a partly migrated database; `/readyz` stays failing.
3. Release the lock, then start serving and (if enabled) the job runner. The scheduler first enqueues any reminders, expiries or rollovers missed while stopped (FR-070).

Downgrades are not supported. To go back, restore the backup taken before the upgrade and run the previous image. Every release's "Upgrading" notes tell operators to take a backup first.

## Backup and restore (FR-068)

Back up together: (a) the PostgreSQL database (`pg_dump -Fc` or the managed service's snapshots), (b) the uploads volume or S3 bucket, (c) `.env` (it holds `LEAVO_SECRET_KEY`). Restore into the **same** Leavo version, then upgrade if needed. Without the original secret key, the restore works but every user must sign in again.

## Secret key rotation (FR-063)

Changing `LEAVO_SECRET_KEY` and restarting signs everyone out and invalidates all unused codes and email action links. Use it after a suspected compromise. Leavo keeps a one-way fingerprint of the key in `instance_settings.secret_fingerprint`; on start, a different key revokes every session, consumes unused sign-in/recovery codes and marks unused action links used, and logs a warning. The key itself is never stored.

## Logs, metrics and alerts (FR-065)

- JSON logs on stdout with request id and org id; never codes, tokens, leave reasons or attachment names.
- `/metrics` (Prometheus text; `Authorization: Bearer $LEAVO_METRICS_TOKEN` when set, else `401`): `leavo_http_requests_total{status="2xx"…}`, `leavo_http_request_duration_seconds` (histogram), `leavo_login_codes_sent_total`, `leavo_login_codes_verified_total`, `leavo_email_send_failures_total`, `leavo_jobs_queued`, `leavo_job_oldest_queued_seconds`, `leavo_migrations_pending`.
- Recommended alerts: email failure rate > 5% over 15 min; oldest queued job > 10 min; 5xx rate > 1%.

## Environment variables

| Variable | Required | Default | Notes |
|---|---|---|---|
| `DATABASE_URL` | Yes | — | `postgres://leavo:***@db:5432/leavo` |
| `LEAVO_BASE_URL` | Yes | — | Public URL used in email links |
| `LEAVO_SECRET_KEY` | Yes | — | ≥ 64 chars; HMAC for codes, sessions, action links. Rotating it signs everyone out |
| `SMTP_URL` | Yes | — | `smtps://user:pass@host:465` or `smtp://host:587?tls=required` |
| `EMAIL_FROM` | Yes | — | `Leavo <leave@acme.com>` |
| `LEAVO_MODE` | No | `single` | `single` \| `multi` |
| `LEAVO_RUN_JOBS` | No | `true` | |
| `LEAVO_BIND` | No | `0.0.0.0:8080` | |
| `LEAVO_TRUSTED_PROXIES` | No | empty | CIDRs whose `X-Forwarded-For` is trusted for per-IP rate limits |
| `STORAGE` | No | `local` | `local` \| `s3` |
| `STORAGE_LOCAL_PATH` | No | `/data/uploads` | |
| `S3_ENDPOINT`, `S3_REGION`, `S3_BUCKET`, `S3_ACCESS_KEY`, `S3_SECRET_KEY` | If `STORAGE=s3` | — | Any S3-compatible service |
| `LEAVO_UPDATE_CHECK` | No | `true` | `false` disables the GitHub releases check |
| `LEAVO_METRICS_TOKEN` | No | empty | When set, `/metrics` requires it |
| `SENTRY_DSN` | No | empty | Error reporting off when empty |
| `LOG_LEVEL` | No | `info` | |
| `DATABASE_MAX_CONNECTIONS` | No | `10` | |

Startup fails fast with a clear message if a required variable is missing or `LEAVO_SECRET_KEY` is too short.

## Shipped files

- `docker-compose.yml` — `leavo` (`ghcr.io/<owner>/leavo:<major>`, overridable with `LEAVO_IMAGE`) + `postgres:18`, volumes `uploads`, `pgdata`; reads `.env`.
- `.env.example` — `DB_PASSWORD` (used by the compose file to build `DATABASE_URL`) and the other four required variables, with comments.
- `deploy/demo/` — demo override (Mailpit) and `leavo-demo-seed --reset` (second binary in the image).
- `compose.dev.yml` — Postgres + Mailpit for contributors.
