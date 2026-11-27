// T136 — US5 end to end (quickstart step 7): an employee sees a colleague's leave with dates
// in a neutral colour and no type or reason; "My team" hides people outside their group and
// department; the colleague's approver sees type and reason (FR-027, FR-058).
import { expect, test, type Page } from '@playwright/test'
import { randomUUID } from 'node:crypto'
import { latestCode, seedOrg, sql } from './support/env'

async function signIn(page: Page, email: string) {
  await page.goto('/login')
  await page.getByLabel('Work email').fill(email)
  const sentAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Send code' }).click()
  await page.getByLabel('6-digit code').fill(await latestCode(email, sentAt))
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page).toHaveURL(/\/dashboard$/)
}

/** Today in the org's time zone (Asia/Kolkata, the default). */
const todayIst = () => new Date(Date.now() + 5.5 * 3600_000).toISOString().slice(0, 10)

test("who's away hides type and reason from colleagues", async ({ browser }) => {
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
  const eng = randomUUID()
  await sql('INSERT INTO approver_groups (id, org_id, name) VALUES ($1, $2, $3)', [
    eng,
    orgId,
    'Engineering',
  ])
  const lead = await user(`lead@${domain}`, 'Lena Lead', 'approver', null)
  await sql('INSERT INTO approver_group_members (group_id, user_id, org_id) VALUES ($1, $2, $3)', [
    eng,
    lead,
    orgId,
  ])
  const ravi = await user(`ravi@${domain}`, 'Ravi Kumar', 'employee', eng)
  await user(`asha@${domain}`, 'Asha Rao', 'employee', null)
  const { rows } = await sql(`SELECT id FROM leave_types WHERE org_id = $1 AND code = 'CL'`, [
    orgId,
  ])

  // Ravi's approved day off today (seeded directly; requesting and approving are covered by
  // apply-leave.spec and approve.spec).
  const day = todayIst()
  const request = randomUUID()
  await sql(
    `INSERT INTO leave_requests (id, org_id, user_id, leave_type_id, from_date, to_date, days,
                                 reason, status, routed_group_id, decided_by, decided_at)
     VALUES ($1, $2, $3, $4, $5, $5, 1, 'Private family matter', 'approved', $6, $7, now())`,
    [request, orgId, ravi, rows[0].id, day, eng, lead],
  )
  await sql(
    `INSERT INTO leave_request_days (id, leave_request_id, org_id, user_id, day, leave_year, portion)
     VALUES ($1, $2, $3, $4, $5, $6, 1.0)`,
    [randomUUID(), request, orgId, ravi, day, Number(day.slice(0, 4))],
  )

  // Asha (no approver, different team): Ravi's dates in a neutral colour, nothing else.
  const asha = await (await browser.newContext()).newPage()
  await signIn(asha, `asha@${domain}`)
  await asha.getByRole('tab', { name: 'This week' }).click()
  const list = asha.getByRole('list', { name: "Who's away" })
  const item = list.getByRole('listitem').filter({ hasText: 'Ravi Kumar' })
  await expect(item).toBeVisible()
  await expect(item.getByTestId('away-dot')).toHaveAttribute('data-neutral', 'true')
  await expect(item).not.toContainText('Casual Leave')
  await expect(item).not.toContainText('Private family matter')
  await expect(asha.getByText('1 person away today')).toBeVisible()
  // The API does not send the fields at all.
  const api = await asha.request.get('/api/v1/away?range=week')
  const body = await api.json()
  expect(body.items[0]).not.toHaveProperty('leave_type')
  expect(body.items[0]).not.toHaveProperty('reason')

  // My team: Asha shares neither Ravi's group nor a department.
  await asha.getByRole('checkbox', { name: 'My team' }).check()
  await expect(asha.getByText('Nobody is away.')).toBeVisible()

  // The team calendar follows the same rule.
  await asha.goto('/team')
  const cell = asha.getByRole('cell', { name: new RegExp(`^${Number(day.slice(8))} `) })
  await expect(cell.getByText('Ravi Kumar')).toBeVisible()
  await expect(cell.getByRole('listitem')).not.toHaveAttribute('title', /.+/)

  // Ravi's approver sees type and reason.
  const leadPage = await (await browser.newContext()).newPage()
  await signIn(leadPage, `lead@${domain}`)
  await leadPage.getByRole('tab', { name: 'This week' }).click()
  const leadItem = leadPage
    .getByRole('list', { name: "Who's away" })
    .getByRole('listitem')
    .filter({ hasText: 'Ravi Kumar' })
  await expect(leadItem).toContainText('Casual Leave')
  await expect(leadItem).toContainText('Private family matter')
  await expect(leadItem.getByTestId('away-dot')).toHaveAttribute('data-neutral', 'false')
})
