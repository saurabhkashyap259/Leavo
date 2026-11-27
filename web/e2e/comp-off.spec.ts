// T157 — US7 end to end (quickstart step 9): request comp-off for last Saturday, the lead
// approves it, the comp-off balance is 1 and expires 90 days after the worked date; last
// Wednesday is refused with COMP_OFF_DATE_INVALID.
import { expect, test, type Page } from '@playwright/test'
import { randomUUID } from 'node:crypto'
import { latestCode, mailTo, seedOrg, sql } from './support/env'

async function signIn(page: Page, email: string) {
  await page.getByLabel('Work email').fill(email)
  const sentAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Send code' }).click()
  await page.getByLabel('6-digit code').fill(await latestCode(email, sentAt))
  await page.getByRole('button', { name: 'Sign in' }).click()
}

const iso = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`

/** The latest `weekday` (0 = Sunday) at least two days ago, so it is past in any time zone. */
function last(weekday: number): Date {
  const d = new Date()
  d.setDate(d.getDate() - 2)
  while (d.getDay() !== weekday) d.setDate(d.getDate() - 1)
  return d
}

test('earn comp-off for a weekend day worked', async ({ browser }) => {
  const { orgId, domain } = await seedOrg()
  await sql(
    `INSERT INTO leave_types (id, org_id, code, name, color, yearly_quota, is_comp_off)
     VALUES ($1, $2, 'CO', 'Comp-off', '#10B981', NULL, true)`,
    [randomUUID(), orgId],
  )
  const groupId = randomUUID()
  await sql('INSERT INTO approver_groups (id, org_id, name) VALUES ($1, $2, $3)', [
    groupId,
    orgId,
    'Engineering',
  ])
  const user = async (email: string, name: string, role: string, group: string | null) => {
    const id = randomUUID()
    await sql(
      `INSERT INTO users (id, org_id, email, full_name, role, approver_group_id)
       VALUES ($1, $2, $3, $4, $5::user_role, $6)`,
      [id, orgId, email, name, role, group],
    )
    return id
  }
  const lead = await user(`lead@${domain}`, 'Lena Lead', 'approver', null)
  await sql('INSERT INTO approver_group_members (group_id, user_id, org_id) VALUES ($1, $2, $3)', [
    groupId,
    lead,
    orgId,
  ])
  await user(`ravi@${domain}`, 'Ravi Kumar', 'employee', groupId)
  const saturday = last(6)
  const wednesday = last(3)

  // Ravi claims last Saturday.
  const raviCtx = await browser.newContext()
  const ravi = await raviCtx.newPage()
  await ravi.goto('/login')
  await signIn(ravi, `ravi@${domain}`)
  await expect(ravi).toHaveURL(/\/dashboard$/)
  await ravi.goto('/comp-offs')
  const form = ravi.getByRole('form', { name: 'Request comp-off' })
  await form.getByLabel('Date worked 1').fill(iso(saturday))
  await form.getByLabel('Reason').fill('Release weekend')
  const requestedAt = Date.now() - 1000
  await form.getByRole('button', { name: 'Request comp-off' }).click()
  await expect(ravi.getByRole('status')).toHaveText('Comp-off requested.')

  // Last Wednesday was a working day.
  await form.getByLabel('Date worked 1').fill(iso(wednesday))
  await form.getByLabel('Reason').fill('Late night')
  await form.getByRole('button', { name: 'Request comp-off' }).click()
  await expect(ravi.getByRole('alert')).toContainText(
    'comp-off can only be claimed for a past weekend or mandatory holiday',
  )

  // The lead approves from the inbox.
  const action = await mailTo(`lead@${domain}`, requestedAt, /comp-off/)
  expect(action.Text).toContain('Release weekend')
  const leadCtx = await browser.newContext()
  const leadPage = await leadCtx.newPage()
  await leadPage.goto('/login')
  await signIn(leadPage, `lead@${domain}`)
  await expect(leadPage).toHaveURL(/\/dashboard$/)
  await leadPage.goto('/approvals')
  const region = leadPage.getByRole('region', { name: 'Comp-off requests' })
  await region.getByRole('button', { name: 'Approve' }).click()
  const decidedAt = Date.now() - 1000
  await leadPage.getByRole('dialog').getByRole('button', { name: 'Confirm approval' }).click()
  await expect(leadPage.getByRole('status')).toHaveText('Approved Ravi Kumar’s request.')
  await mailTo(`ravi@${domain}`, decidedAt, /approved/)

  // Balance 1, expiring 90 days after the worked date.
  const expires = new Date(saturday)
  expires.setDate(expires.getDate() + 90)
  const card = (await (await ravi.request.get('/api/v1/me/balances')).json()).find(
    (b: { leave_type: { code: string } }) => b.leave_type.code === 'CO',
  )
  expect(card.available).toBe(1)
  expect(card.next_expiry).toEqual({ date: iso(expires), days: 1 })
  await ravi.goto('/comp-offs')
  await expect(ravi.getByText(/^1 day expires on /)).toBeVisible()
  await expect(ravi.getByRole('table', { name: 'Credits' })).toContainText('Active')

  await Promise.all([raviCtx.close(), leadCtx.close()])
})
