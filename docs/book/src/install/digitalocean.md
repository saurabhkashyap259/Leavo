# DigitalOcean (App Platform + Managed PostgreSQL)

- **Database**: a Managed PostgreSQL cluster (15+). Add the app as a trusted source; use the
  connection string as `DATABASE_URL` (keep `sslmode=require`).
- **App**: an App Platform service from the image `ghcr.io/leavo-hq/leavo:1`, HTTP port 8080,
  1 instance (Basic, 1 GB). App Platform provides HTTPS and your domain.
- **Files**: a Spaces bucket: `STORAGE=s3`, `S3_ENDPOINT=https://<region>.digitaloceanspaces.com`,
  `S3_REGION=<region>`, plus a Spaces access key.
- **Email**: any SMTP provider (DigitalOcean blocks outbound port 25 on new accounts).
- **Setup token**: the service's runtime logs, search for `Setup token`.

A plain Droplet with the [Linux server guide](vps.md) is the cheapest option.
