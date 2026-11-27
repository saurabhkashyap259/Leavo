# Quickstart

From an empty server to a working organization in about 30 minutes. You need Docker with the
Compose plugin.

## 1. Get the files

```bash
mkdir leavo && cd leavo
curl -LO https://github.com/leavo-hq/leavo/releases/latest/download/docker-compose.yml
curl -LO https://github.com/leavo-hq/leavo/releases/latest/download/.env.example
mv .env.example .env
```

## 2. Fill in `.env`

| Setting | What to put |
|---|---|
| `DB_PASSWORD` | A long random password for the bundled database |
| `LEAVO_BASE_URL` | The address people will open, e.g. `https://leave.example.com` |
| `LEAVO_SECRET_KEY` | The output of `openssl rand -hex 32`. Keep it safe; changing it signs everyone out |
| `SMTP_URL` | Your mail service, e.g. `smtps://user:password@smtp.example.com:465` |
| `EMAIL_FROM` | The sender, e.g. `Leavo <leave@example.com>` |

Every other setting is optional; see [Configuration](operate/configuration.md).

## 3. Start Leavo

```bash
docker compose up -d
docker compose logs leavo | grep "Setup token"
```

Leavo applies its database migrations on start, then prints a one-time **setup token**. Put
HTTPS in front of port 8080 ([example with Caddy](install/vps.md)).

## 4. Run the setup wizard

Open `LEAVO_BASE_URL`. The setup wizard asks for the token, your organization's name and time
zone, your own name and work email (you receive a sign-in code), sends a test email, and offers
a country holiday list. When it finishes you are signed in as the organization's admin.

If you lose the token before finishing: `docker compose exec leavo leavo setup-token`.

## 5. Next steps in the app

The admin home shows a checklist: review the leave types and quotas, check the allowed email
domains, add this year's holidays, set up approver groups, and invite people (one by one, by
CSV, or with the Keka import).
