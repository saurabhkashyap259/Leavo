# Contributing

Thanks for helping! Small, focused pull requests are easiest to review.

## Set up

You need Rust (stable, via rustup — `rust-toolchain.toml` picks the version), Node LTS, Docker
with Compose, and Python 3 for the scripts.

```bash
docker compose -f compose.dev.yml up -d          # PostgreSQL :5432, Mailpit :1025 / :8025
cp .env.example .env.dev                          # uncomment DATABASE_URL, use smtp://localhost:1025
cargo run -p leavo-api -- serve                   # migrates, serves the API on :8080
(cd web && npm ci && npm run dev)                 # Vite on :5173, proxies /api
```

If port 5432 is taken, start the database with `LEAVO_DB_PORT=55432 docker compose -f compose.dev.yml up -d`.

## Tests come first

Every change starts with a failing test (the project constitution, principle X): write the test,
see it fail for the right reason, then make it pass. Pull requests without tests for new
behaviour will be asked for them.

| What | Command |
|---|---|
| Rust unit + integration (needs Docker for PostgreSQL) | `cargo test --workspace` |
| Lints | `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings` |
| Web unit tests, types, lint, translations | `cd web && npm test && npx tsc -b && npm run lint && npm run check:i18n` |
| End to end (dev stack running) | `cd web && npx playwright test` |
| Scripts | `python3 -m pytest scripts/tests` |

Integration tests each get a fresh database; tests that change the clock use `FixedClock`.

## Conventions

- API changes: update the handler's `#[utoipa::path]`, then `cargo run -p leavo-api -- openapi
  --write` and `cd web && npm run gen:api`; the drift test fails otherwise. Describe the change in
  `specs/001-leave-management-v1/contracts/http-api.md`.
- Database changes: a new numbered file in `migrations/` (never edit an applied one), with
  row-level security for any table holding organization data. Add the route to the cross-org
  matrix test if it takes a resource id.
- Interface text lives in `locales/en.json`; never hard-code strings in components.
- Decisions that depart from the spec go in `specs/001-leave-management-v1/research.md`.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/) — releases
  and the changelog are generated from them.

## Sign your commits (DCO)

By contributing you certify the [Developer Certificate of Origin](https://developercertificate.org/).
Add a `Signed-off-by` line to every commit with `git commit -s`.

## Holiday packs

Country packs live in `holidays/<COUNTRY>[-<REGION>]/<YEAR>.csv` (`date,name,kind`), with sources
noted in `holidays/README.md`. `python3 scripts/validate_holiday_packs.py` checks them.

## Conduct

This project follows the [Code of Conduct](CODE_OF_CONDUCT.md).
