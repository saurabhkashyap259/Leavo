# Back up and restore

Back up three things **together**:

1. **The database** — `pg_dump -Fc`, or your managed database's snapshots.
2. **Uploaded files** — the `uploads` volume or your S3 bucket.
3. **`.env`** — it holds `LEAVO_SECRET_KEY`.

With the bundled Compose file:

```bash
docker compose exec -T db pg_dump -U leavo -Fc leavo > leavo-$(date +%F).dump
docker run --rm -v leavo_uploads:/data -v "$PWD":/backup alpine \
  tar czf /backup/uploads-$(date +%F).tgz -C /data .
cp .env env-$(date +%F).backup
```

Keep copies off the server and test a restore now and then.

## Restore

Restore into the **same Leavo version** the backup came from, then upgrade if needed.

```bash
docker compose down
docker compose up -d db
docker compose exec -T db dropdb -U leavo leavo
docker compose exec -T db createdb -U leavo leavo
docker compose exec -T db pg_restore -U leavo -d leavo < leavo-2026-11-26.dump
docker run --rm -v leavo_uploads:/data -v "$PWD":/backup alpine \
  sh -c 'rm -rf /data/* && tar xzf /backup/uploads-2026-11-26.tgz -C /data'
docker compose up -d
```

Without the original secret key the restore still works, but everyone must sign in again and
unused sign-in codes and email links stop working.
