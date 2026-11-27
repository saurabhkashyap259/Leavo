# A Linux server (Docker Compose + Caddy)

Any Linux VM with Docker works (Hetzner, Linode, Lightsail, an office server).

1. Follow the [Quickstart](../quickstart.md) steps 1–3 in `/opt/leavo`.
2. Point your domain's DNS at the server.
3. Put Caddy in front for automatic HTTPS. Add to `docker-compose.override.yml` next to the
   shipped file:

   ```yaml
   services:
     leavo:
       ports: !reset []
     caddy:
       image: caddy:2
       restart: unless-stopped
       ports: ["80:80", "443:443"]
       volumes:
         - ./Caddyfile:/etc/caddy/Caddyfile:ro
         - caddy-data:/data
   volumes:
     caddy-data: {}
   ```

   and a `Caddyfile`:

   ```
   leave.example.com {
       reverse_proxy leavo:8080
   }
   ```

4. `docker compose up -d`. Set `LEAVO_BASE_URL=https://leave.example.com` in `.env`.
5. If Caddy (or another proxy) sets `X-Forwarded-For`, add its network to
   `LEAVO_TRUSTED_PROXIES` (e.g. `172.16.0.0/12`) so sign-in rate limits see real client
   addresses.

Back up the `pgdata` and `uploads` volumes and `.env` — see
[Back up and restore](../operate/backup-restore.md).
