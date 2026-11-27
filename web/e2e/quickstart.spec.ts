// T227 — the whole validation walkthrough (quickstart.md section C, steps 1–11) as one journey on
// a fresh install, plus the timing criteria:
//   SC-001: the apply-leave form can be filled in and submitted within 30 s of opening it.
//   SC-002: approving from the email takes at most 2 actions after signing in.
// The browser drives what people do on screen (setup, signup, apply, approve from email); the
// remaining checks call the same API the screens use.
//
// Differences from the written walkthrough, kept so the run is repeatable:
// - Asha is locked out in step 3 (as intended), so a second colleague without an approver role,
//   Dev, is the viewer in step 7 and receives the restricted grant in step 10.
// - The leave week is picked so no pack holiday falls inside it, which keeps the count at 3.5.
import { expect, test, type Browser, type BrowserContext, type Page } from '@playwright/test'
import { randomUUID } from 'node:crypto'
import { latestCode, mailTo, resetInstance, setupToken, subjectsTo } from './support/env'

const H = { 'X-Requested-With': 'leavo' }
const iso = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
const plus = (d: Date, days: number) => {
  const x = new Date(d)
  x.setDate(x.getDate() + days)
  return x
}

/** Sign up through the screens; returns when the dashboard shows. */
async function signUp(page: Page, email: string, name: string) {
  await page.goto('/signup')
  await page.getByLabel('Full name').fill(name)
  await page.getByLabel('Work email').fill(email)
  const sentAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Send code' }).click()
  await page.getByLabel('6-digit code').fill(await latestCode(email, sentAt))
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page).toHaveURL(/\/dashboard$/)
}

/** Sign up through the API in a new browser context (for people who only look at data). */
async function apiSignUp(browser: Browser, email: string, name: string): Promise<BrowserContext> {
  const ctx = await browser.newContext()
  const sentAt = Date.now() - 1000
  const sent = await ctx.request.post('/api/v1/auth/code', {
    headers: H,
    data: { email, purpose: 'signup', full_name: name },
  })
  expect(sent.status()).toBe(202)
  const verified = await ctx.request.post('/api/v1/auth/verify', {
    headers: H,
    data: { email, code: await latestCode(email, sentAt), purpose: 'signup' },
  })
  expect(verified.status()).toBe(200)
  return ctx
}

type Card = {
  leave_type: { id: string; code: string }
  leave_year: number
  allotted: number
  pending: number
  available: number
}

test.beforeAll(async () => {
  await resetInstance()
})

