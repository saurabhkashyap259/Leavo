# Configuration

Leavo reads only environment variables. They are a versioned contract: renaming or removing one
only happens in a major release.

| Variable | Required | Default | Notes |
|---|---|---|---|
| `DATABASE_URL` | Yes | — | `postgres://leavo:***@db:5432/leavo` |
| `LEAVO_BASE_URL` | Yes | — | Public address used in email links |
| `LEAVO_SECRET_KEY` | Yes | — | 64+ characters. Changing it signs everyone out ([details](secret-rotation.md)) |
| `SMTP_URL` | Yes | — | `smtps://user:pass@host:465` or `smtp://host:587?tls=required` |
| `EMAIL_FROM` | Yes | — | `Leavo <leave@example.com>` |
| `LEAVO_MODE` | No | `single` | `multi` lets several organizations register on one instance |
| `LEAVO_RUN_JOBS` | No | `true` | Run background jobs inside `serve` |
| `LEAVO_BIND` | No | `0.0.0.0:8080` | |
| `LEAVO_TRUSTED_PROXIES` | No | empty | CIDRs whose `X-Forwarded-For` is trusted |
| `STORAGE` | No | `local` | `local` or `s3` |
| `STORAGE_LOCAL_PATH` | No | `/data/uploads` | |
| `S3_ENDPOINT`, `S3_REGION`, `S3_BUCKET`, `S3_ACCESS_KEY`, `S3_SECRET_KEY` | With `STORAGE=s3` | — | Any S3-compatible service |
| `LEAVO_UPDATE_CHECK` | No | `true` | `false`: never contact GitHub. Leavo sends no telemetry either way |
| `LEAVO_METRICS_TOKEN` | No | empty | When set, `/metrics` needs `Authorization: Bearer <token>` |
| `LOG_LEVEL` | No | `info` | |
| `DATABASE_MAX_CONNECTIONS` | No | `10` | |

Leavo refuses to start, with a clear message, when a required variable is missing or the secret
key is too short.

## Commands

| Command | What it does |
|---|---|
| `leavo serve` | Web app and API; applies migrations first; runs jobs unless `LEAVO_RUN_JOBS=false` |
| `leavo worker` | Background jobs only (when you run several web instances) |
| `leavo migrate` | Apply pending migrations and exit |
| `leavo setup-token` | Print a new setup token (only before setup is finished) |
| `leavo recovery-login <email>` | A one-time sign-in link for an admin ([details](recovery-login.md)) |
| `leavo --version` | |

With Docker Compose: `docker compose exec leavo leavo <command>`.
