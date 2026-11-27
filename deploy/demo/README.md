# Demo instance

A public demo with sample data, reset every night (FR-066).

## Run it

```bash
cp .env.example .env            # fill DB_PASSWORD, LEAVO_BASE_URL, LEAVO_SECRET_KEY
docker compose -f docker-compose.yml -f deploy/demo/compose.demo.yml up -d
docker compose -f docker-compose.yml -f deploy/demo/compose.demo.yml exec leavo leavo-demo-seed --reset
```

Visitors sign in as `admin@demo.leavo.test`, `priya@demo.leavo.test` (approver) or
`asha@demo.leavo.test` (employee). Codes are emailed to the bundled Mailpit, open at port 8025 —
link to it from the demo's landing page. Never point a demo at real email or real people.

## Nightly reset

`.github/workflows/demo-reset.yml` connects over SSH at 02:00 UTC and runs the seed with
`--reset`. It needs the repository secrets `DEMO_HOST`, `DEMO_USER`, `DEMO_SSH_KEY` and
`DEMO_PATH` (the directory holding the compose files).
