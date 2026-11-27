# Upgrade

Leavo follows semantic versioning. The Compose file pins the **major** version
(`ghcr.io/leavo-hq/leavo:1`), so pulling brings minor and patch releases only.

1. **Back up first** — database, uploads and `.env` ([how](backup-restore.md)).
2. Read the release notes' *Upgrading* section.
3. Pull and restart:

   ```bash
   docker compose pull
   docker compose up -d
   ```

4. Open **Admin › Instance**: it shows the new version and `… applied, 0 pending` migrations.

On start Leavo takes a database lock, applies each pending migration in its own transaction and
only then serves traffic. If a migration fails it is rolled back, the log says
`migration <version> failed: <reason>`, the process exits and `/readyz` keeps failing — nothing
is left half-migrated. Fix the cause (often disk space or permissions) and start again, or
restore the backup and run the previous version.

**Downgrades are not supported.** An older image refuses to start on a database migrated by a
newer one. To go back, restore the backup taken before the upgrade and run the previous image.

Every release is tested by upgrading a database from the previous release and checking that
balances are unchanged.
