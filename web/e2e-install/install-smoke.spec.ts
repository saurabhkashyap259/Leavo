// T218 — from `docker compose up` on the shipped docker-compose.yml to a signed-in admin, with
// the setup token read from the logs, in under 5 minutes of machine time (budget for SC-012).
// Started by scripts/ci/install_smoke.sh, which sets INSTALL_DIR and INSTALL_STARTED_AT.
import { expect, test } from '@playwright/test'
import { execFileSync } from 'node:child_process'

const MAILPIT = process.env.INSTALL_MAILPIT_URL ?? 'http://localhost:8025'
const BUDGET_MS = 5 * 60_000

function setupTokenFromLogs(): string {
  const deadline = Date.now() + 120_000
  while (Date.now() < deadline) {
    const logs = execFileSync('docker', ['compose', 'logs', 'leavo'], {
      cwd: process.env.INSTALL_DIR,
      encoding: 'utf8',
    })
    const m = /Setup token: (\S+)/.exec(logs)
    if (m) return m[1]
    execFileSync('sleep', ['2'])
  }
  throw new Error('no "Setup token" line in the leavo logs')
}

async function code(to: string, after: number): Promise<string> {
  const deadline = Date.now() + 60_000
  while (Date.now() < deadline) {
    const res = await fetch(`${MAILPIT}/api/v1/search?query=${encodeURIComponent(`to:"${to}"`)}`)
    const body = (await res.json()) as { messages?: { Subject: string; Created: string }[] }
    const match = (body.messages ?? [])
      .filter((m) => Date.parse(m.Created) >= after)
      .map((m) => /^(\d{6}) is your Leavo code$/.exec(m.Subject))
      .find(Boolean)
    if (match) return match[1]
    await new Promise((r) => setTimeout(r, 1000))
  }
  throw new Error(`no code email for ${to}`)
}

test('fresh install to a signed-in admin', async ({ page }) => {
  const started = Number(process.env.INSTALL_STARTED_AT ?? Date.now())
  const token = setupTokenFromLogs()
  await page.goto('/')
  await expect(page).toHaveURL(/\/setup$/)
  await page.getByLabel('Setup token').fill(token)
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByLabel('Organization name').fill('Smoke Test Co')
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByLabel('Your full name').fill('Smoke Admin')
  await page.getByLabel('Work email').fill('admin@smoke.test')
  const sentAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Send code' }).click()
  await page.getByLabel('6-digit code').fill(await code('admin@smoke.test', sentAt))
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByRole('button', { name: 'Finish setup' }).click()
  await expect(page).toHaveURL(/\/admin$/)

  // The instance is healthy and fully migrated.
  await page.goto('/admin/instance')
  await expect(page.getByText(/applied, 0 pending/)).toBeVisible()
  const elapsed = Date.now() - started
  console.log(`docker compose up → signed-in admin: ${Math.round(elapsed / 1000)} s`)
  expect(elapsed).toBeLessThan(BUDGET_MS)
})
