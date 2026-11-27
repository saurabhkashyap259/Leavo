#!/usr/bin/env bash
# T225 — run the dashboard load check (SC-003, SC-013).
#
#   scripts/perf/run.sh            # "full": 1,000 employees, 50 people at once, release build
#   scripts/perf/run.sh small      # "small": 300 employees, 30 people at once, the server in a
#                                  # container limited to 1 vCPU and 1 GB (needs LEAVO_IMAGE)
#
# Needs the dev database (LEAVO_DB_PORT=55432 docker compose -f compose.dev.yml up -d --wait),
# psql, and k6 (or Docker, to use the grafana/k6 image). It uses its own database, leavo_perf,
# which it resets on every run.
set -euo pipefail
cd "$(dirname "$0")/../.."

PROFILE="${1:-full}"
case "$PROFILE" in
  full) EMPLOYEES="${EMPLOYEES:-1000}" VUS="${VUS:-50}" ;;
  small) EMPLOYEES="${EMPLOYEES:-300}" VUS="${VUS:-30}" ;;
  *) echo "usage: $0 [full|small]" >&2; exit 2 ;;
esac
DURATION="${DURATION:-2m}"
DOMAIN=perf.leavo.test
PORT="${PERF_PORT:-18090}"
DB_PORT="${LEAVO_DB_PORT:-55432}"
ADMIN_URL="${PERF_ADMIN_DATABASE_URL:-postgres://leavo:leavo@localhost:$DB_PORT/leavo}"
DB_URL="${PERF_DATABASE_URL:-postgres://leavo:leavo@localhost:$DB_PORT/leavo_perf}"

# Seeding deletes every organization in the database: never point it at a real one.
case "${DB_URL##*/}" in
  *perf*) ;;
  *) echo "PERF_DATABASE_URL must name a database with 'perf' in it (it is reset)" >&2; exit 2 ;;
esac

echo "==> database ${DB_URL##*/}"
psql "$ADMIN_URL" -qtAc "SELECT 1 FROM pg_database WHERE datname = 'leavo_perf'" | grep -q 1 \
  || psql "$ADMIN_URL" -qc "CREATE DATABASE leavo_perf"

echo "==> seeding $EMPLOYEES employees"
DATABASE_URL="$DB_URL" DEMO_DOMAIN="$DOMAIN" \
  cargo run -q --release -p leavo-api --bin leavo-demo-seed -- --reset --employees "$EMPLOYEES"

SERVER_ENV=(
  LEAVO_BASE_URL="http://127.0.0.1:$PORT"
  LEAVO_SECRET_KEY="perf-perf-perf-perf-perf-perf-perf-perf-perf-perf-perf-perf-perf"
  SMTP_URL="smtp://localhost:1025"
  EMAIL_FROM="Leavo <leave@leavo.test>"
  LEAVO_UPDATE_CHECK=false
  LOG_LEVEL=warn
)

cleanup() {
  [[ -n "${SERVER_PID:-}" ]] && kill "$SERVER_PID" 2>/dev/null || true
  [[ "$PROFILE" == small ]] && docker rm -f leavo-perf >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "==> starting the server ($PROFILE)"
if [[ "$PROFILE" == full ]]; then
  cargo build -q --release -p leavo-api --bin leavo
  env "${SERVER_ENV[@]}" DATABASE_URL="$DB_URL" LEAVO_BIND="127.0.0.1:$PORT" \
    STORAGE_LOCAL_PATH=target/perf-uploads ./target/release/leavo serve &
  SERVER_PID=$!
else
  : "${LEAVO_IMAGE:?set LEAVO_IMAGE to a built image, e.g. docker build -t leavo:perf . && LEAVO_IMAGE=leavo:perf}"
  docker rm -f leavo-perf >/dev/null 2>&1 || true
  # The container reaches the database on the host through host.docker.internal.
  args=()
  for kv in "${SERVER_ENV[@]}"; do args+=(-e "$kv"); done
  docker run -d --name leavo-perf --cpus 1 --memory 1g \
    --add-host host.docker.internal:host-gateway -p "127.0.0.1:$PORT:8080" \
    "${args[@]}" -e LEAVO_BIND=0.0.0.0:8080 \
    -e DATABASE_URL="${DB_URL/localhost/host.docker.internal}" \
    "$LEAVO_IMAGE" >/dev/null
fi
for _ in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$PORT/readyz" >/dev/null && break
  sleep 1
done
curl -fs "http://127.0.0.1:$PORT/readyz" >/dev/null || { echo "server did not become ready" >&2; exit 1; }

# One session per person, with the token dashboard_load.js sends: "perf-<email>". Created after
# the server starts: a server started with a different LEAVO_SECRET_KEY signs everyone out.
psql "$DB_URL" -qc "
  DELETE FROM sessions;
  INSERT INTO sessions (id, user_id, org_id, token_hash, keep_signed_in, expires_at, absolute_expires_at)
  SELECT gen_random_uuid(), id, org_id, sha256(convert_to('perf-' || email, 'UTF8')), true,
         now() + interval '1 day', now() + interval '1 day'
  FROM users;"

echo "==> k6: $VUS people at once for $DURATION"
K6_ENV=(-e DOMAIN="$DOMAIN" -e EMPLOYEES="$EMPLOYEES" -e VUS="$VUS" -e DURATION="$DURATION")
if command -v k6 >/dev/null; then
  k6 run "${K6_ENV[@]}" -e BASE_URL="http://127.0.0.1:$PORT" scripts/perf/dashboard_load.js
else
  docker run --rm -i --add-host host.docker.internal:host-gateway grafana/k6:latest run \
    "${K6_ENV[@]}" -e BASE_URL="http://host.docker.internal:$PORT" - < scripts/perf/dashboard_load.js
fi
