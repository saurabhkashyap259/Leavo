# Azure (Container Apps + PostgreSQL)

- **Database**: Azure Database for PostgreSQL – Flexible Server, 15+, with backups on.
- **App**: an Azure Container App running `ghcr.io/leavo-hq/leavo:1`, ingress on port 8080,
  **minimum replicas 1** so background jobs keep running. Liveness probe `/healthz`, readiness
  probe `/readyz`.
- **Files**: Azure Blob Storage has no S3 API; use local storage on an Azure Files volume
  mounted at `/data/uploads` (`STORAGE=local`), or an S3-compatible gateway.
- **Email**: Azure Communication Services Email (SMTP) or any SMTP provider.
- **Secrets**: Container Apps secrets (or Key Vault references) for `DATABASE_URL`,
  `LEAVO_SECRET_KEY` and `SMTP_URL`.
- **Setup token**: Log Analytics / the app's console logs, search for `Setup token`.
