#!/usr/bin/env bash
# T218 / SC-012 — the shipped docker-compose.yml, untouched, from `docker compose up` to a
# signed-in admin through the setup wizard, within 5 minutes of machine time.
#
#   IMAGE  the image to install (default: leavo:ci, built beforehand)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
IMAGE="${IMAGE:-leavo:ci}"
DIR="$(mktemp -d)"
cleanup() { (cd "$DIR" && docker compose down -v >/dev/null 2>&1) || true; rm -rf "$DIR"; }
trap cleanup EXIT

cp "$ROOT/docker-compose.yml" "$DIR/"
cp "$ROOT/.env.example" "$DIR/.env"
# What an operator fills in, plus Mailpit for the sign-in code.
{
  echo "DB_PASSWORD=smoke-$(date +%s)"
  echo "LEAVO_SECRET_KEY=$(openssl rand -hex 32)"
  echo "SMTP_URL=smtp://mail:1025"
  echo "EMAIL_FROM=Leavo <leave@smoke.test>"
  echo "LEAVO_UPDATE_CHECK=false"
  echo "LEAVO_IMAGE=$IMAGE"
} >> "$DIR/.env"
cat > "$DIR/docker-compose.override.yml" <<'YAML'
services:
  mail:
    image: axllent/mailpit:latest
    ports: ["8025:8025"]
YAML

export INSTALL_DIR="$DIR"
export INSTALL_STARTED_AT="$(($(date +%s) * 1000))"
(cd "$DIR" && docker compose up -d)
cd "$ROOT/web"
npx playwright test -c playwright.install.config.ts
