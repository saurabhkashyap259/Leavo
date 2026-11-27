// T124 — US4 end to end (quickstart step 6): the approver opens the Approve link from the email,
// signs in, confirms; the employee and notified members get the decision with a calendar
// invite; a second decision with the old version is refused with who decided.
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

function nextWeekday(): string {
  const d = new Date()
  d.setDate(d.getDate() + 7)
  while (d.getDay() === 0 || d.getDay() === 6) d.setDate(d.getDate() + 1)
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

test('approve from the email link', async ({ browser }) => {
  const { orgId, domain } = await seedOrg()
  const user = async (email: string, name: string, role: string, group: string | null) => {
    const id = randomUUID()
    await sql(
      `INSERT INTO users (id, org_id, email, full_name, role, approver_group_id)
       VALUES ($1, $2, $3, $4, $5::user_role, $6)`,
      [id, orgId, email, name, role, group],
    )
    return id
  }
  const groupId = randomUUID()
  await sql('INSERT INTO approver_groups (id, org_id, name) VALUES ($1, $2, $3)', [
    groupId,
    orgId,
    'Engineering',
  ])
  await user(`admin@${domain}`, 'Admin', 'admin', null)
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
  const day = nextWeekday()
  await sql(
    `INSERT INTO balance_ledger (id, org_id, user_id, leave_type_id, leave_year, delta, reason)
     VALUES ($1, $2, $3, $4, $5, 12, 'allotment')`,
    [randomUUID(), orgId, ravi, cl, Number(day.slice(0, 4))],
  )
  const client = `client-${randomUUID().slice(0, 8)}@example.com`

  // Ravi requests a day off, notifying a client.
  const raviCtx = await browser.newContext()
  const raviPage = await raviCtx.newPage()
  await raviPage.goto('/login')
  await signIn(raviPage, `ravi@${domain}`)
  await expect(raviPage).toHaveURL(/\/dashboard$/)
  const submittedAt = Date.now() - 1000
  const created = await raviPage.request.post('/api/v1/leaves', {
    headers: { 'X-Requested-With': 'leavo', 'Idempotency-Key': randomUUID() },
    data: {
      leave_type_id: cl,
      from_date: day,
      to_date: day,
      reason: 'Family function',
      notify: [{ email: client }],
    },
  })
  expect(created.status()).toBe(201)
  const { id } = await created.json()

  // The lead opens Approve from the email, signs in and confirms.
  const action = await mailTo(`lead@${domain}`, submittedAt, /requested/)
  const approveUrl = /Approve: (\S+)/.exec(action.Text)![1]
  const leadCtx = await browser.newContext()
  const leadPage = await leadCtx.newPage()
  await leadPage.goto(approveUrl)
  await expect(leadPage).toHaveURL(/\/login/)
  await signIn(leadPage, `lead@${domain}`)
  await expect(leadPage).toHaveURL(/\/approvals\?/)
  const dialog = leadPage.getByRole('dialog', { name: 'Approve Ravi Kumar’s request' })
  await expect(dialog).toBeVisible()
  const decidedAt = Date.now() - 1000
  await dialog.getByRole('button', { name: 'Confirm approval' }).click()
  await expect(leadPage.getByRole('status')).toHaveText('Approved Ravi Kumar’s request.')
  await expect(leadPage.getByText('Nothing waiting for you.')).toBeVisible()

  // The link worked once.
  await leadPage.goto(approveUrl)
  await expect(
    leadPage.getByText('This link was already used. The request has been decided.'),
  ).toBeVisible()

  // Ravi and the client get the decision with a calendar invite.
  const mine = await mailTo(`ravi@${domain}`, decidedAt, /approved/)
  expect(mine.Text).toContain('Lena Lead')
  expect(mine.Attachments.map((a) => a.FileName)).toEqual(['leave.ics'])
  const fyi = await mailTo(client, decidedAt, /approved/)
  expect(fyi.Attachments.map((a) => a.FileName)).toEqual(['leave.ics'])
  expect(fyi.Text).not.toContain('Casual')

  const card = (await (await raviPage.request.get('/api/v1/me/balances')).json()).find(
    (b: { leave_type: { code: string } }) => b.leave_type.code === 'CL',
  )
  expect([card.taken, card.pending, card.available]).toEqual([1, 0, 11])

  // The admin's late decision with the old version is refused, naming who decided.
  const adminCtx = await browser.newContext()
  const adminPage = await adminCtx.newPage()
  await adminPage.goto('/login')
  await signIn(adminPage, `admin@${domain}`)
  await expect(adminPage).toHaveURL(/\/dashboard$/)
  const late = await adminPage.request.post(`/api/v1/approvals/leave/${id}/decide`, {
    headers: { 'X-Requested-With': 'leavo', 'Idempotency-Key': randomUUID() },
    data: { decision: 'reject', version: 1, comment: 'Too late' },
  })
  expect(late.status()).toBe(409)
  const body = await late.json()
  expect(body.error.code).toBe('ALREADY_DECIDED')
  expect(body.error.details.decided_by.full_name).toBe('Lena Lead')

  await Promise.all([raviCtx.close(), leadCtx.close(), adminCtx.close()])
})
