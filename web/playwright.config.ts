import { defineConfig, devices } from '@playwright/test'

// E2E runs against the real binary + PostgreSQL + Mailpit:
//   LEAVO_DB_PORT=55432 docker compose -f compose.dev.yml up -d --wait   (from the repo root)
//   npm run e2e                                                           (from web/)
const DB_PORT = process.env.LEAVO_DB_PORT ?? '55432'
export const E2E = {
  baseURL: process.env.E2E_BASE_URL ?? 'http://127.0.0.1:18080',
  // A dedicated database: setup.spec resets it to a fresh install.
  databaseUrl:
    process.env.E2E_DATABASE_URL ?? `postgres://leavo:leavo@localhost:${DB_PORT}/leavo_e2e`,
  mailpit: process.env.E2E_MAILPIT_URL ?? 'http://localhost:8025',
}

export default defineConfig({
  testDir: './e2e',
  timeout: 120_000,
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? 'github' : 'list',
  use: { baseURL: E2E.baseURL, trace: 'retain-on-failure' },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: {
    command:
      'node e2e/support/create-db.mjs && npm run build && cd .. && cargo run -q -p leavo-api -- serve',
    url: `${E2E.baseURL}/readyz`,
    timeout: 300_000,
    reuseExistingServer: !process.env.CI,
    env: {
      E2E_DATABASE_URL: E2E.databaseUrl,
      DATABASE_URL: E2E.databaseUrl,
      LEAVO_BASE_URL: E2E.baseURL,
      LEAVO_BIND: new URL(E2E.baseURL).host,
      LEAVO_SECRET_KEY: 'e2e-'.repeat(16),
      SMTP_URL: 'smtp://localhost:1025',
      EMAIL_FROM: 'Leavo <leave@leavo.test>',
      LEAVO_UPDATE_CHECK: 'false',
      LOG_LEVEL: 'warn',
      // Relative to the repository root, where the server runs.
      STORAGE_LOCAL_PATH: 'target/e2e-uploads',
    },
  },
})
