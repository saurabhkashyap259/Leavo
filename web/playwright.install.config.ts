import { defineConfig, devices } from '@playwright/test'

// Install smoke test (T218, SC-012): runs against the shipped docker-compose.yml started by
// scripts/ci/install_smoke.sh — no dev server. See that script for the environment.
export default defineConfig({
  testDir: './e2e-install',
  timeout: 300_000,
  workers: 1,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: process.env.INSTALL_BASE_URL ?? 'http://localhost:8080',
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
})
