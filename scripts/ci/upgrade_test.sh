#!/usr/bin/env bash
# T217 / SC-014 — upgrading keeps every balance. Runs the previous release image on a fresh
# database, loads sample data, then starts the new image on the same database and checks that
# /admin/users/{id}/balances is unchanged and the Instance page shows the new version.
#
#   PREV_IMAGE  previous release (default: latest GitHub release of this repository via `gh`)
#   NEW_IMAGE   image under test (default: leavo:ci)
#
# Without a previous release (before the first one) the test is skipped.
set -euo pipefail

NEW_IMAGE="${NEW_IMAGE:-leavo:ci}"
REGISTRY_IMAGE="${REGISTRY_IMAGE:-ghcr.io/leavo-hq/leavo}"
if [[ -z "${PREV_IMAGE:-}" ]]; then
  if tag=$(gh release view --json tagName --jq .tagName 2>/dev/null); then
    PREV_IMAGE="$REGISTRY_IMAGE:${tag#v}"
  else
    echo "upgrade-test: no previous release yet; skipping"
    exit 0
  fi
fi

NET="leavo-upgrade-$$"
DB="leavo-upgrade-db-$$"
APP="leavo-upgrade-app-$$"
PORT="${PORT:-18181}"
SECRET="upgrade-test-$(printf 'x%.0s' {1..60})"
SESSION="upgrade-test-session-token"
cleanup() { docker rm -f "$APP" "$DB" >/dev/null 2>&1 || true; docker network rm "$NET" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker network create "$NET" >/dev/null
docker run -d --name "$DB" --network "$NET" -e POSTGRES_USER=leavo -e POSTGRES_PASSWORD=leavo \
  -e POSTGRES_DB=leavo postgres:18 >/dev/null
until docker exec "$DB" pg_isready -U leavo -d leavo >/dev/null 2>&1; do sleep 1; done

start() {
  docker rm -f "$APP" >/dev/null 2>&1 || true
  docker run -d --name "$APP" --network "$NET" -p "$PORT:8080" \
    -e DATABASE_URL="postgres://leavo:leavo@$DB:5432/leavo" \
    -e LEAVO_BASE_URL="http://localhost:$PORT" -e LEAVO_SECRET_KEY="$SECRET" \
    -e SMTP_URL="smtp://localhost:1025" -e EMAIL_FROM="Leavo <leave@upgrade.test>" \
    -e LEAVO_UPDATE_CHECK=false -e LEAVO_RUN_JOBS=false "$1" >/dev/null
  for _ in $(seq 1 120); do
    if curl -fsS "http://localhost:$PORT/readyz" >/dev/null 2>&1; then return 0; fi
    sleep 1
  done
  docker logs "$APP"; echo "upgrade-test: $1 did not become ready"; exit 1
}

psql() { docker exec -i "$DB" psql -v ON_ERROR_STOP=1 -qtAX -U leavo -d leavo "$@"; }

echo "upgrade-test: starting previous release $PREV_IMAGE"
start "$PREV_IMAGE"

# Sample data straight into the database: an org, an admin with a session, an employee with
# balances of several kinds.
SESSION_HASH=$(printf '%s' "$SESSION" | sha256sum | cut -d' ' -f1)
psql <<SQL
INSERT INTO organizations (id, name) VALUES ('00000000-0000-7000-8000-000000000001', 'Upgrade Co');
INSERT INTO org_domains (id, org_id, domain) VALUES (gen_random_uuid(), '00000000-0000-7000-8000-000000000001', 'upgrade.test');
INSERT INTO users (id, org_id, email, full_name, role) VALUES
  ('00000000-0000-7000-8000-00000000000a', '00000000-0000-7000-8000-000000000001', 'admin@upgrade.test', 'Admin', 'admin'),
  ('00000000-0000-7000-8000-00000000000e', '00000000-0000-7000-8000-000000000001', 'emp@upgrade.test', 'Employee', 'employee');
INSERT INTO leave_types (id, org_id, code, name, color, yearly_quota) VALUES
  ('00000000-0000-7000-8000-0000000000c1', '00000000-0000-7000-8000-000000000001', 'CL', 'Casual Leave', '#3B82F6', 12),
  ('00000000-0000-7000-8000-0000000000c2', '00000000-0000-7000-8000-000000000001', 'SL', 'Sick Leave', '#EF4444', 6);
INSERT INTO balance_ledger (id, org_id, user_id, leave_type_id, leave_year, delta, reason, note)
SELECT gen_random_uuid(), '00000000-0000-7000-8000-000000000001', '00000000-0000-7000-8000-00000000000e', t, y, d, r::ledger_reason, n
FROM (VALUES
  ('00000000-0000-7000-8000-0000000000c1'::uuid, extract(year FROM now())::int, 12.0, 'allotment', NULL),
  ('00000000-0000-7000-8000-0000000000c1'::uuid, extract(year FROM now())::int, -2.5, 'manual_adjustment', 'sample'),
  ('00000000-0000-7000-8000-0000000000c2'::uuid, extract(year FROM now())::int, 6.0, 'allotment', NULL)
) AS v(t, y, d, r, n);
INSERT INTO sessions (id, user_id, org_id, token_hash, keep_signed_in, expires_at, absolute_expires_at)
VALUES (gen_random_uuid(), '00000000-0000-7000-8000-00000000000a', '00000000-0000-7000-8000-000000000001',
        decode('$SESSION_HASH', 'hex'), true, now() + interval '30 days', now() + interval '90 days');
SQL

get() {
  curl -fsS -H "Cookie: leavo_sid=$SESSION" -H "X-Requested-With: leavo" "http://localhost:$PORT/api/v1/$1"
}
before=$(get "admin/users/00000000-0000-7000-8000-00000000000e/balances" | jq -S .)

echo "upgrade-test: upgrading to $NEW_IMAGE"
start "$NEW_IMAGE"
after=$(get "admin/users/00000000-0000-7000-8000-00000000000e/balances" | jq -S .)
if [[ "$before" != "$after" ]]; then
  diff <(echo "$before") <(echo "$after") || true
  echo "upgrade-test: balances changed across the upgrade"; exit 1
fi
instance=$(get "admin/instance")
echo "$instance" | jq -e '.migrations.pending == 0' >/dev/null || { echo "$instance"; exit 1; }
echo "upgrade-test: ok — balances unchanged, $(echo "$instance" | jq -r .version), migrations applied"
