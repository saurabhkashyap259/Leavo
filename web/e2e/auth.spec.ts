// T066 — US1 end to end: sign up with an emailed code, sign out, sign in, lock after 5 wrong codes.
import { expect, test } from '@playwright/test'
import { latestCode, seedOrg } from './support/env'

test('sign up, sign out, sign in again', async ({ page }) => {
  const { domain } = await seedOrg()
  const email = `asha@${domain}`

  await page.goto('/dashboard')
  await expect(page).toHaveURL(/\/login/)

  await page.getByRole('link', { name: 'Create an account' }).click()
  await page.getByLabel('Full name').fill('Asha Rao')
  await page.getByLabel('Work email').fill(email)
  let sentAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Send code' }).click()
  await expect(page.getByRole('status')).toContainText(email)
  await page.getByLabel('6-digit code').fill(await latestCode(email, sentAt))
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/dashboard$/)
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible()
  await expect(page.getByRole('link', { name: /Asha Rao/ })).toBeVisible()

  await page.getByRole('link', { name: /Profile/ }).click()
  await page.getByRole('button', { name: 'Sign out', exact: true }).click()
  await expect(page).toHaveURL(/\/login/)
  await page.goto('/dashboard')
  await expect(page).toHaveURL(/\/login/)

  // One code per 30 s per email (FR-003): wait out the window from the signup code.
  await page.waitForTimeout(31_000)
  await page.getByLabel('Work email').fill(email)
  sentAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Send code' }).click()
  await page.getByLabel('6-digit code').fill(await latestCode(email, sentAt))
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page).toHaveURL(/\/dashboard$/)
})

test('five wrong codes lock sign-in', async ({ page }) => {
  const { domain } = await seedOrg()
  const email = `ravi@${domain}`
  await page.goto('/signup')
  await page.getByLabel('Full name').fill('Ravi Kumar')
  await page.getByLabel('Work email').fill(email)
  const sentAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Send code' }).click()
  const real = await latestCode(email, sentAt)
  const wrong = real === '000000' ? '111111' : '000000'

  for (let i = 1; i <= 4; i++) {
    await page.getByLabel('6-digit code').fill(wrong)
    await page.getByRole('button', { name: 'Sign in' }).click()
    await expect(page.getByRole('alert')).toHaveText('That code is wrong or has expired.')
  }
  await page.getByLabel('6-digit code').fill(wrong)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByRole('alert')).toHaveText(
    'Too many wrong attempts. Try again in 15 minutes.',
  )

  // The real code no longer works during the lock.
  await page.getByLabel('6-digit code').fill(real)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByRole('alert')).toHaveText(
    'Too many wrong attempts. Try again in 15 minutes.',
  )
  await expect(page).toHaveURL(/\/signup/)
})