test('quickstart walkthrough, steps 1–11', async ({ page, browser }) => {
  test.setTimeout(300_000)
  const domain = `acme-${Date.now()}.test`
  const adminEmail = `admin@${domain}`
  const year = new Date().getFullYear()

  // ── 1. First-run setup ────────────────────────────────────────────────────────────────────
  await page.goto('/dashboard')
  await expect(page).toHaveURL(/\/setup$/)
  await page.getByLabel('Setup token').fill(setupToken())
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByLabel('Organization name').fill('Acme')
  await page.getByLabel('Time zone').selectOption('Asia/Kolkata')
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByLabel('Your full name').fill('Ada Admin')
  await page.getByLabel('Work email').fill(adminEmail)
  let sentAt = Date.now() - 1000
  await page.getByRole('button', { name: 'Send code' }).click()
  await page.getByLabel('6-digit code').fill(await latestCode(adminEmail, sentAt))
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByRole('button', { name: 'Send test email' }).click()
  await expect(page.getByRole('status')).toContainText(`Test email sent to ${adminEmail}`)
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByLabel('Holiday list').selectOption(`IN/${year}`)
  await page.getByRole('button', { name: 'Finish setup' }).click()
  await expect(page).toHaveURL(/\/admin$/)
  await expect(page.getByRole('heading', { name: 'Finish setting up' })).toBeVisible()
  // Single mode: no self-service organization registration.
  const register = await page.request.post('/api/v1/orgs', {
    headers: H,
    data: {
      ticket: 'x',
      name: 'Other',
      timezone: 'Asia/Kolkata',
      leave_year_start_month: 1,
      full_name: 'Other Admin',
    },
  })
  expect(register.status()).toBe(404)

  // ── 2. Policy ─────────────────────────────────────────────────────────────────────────────
  await page.goto('/admin/policy')
  await expect(page.getByRole('cell', { name: 'Casual Leave' })).toBeVisible()
  await expect(page.getByRole('cell', { name: 'Sick Leave' })).toBeVisible()
  await expect(page.getByRole('row').filter({ hasText: 'Maternity Leave' })).toContainText(
    'Restricted — 0 employees granted',
  )
  await page.goto('/admin/org')
  await page.getByLabel('Carry-forward cap (days)').fill('5')
  await page.getByRole('button', { name: 'Save settings' }).click()
  await expect(page.getByRole('status')).toHaveText('Settings saved.')
  await expect(page.getByRole('list', { name: 'Allowed email domains' })).toContainText(domain)

  // ── 3. Employee signup ────────────────────────────────────────────────────────────────────
  const asha = await browser.newContext()
  const ashaPage = await asha.newPage()
  await ashaPage.goto('/signup')
  await ashaPage.getByLabel('Full name').fill('Asha')
  await ashaPage.getByLabel('Work email').fill('asha@gmail.com')
  await ashaPage.getByRole('button', { name: 'Send code' }).click()
  await expect(ashaPage.getByRole('alert')).toContainText("Your company isn't on Leavo yet")
  await ashaPage.getByLabel('Work email').fill(`asha@${domain}`)
  sentAt = Date.now() - 1000
  await ashaPage.getByRole('button', { name: 'Send code' }).click()
  const real = await latestCode(`asha@${domain}`, sentAt)
  const wrong = real === '000000' ? '111111' : '000000'
  for (let i = 0; i < 5; i++) {
    await ashaPage.getByLabel('6-digit code').fill(wrong)
    await ashaPage.getByRole('button', { name: 'Sign in' }).click()
  }
  await expect(ashaPage.getByRole('alert')).toHaveText(
    'Too many wrong attempts. Try again in 15 minutes.',
  )
  await asha.close()

  const raviCtx = await browser.newContext()
  const ravi = await raviCtx.newPage()
  await signUp(ravi, `ravi@${domain}`, 'Ravi Kumar')
  const firstCards: Card[] = await (await ravi.request.get('/api/v1/me/balances')).json()
  expect(firstCards.map((c) => c.leave_type.code)).toEqual(['CL', 'SL', 'CO'])
  const cl = firstCards.find((c) => c.leave_type.code === 'CL')!
  // Pro-rated to the joining month, in half days.
  expect(cl.allotted).toBeGreaterThan(0)
  expect(cl.allotted).toBeLessThanOrEqual(12)
  expect((cl.allotted * 2) % 1).toBe(0)

  // ── 4. Approver setup ─────────────────────────────────────────────────────────────────────
  const leadSignedUpAt = Date.now()
  const leadCtx0 = await apiSignUp(browser, `lead@${domain}`, 'Lena Lead')
  await leadCtx0.close()
  const devCtx = await apiSignUp(browser, `dev@${domain}`, 'Dev Patel')
  const people = (await (await page.request.get('/api/v1/admin/users?limit=50')).json()).items as {
    id: string
    email: string
  }[]
  const idOf = (local: string) => people.find((p) => p.email === `${local}@${domain}`)!.id
  expect(
    (
      await page.request.patch(`/api/v1/admin/users/${idOf('lead')}`, {
        headers: H,
        data: { role: 'approver' },
      })
    ).status(),
  ).toBe(200)
  const group = await page.request.post('/api/v1/admin/approver-groups', {
    headers: H,
    data: { name: 'Engineering', member_ids: [idOf('lead')] },
  })
  expect(group.status()).toBe(201)
  const groupId = (await group.json()).id
  expect(
    (
      await page.request.patch(`/api/v1/admin/users/${idOf('ravi')}`, {
        headers: H,
        data: { approver_group_id: groupId },
      })
    ).status(),
  ).toBe(200)

  // ── 5. Request leave ──────────────────────────────────────────────────────────────────────
  // A Thursday → Tuesday at least a week out with no pack holiday inside it.
  const holidays = new Set<string>()
  for (const y of [year, year + 1]) {
    for (const h of await (await ravi.request.get(`/api/v1/holidays?year=${y}`)).json()) {
      holidays.add(h.date)
    }
  }
  let thu = plus(new Date(), 7)
  while (thu.getDay() !== 4) thu = plus(thu, 1)
  while ([0, 1, 4, 5].some((d) => holidays.has(iso(plus(thu, d))))) thu = plus(thu, 7)
  const tue = plus(thu, 5)

  const opened = Date.now()
  await ravi.goto('/leave/apply')
  await ravi.getByLabel('Leave type').selectOption(cl.leave_type.id)
  await ravi.getByLabel('From', { exact: true }).fill(iso(thu))
  await ravi.getByLabel('To', { exact: true }).fill(iso(tue))
  await ravi.getByLabel('Last day').selectOption({ label: 'Second half' })
  await expect(ravi.getByText('3.5 working days')).toBeVisible()
  await expect(ravi.getByRole('list', { name: 'Not counted' }).getByText(/— Weekend$/)).toHaveCount(
    2,
  )
  const notify = ravi.getByLabel('Notify members')
  await notify.fill('le')
  await ravi.getByRole('option', { name: /Lena Lead/ }).click()
  const client = `client-${randomUUID().slice(0, 8)}@example.com`
  await notify.fill(client)
  await notify.press('Enter')
  // Sick leave over 2 days without a document cannot be sent.
  await ravi
    .getByLabel('Leave type')
    .selectOption(firstCards.find((c) => c.leave_type.code === 'SL')!.leave_type.id)
  await expect(ravi.getByText('A supporting document is required for this request.')).toBeVisible()
  await expect(ravi.getByRole('button', { name: 'Submit request' })).toBeDisabled()
  await ravi.getByLabel('Leave type').selectOption(cl.leave_type.id)
  await ravi.getByLabel('Reason').fill('Family function')
  const submittedAt = Date.now() - 1000
  await ravi.getByRole('button', { name: 'Submit request' }).click()
  await expect(ravi.getByRole('status')).toHaveText(
    'Request submitted. Your approvers have been emailed.',
  )
  // SC-001: opened, filled in and submitted within 30 s.
  expect(Date.now() - opened).toBeLessThan(30_000)
  const pending: Card[] = await (await ravi.request.get('/api/v1/me/balances')).json()
  expect(pending.find((c) => c.leave_type.code === 'CL')!.pending).toBe(3.5)
  const subject = 'Ravi Kumar requested 3.5 days of Casual Leave'
  expect(await subjectsTo(`lead@${domain}`, submittedAt, 2)).toContain(subject)
  expect(await subjectsTo(adminEmail, submittedAt)).toContain(subject)
  expect((await subjectsTo(client, submittedAt)).join(' ')).toMatch(
    /Ravi Kumar: leave on .* \(pending\)/,
  )
  // An overlapping request is refused before it is sent.
  await ravi.getByLabel('Leave type').selectOption(cl.leave_type.id)
  await ravi.getByLabel('From', { exact: true }).fill(iso(plus(thu, 1)))
  await ravi.getByLabel('To', { exact: true }).fill(iso(plus(thu, 4)))
  await expect(ravi.getByText('You already have leave on some of these dates.')).toBeVisible()
  await expect(ravi.getByRole('button', { name: 'Submit request' })).toBeDisabled()

  // ── 6. Approve from the email ─────────────────────────────────────────────────────────────
  const action = await mailTo(`lead@${domain}`, submittedAt, /requested/)
  const approveUrl = /Approve: (\S+)/.exec(action.Text)![1]
  // One sign-in code per 30 s per address: wait out the window from the lead's signup.
  await ravi.waitForTimeout(Math.max(0, leadSignedUpAt + 31_000 - Date.now()))
  const leadCtx = await browser.newContext()
  const lead = await leadCtx.newPage()
  await lead.goto(approveUrl)
  await expect(lead).toHaveURL(/\/login/)
  await lead.getByLabel('Work email').fill(`lead@${domain}`)
  sentAt = Date.now() - 1000
  await lead.getByRole('button', { name: 'Send code' }).click()
  await lead.getByLabel('6-digit code').fill(await latestCode(`lead@${domain}`, sentAt))
  await lead.getByRole('button', { name: 'Sign in' }).click()
  // SC-002: after signing in the confirmation is already open — one action approves.
  let actions = 0
  const dialog = lead.getByRole('dialog', { name: 'Approve Ravi Kumar’s request' })
  await expect(dialog).toBeVisible()
  const decidedAt = Date.now() - 1000
  await dialog.getByRole('button', { name: 'Confirm approval' }).click()
  actions++
  await expect(lead.getByRole('status')).toHaveText('Approved Ravi Kumar’s request.')
  expect(actions).toBeLessThanOrEqual(2)
  const decision = await mailTo(`ravi@${domain}`, decidedAt, /approved/)
  expect(decision.Attachments.map((a) => a.FileName)).toEqual(['leave.ics'])
  const fyi = await mailTo(client, decidedAt, /approved/)
  expect(fyi.Attachments.map((a) => a.FileName)).toEqual(['leave.ics'])
  // A late decision with the old version is refused.
  const history = (await (await ravi.request.get('/api/v1/me/leaves?limit=50')).json()).items as {
    id: string
    status: string
  }[]
  const leaveId = history.find((h) => h.status === 'approved')!.id
  const late = await page.request.post(`/api/v1/approvals/leave/${leaveId}/decide`, {
    headers: { ...H, 'Idempotency-Key': randomUUID() },
    data: { decision: 'reject', version: 1, comment: 'Too late' },
  })
  expect(late.status()).toBe(409)
  expect((await late.json()).error.code).toBe('ALREADY_DECIDED')

  // ── 7. Who's away privacy ─────────────────────────────────────────────────────────────────
  const month = iso(thu).slice(0, 7)
  type Person = { user: { full_name: string }; leave_type?: unknown; reason?: unknown }
  const raviIn = async (ctx: BrowserContext | Page) => {
    const cal = await (await ctx.request.get(`/api/v1/team/calendar?month=${month}`)).json()
    const day = cal.days.find((d: { date: string }) => d.date === iso(thu))
    return day.people.find((p: Person) => p.user.full_name === 'Ravi Kumar') as Person
  }
  const seenByDev = await raviIn(devCtx)
  expect(seenByDev).toBeDefined()
  expect(seenByDev).not.toHaveProperty('leave_type')
  expect(seenByDev).not.toHaveProperty('reason')
  const seenByLead = await raviIn(leadCtx)
  expect(seenByLead.leave_type).toBeDefined()
  expect(seenByLead.reason).toBe('Family function')

  // ── 8. Cancel the approved future leave ───────────────────────────────────────────────────
  const cancelled = await ravi.request.post(`/api/v1/leaves/${leaveId}/cancel`, {
    headers: H,
    data: {},
  })
  expect((await cancelled.json()).status).toBe('cancelled')
  const afterCancel: Card[] = await (await ravi.request.get('/api/v1/me/balances')).json()
  const clAfter = afterCancel.find((c) => c.leave_type.code === 'CL')!
  expect(clAfter.available).toBe(cl.available)
  const raviBooks = await (
    await page.request.get(`/api/v1/admin/users/${idOf('ravi')}/balances`)
  ).json()
  expect(
    raviBooks.ledger.some(
      (e: { reason: string; delta: number }) => e.reason === 'leave_reversed' && e.delta === 3.5,
    ),
  ).toBe(true)

  // ── 9. Comp-off ───────────────────────────────────────────────────────────────────────────
  let saturday = plus(new Date(), -1)
  while (saturday.getDay() !== 6) saturday = plus(saturday, -1)
  const claim = await ravi.request.post('/api/v1/comp-offs', {
    headers: { ...H, 'Idempotency-Key': randomUUID() },
    data: { dates: [{ date: iso(saturday), portion: 1 }], reason: 'Release weekend', notify: [] },
  })
  expect(claim.status()).toBe(201)
  const claimId = (await claim.json()).id
  const waiting = (await (
    await lead.request.get('/api/v1/approvals?status=pending&kind=comp_off')
  ).json()) as { id: string; version: number }[]
  const item = waiting.find((w) => w.id === claimId)!
  const approved = await lead.request.post(`/api/v1/approvals/comp_off/${item.id}/decide`, {
    headers: { ...H, 'Idempotency-Key': randomUUID() },
    data: { decision: 'approve', version: item.version },
  })
  expect(approved.status()).toBe(200)
  const comp = await (await ravi.request.get('/api/v1/comp-offs')).json()
  expect(comp.available).toBe(1)
  const expires = new Date(comp.credits[0].expires_on)
  const daysLeft = (expires.getTime() - Date.now()) / 86_400_000
  expect(daysLeft).toBeGreaterThan(80)
  expect(daysLeft).toBeLessThanOrEqual(91)
  let wednesday = plus(new Date(), -1)
  while (wednesday.getDay() !== 3) wednesday = plus(wednesday, -1)
  const weekday = await ravi.request.post('/api/v1/comp-offs', {
    headers: { ...H, 'Idempotency-Key': randomUUID() },
    data: { dates: [{ date: iso(wednesday), portion: 1 }], reason: 'Weekday', notify: [] },
  })
  expect(weekday.status()).toBe(422)
  expect((await weekday.json()).error.code).toBe('COMP_OFF_DATE_INVALID')

  // ── 10. Restricted grant ──────────────────────────────────────────────────────────────────
  const types = (await (await page.request.get('/api/v1/admin/leave-types')).json()) as {
    id: string
    code: string
  }[]
  const ml = types.find((t) => t.code === 'ML')!
  const grant = await page.request.post(`/api/v1/admin/leave-types/${ml.id}/grants`, {
    headers: H,
    data: { email: `dev@${domain}` },
  })
  expect(grant.status()).toBe(201)
  const devCards: Card[] = await (await devCtx.request.get('/api/v1/me/balances')).json()
  expect(devCards.find((c) => c.leave_type.code === 'ML')!.allotted).toBe(130)
  const raviCards: Card[] = await (await ravi.request.get('/api/v1/me/balances')).json()
  expect(raviCards.map((c) => c.leave_type.code)).not.toContain('ML')

  // ── 11. Ledger reconciliation ─────────────────────────────────────────────────────────────
  const books = await (
    await page.request.get(`/api/v1/admin/users/${idOf('ravi')}/balances`)
  ).json()
  type Entry = { leave_type: { id: string; code: string }; leave_year: number; delta: number }
  for (const card of books.balances as Card[]) {
    const rows = (books.ledger as Entry[]).filter(
      (e) =>
        e.leave_type.id === card.leave_type.id &&
        (card.leave_type.code === 'CO' || e.leave_year === card.leave_year),
    )
    const sum = rows.reduce((s, e) => s + e.delta, 0)
    expect(sum, `${card.leave_type.code} balance equals its ledger rows`).toBeCloseTo(
      card.available + card.pending,
      5,
    )
  }

  await Promise.all([raviCtx.close(), leadCtx.close(), devCtx.close()])
})
