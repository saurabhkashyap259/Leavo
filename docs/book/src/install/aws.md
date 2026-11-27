# AWS (ECS Fargate + RDS + SES)

- **Database**: RDS for PostgreSQL 15+ (db.t4g.micro is enough to start). Enable automated
  backups.
- **Email**: Amazon SES. Verify your domain, leave the sandbox, create SMTP credentials and use
  `SMTP_URL=smtps://<user>:<password>@email-smtp.<region>.amazonaws.com:465`.
- **Files**: an S3 bucket with `STORAGE=s3`, `S3_REGION`, `S3_BUCKET`, `S3_ACCESS_KEY`,
  `S3_SECRET_KEY` (or an IAM user scoped to that bucket). Leave `S3_ENDPOINT` empty for AWS.
- **App**: an ECS Fargate service running `ghcr.io/leavo-hq/leavo:1` (0.5 vCPU / 1 GB), one
  task, container port 8080, behind an Application Load Balancer with an ACM certificate.
  Health check path `/readyz`.
- **Configuration**: store `DATABASE_URL`, `LEAVO_SECRET_KEY`, `SMTP_URL` and the S3 keys in
  Secrets Manager and reference them from the task definition; set `LEAVO_BASE_URL`,
  `EMAIL_FROM` and `LEAVO_TRUSTED_PROXIES` (your VPC CIDR) as plain environment variables.
- **Setup token**: read it from the task's CloudWatch log group (`Setup token:`).

Run a single task, or set `LEAVO_RUN_JOBS=false` on the web tasks and run one extra task with
the command `worker` if you scale out.
