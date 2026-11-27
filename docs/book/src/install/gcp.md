# Google Cloud (Cloud Run + Cloud SQL)

- **Database**: Cloud SQL for PostgreSQL 15+, with automated backups. Connect Cloud Run to it
  through the Cloud SQL connector and use a Unix-socket `DATABASE_URL`, e.g.
  `postgres://leavo:<password>@/leavo?host=/cloudsql/<project>:<region>:<instance>`.
- **App**: deploy `ghcr.io/leavo-hq/leavo:1` (mirror it to Artifact Registry if your policy
  requires) with port 8080, 1 vCPU / 1 GiB.
- **Keep one instance warm**: set **minimum instances to 1** and **CPU always allocated**.
  Leavo runs its reminder, comp-off expiry and year-end jobs in the background; scaled to zero,
  they would only run when someone opens the app (they still catch up, but emails are late).
- **Files**: a Cloud Storage bucket through its S3-compatible XML API with HMAC keys:
  `STORAGE=s3`, `S3_ENDPOINT=https://storage.googleapis.com`, `S3_REGION=auto`.
- **Email**: any SMTP provider (Cloud Run blocks port 25; use 465 or 587).
- **Secrets**: Secret Manager for `LEAVO_SECRET_KEY`, `SMTP_URL` and the database password.
- **Setup token**: Cloud Logging, search for `Setup token`.
