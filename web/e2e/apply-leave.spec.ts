// T109 — US3 end to end (quickstart step 5): live day count, notify picker, attachment rule,
// pending balance, emails to approvers and notified members, and overlap protection.
import { expect, test } from '@playwright/test'
import { randomUUID } from 'node:crypto'
import { latestCode, seedOrg, sql, subjectsTo } from './support/env'

/** The first Thursday at least a week from today (local date, YYYY-MM-DD). */
function nextThursday(): Date {
  const d = new Date()
  d.setDate(d.getDate() + 7)
  while (d.getDay() !== 4) d.setDate(d.getDate() + 1)
  return d
}
const iso = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
const plus = (d: Date, days: number) => {
  const x = new Date(d)
  x.setDate(x.getDate() + days)
  return x
}

test('request leave with notify, attachment rule and overlap check', async ({ page }) => {
  const { orgId, domain } = await seedOrg()
  const year = new Date().getFullYear()
  const user = async (email: string, name: string, role: string, group: string | null) => {
    const id = randomUUID()
    await sql(
      `INSERT INTO users (id, org_id, email, full_name, role, approver_group_id)
       VALUES ($1, $2, $3, $4, $5::user_role, $6)`,
      [id, orgId, email, name, role, group],
    )
    return id
  }
  await user(`admin@${domain}`, 'Admin', 'admin', null)
  const groupId = randomUUID()
  await sql('INSERT INTO approver_groups (id, org_id, name) VALUES ($1, $2, $3)', [
    groupId,
    orgId,
    'Engineering',
  ])
  const lead = await user(`lead@${domain}`, 'Lena Lead', 'approver', null)
  await sql('INSERT INTO approver_group_members (group_id, user_id, org_id) VALUES ($1, $2, $3)', [
    groupId,
    lead,
    orgId,
  ])
  const ravi = await user(`ravi@${domain}`, 'Ravi Kumar', 'employee', groupId)
  const { rows } = await sql(`SELECT id FROM leave_types WHERE org_id = $1 AND code = 'CL'`, [
    orgId,
  ])
  const cl = rows[0].id as string
  const sl = randomUUID()
  await sql(
    `INSERT INTO leave_types (id, org_id, code, name, color, yearly_quota, attachment_required_above_days, sort_order)
     VALUES ($1, $2, 'SL', 'Sick Leave', '#10B981', 6, 2, 2)`,
    [sl, orgId],
  )
  for (const [type, days] of [
    [cl, 12],
    [sl, 6],
  ] as const) {
    await sql(
      `INSERT INTO balance_ledger (id, org_id, user_id, leave_type_id, leave_year, delta, reason)
       VALUES ($1, $2, $3, $4, $5, $6, 'allotment')`,
      [randomUUID(), orgId, ravi, type, year, days],
    )
  }

  // Ravi signs in.
  const email = `ravi@${domain}`
  await page.goto('/login')
  await page.getByLabel('Work email').fill(email)
  const sentAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Send code' }).click()
  await page.getByLabel('6-digit code').fill(await latestCode(email, sentAt))
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page).toHaveURL(/\/dashboard$/)

  await page.getByRole('link', { name: 'Apply leave' }).click()
  await expect(page).toHaveURL(/\/leave\/apply$/)

  // Thursday → following Tuesday, last day a half day: 3.5 days, weekend not counted.
  const thu = nextThursday()
  const tue = plus(thu, 5)
  await page.getByLabel('Leave type').selectOption({ label: 'Casual Leave (12 available)' })
  await page.getByLabel('From', { exact: true }).fill(iso(thu))
  await page.getByLabel('To', { exact: true }).fill(iso(tue))
  await page.getByLabel('Last day').selectOption({ label: 'Second half' })
  await expect(page.getByText('3.5 working days')).toBeVisible()
  const notCounted = page.getByRole('list', { name: 'Not counted' })
  await expect(notCounted.getByText(/— Weekend$/)).toHaveCount(2)

  // Notify: a colleague by name and an external address.
  const notify = page.getByLabel('Notify members')
  await notify.fill('le')
  await page.getByRole('option', { name: /Lena Lead/ }).click()
  await notify.fill('client@example.com')
  await notify.press('Enter')
  await expect(page.getByRole('button', { name: 'Remove Lena Lead' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Remove client@example.com' })).toBeVisible()

  // Sick leave above 2 days needs a document.
  await page.getByLabel('Leave type').selectOption({ label: 'Sick Leave (6 available)' })
  await expect(page.getByText('A supporting document is required for this request.')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Submit request' })).toBeDisabled()

  // Submit the casual leave.
  await page.getByLabel('Leave type').selectOption({ label: 'Casual Leave (12 available)' })
  await page.getByLabel('Reason').fill('Family function')
  await expect(page.getByText('3.5 working days')).toBeVisible()
  const submittedAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Submit request' }).click()
  await expect(page.getByRole('status')).toHaveText(
    'Request submitted. Your approvers have been emailed.',
  )

  const balances = await (await page.request.get('/api/v1/me/balances')).json()
  const clCard = balances.find((b: { leave_type: { code: string } }) => b.leave_type.code === 'CL')
  expect(clCard.pending).toBe(3.5)
  await expect(page.getByRole('option', { name: 'Casual Leave (8.5 available)' })).toBeAttached()

  // Emails: action to the group's approver and the admin, FYI to the notified members.
  expect(await subjectsTo(`lead@${domain}`, submittedAt, 2)).toEqual(
    expect.arrayContaining([
      'Ravi Kumar requested 3.5 days of Casual Leave',
      expect.stringMatching(/^Ravi Kumar: leave on .* \(pending\)$/),
    ]),
  )
  expect(await subjectsTo(`admin@${domain}`, submittedAt)).toEqual([
    'Ravi Kumar requested 3.5 days of Casual Leave',
  ])
  const fyi = await subjectsTo('client@example.com', submittedAt)
  expect(fyi.some((s) => /^Ravi Kumar: leave on .* \(pending\)$/.test(s))).toBe(true)
  expect(fyi.join(' ')).not.toContain('Casual')

  // Overlapping request: Friday → Monday is refused before submitting.
  await page.getByLabel('Leave type').selectOption({ label: 'Casual Leave (8.5 available)' })
  await page.getByLabel('From', { exact: true }).fill(iso(plus(thu, 1)))
  await page.getByLabel('To', { exact: true }).fill(iso(plus(thu, 4)))
  await expect(page.getByText('You already have leave on some of these dates.')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Submit request' })).toBeDisabled()
})
