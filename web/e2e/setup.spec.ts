// T088 — US2 end to end (quickstart steps 1, 2 and 10): first-run setup wizard, leave policy,
// and a restricted leave type granted to one employee only.
import { expect, test } from '@playwright/test'
import { latestCode, resetInstance, setupToken } from './support/env'

test.beforeAll(async () => {
  await resetInstance()
})

test('first-run setup, policy and a restricted grant', async ({ page, browser }) => {
  const token = setupToken()
  const domain = `acme-${Date.now()}.test`
  const adminEmail = `admin@${domain}`

  // 1. Any page leads to the wizard on a fresh install.
  await page.goto('/dashboard')
  await expect(page).toHaveURL(/\/setup$/)
  await page.getByLabel('Setup token').fill(token)
  await page.getByRole('button', { name: 'Continue' }).click()

  await page.getByLabel('Organization name').fill('Acme')
  await page.getByLabel('Time zone').selectOption('Asia/Kolkata')
  await page.getByRole('button', { name: 'Continue' }).click()

  await page.getByLabel('Your full name').fill('Asha Admin')
  await page.getByLabel('Work email').fill(adminEmail)
  const sentAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Send code' }).click()
  await page.getByLabel('6-digit code').fill(await latestCode(adminEmail, sentAt))
  await page.getByRole('button', { name: 'Continue' }).click()

  await page.getByRole('button', { name: 'Send test email' }).click()
  await expect(page.getByRole('status')).toContainText(`Test email sent to ${adminEmail}`)
  await page.getByRole('button', { name: 'Continue' }).click()

  const year = new Date().getFullYear()
  await page.getByLabel('Holiday list').selectOption(`IN/${year}`)
  await page.getByRole('button', { name: 'Finish setup' }).click()

  // Signed in as admin, with the setup checklist.
  await expect(page).toHaveURL(/\/admin$/)
  await expect(page.getByRole('heading', { name: 'Finish setting up' })).toBeVisible()
  // Setup is closed now.
  await page.goto('/setup')
  await expect(page).not.toHaveURL(/\/setup$/)

  // 2. Policy: seeded types, ML restricted; carry-forward cap.
  await page.goto('/admin/policy')
  await expect(page.getByRole('cell', { name: 'Casual Leave' })).toBeVisible()
  const ml = page.getByRole('row').filter({ hasText: 'Maternity Leave' })
  await expect(ml).toContainText('Restricted — 0 employees granted')

  await page.goto('/admin/org')
  await page.getByLabel('Carry-forward cap (days)').fill('5')
  await page.getByRole('button', { name: 'Save settings' }).click()
  await expect(page.getByRole('status')).toHaveText('Settings saved.')
  await expect(page.getByRole('list', { name: 'Allowed email domains' })).toContainText(domain)

  // An employee signs up on the whitelisted domain.
  const emp = await browser.newContext()
  const empPage = await emp.newPage()
  const empEmail = `ravi@${domain}`
  await empPage.goto('/signup')
  await empPage.getByLabel('Full name').fill('Ravi')
  await empPage.getByLabel('Work email').fill(empEmail)
  const empSentAt = Date.now() - 1000
  await empPage.getByRole('button', { name: 'Send code' }).click()
  await empPage.getByLabel('6-digit code').fill(await latestCode(empEmail, empSentAt))
  await empPage.getByRole('button', { name: 'Sign in' }).click()
  await expect(empPage).toHaveURL(/\/dashboard$/)
  const codesBefore = (await (await empPage.request.get('/api/v1/me/balances')).json()).map(
    (b: { leave_type: { code: string } }) => b.leave_type.code,
  )
  expect(codesBefore).toEqual(['CL', 'SL', 'CO'])

  // 10. Admin grants Maternity Leave to Ravi only.
  await page.goto('/admin/policy')
  await page
    .getByRole('row')
    .filter({ hasText: 'Maternity Leave' })
    .getByRole('button', { name: 'Edit' })
    .click()
  const drawer = page.getByRole('dialog', { name: 'Edit Maternity Leave' })
  await drawer.getByLabel("Employee's email").fill(empEmail)
  await drawer.getByRole('button', { name: 'Grant' }).click()
  await expect(drawer.getByText(`(${empEmail})`)).toBeVisible()
  await drawer.getByRole('button', { name: 'Cancel' }).click()
  await expect(page.getByRole('row').filter({ hasText: 'Maternity Leave' })).toContainText(
    'Restricted — 1 employee granted',
  )

  const after = await (await empPage.request.get('/api/v1/me/balances')).json()
  const mlCard = after.find((b: { leave_type: { code: string } }) => b.leave_type.code === 'ML')
  expect(mlCard.allotted).toBe(130)
  const adminCodes = (await (await page.request.get('/api/v1/me/balances')).json()).map(
    (b: { leave_type: { code: string } }) => b.leave_type.code,
  )
  expect(adminCodes).not.toContain('ML')
  await emp.close()
})
