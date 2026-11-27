# Monitoring and alerts

## Health

- `GET /healthz` — the process is up.
- `GET /readyz` — the database answers and no migration is pending. Use it for load-balancer
  and orchestrator readiness checks.

## Logs

JSON lines on stdout, one per request with a request id and organization id. Sign-in codes,
session tokens, leave reasons and attachment names are never logged.

## Metrics

`GET /metrics` serves Prometheus text. Set `LEAVO_METRICS_TOKEN` and scrape with
`Authorization: Bearer <token>`; otherwise keep the path off the public internet at your proxy.

| Metric | Type | Meaning |
|---|---|---|
| `leavo_http_requests_total{status}` | counter | Requests by status class (`2xx`, `4xx`, `5xx`, …) |
| `leavo_http_request_duration_seconds` | histogram | Request latency |
| `leavo_login_codes_sent_total` | counter | Sign-in codes emailed |
| `leavo_login_codes_verified_total` | counter | Sign-in codes used successfully |
| `leavo_email_send_failures_total` | counter | Emails that failed at least once |
| `leavo_jobs_queued` | gauge | Background jobs due and waiting |
| `leavo_job_oldest_queued_seconds` | gauge | How long the oldest due job has waited |
| `leavo_migrations_pending` | gauge | Migrations not applied (should be 0) |

## Suggested alerts

| Alert | Condition |
|---|---|
| Email failing | `increase(leavo_email_send_failures_total[15m])` above 5% of emails sent |
| Jobs stuck | `leavo_job_oldest_queued_seconds > 600` |
| Server errors | `5xx` above 1% of requests over 10 minutes |
| Not ready | `/readyz` failing for 2 minutes |
| Sign-in trouble | codes sent rising while codes verified stay flat |

**Admin › Instance** shows the same picture for admins: version and available update, emails
sent and failed in the last 24 hours with the last error, the job queue, storage and migrations.
