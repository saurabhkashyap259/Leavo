// T221 — quickstart steps 12–13 in the browser: the admin's Instance page shows this version,
// every migration applied, email delivering, and a test email arrives. (Step 12, year end, is
// covered by year_end_rollover_test.rs; the image-to-image upgrade of step 13 by the CI
// upgrade-test job, scripts/ci/upgrade_test.sh.)
import { expect, test } from '@playwright/test'
import { randomUUID } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { latestCode, mailTo, seedOrg, sql } from './support/env'

const version = /^version = "([^"]+)"/m.exec(
  readFileSync(fileURLToPath(new URL('../../Cargo.toml', import.meta.url)), 'utf8'),
)![1]

test('the instance page reports a healthy, current instance', async ({ page }) => {
  const { orgId, domain } = await seedOrg()
  const email = `admin@${domain}`
  await sql(
    `INSERT INTO users (id, org_id, email, full_name, role) VALUES ($1, $2, $3, 'Ops Admin', 'admin')`,
    [randomUUID(), orgId, email],
  )
  await page.goto('/login')
  await page.getByLabel('Work email').fill(email)
  const sentAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Send code' }).click()
  await page.getByLabel('6-digit code').fill(await latestCode(email, sentAt))
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page).toHaveURL(/\/dashboard$/)

  await page.goto('/admin/instance')
  await expect(page.getByText(`Version ${version}`)).toBeVisible()
  await expect(page.getByText(/applied, 0 pending/)).toBeVisible()
  await expect(page.getByText('Update check is off (LEAVO_UPDATE_CHECK=false).')).toBeVisible()

  const queuedAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Send a test email' }).click()
  await expect(page.getByRole('status')).toHaveText(`Test email queued to ${email}.`)
  const mail = await mailTo(email, queuedAt, /test/i)
  expect(mail.Subject.length).toBeGreaterThan(0)
})
