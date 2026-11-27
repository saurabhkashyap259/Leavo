// T224 — accessibility (SC-016): axe finds no WCAG 2.1 A/AA violations on any route, applying
// for and approving leave work with the keyboard alone, and pages fit a 360 px wide screen.
import AxeBuilder from '@axe-core/playwright'
import { expect, test, type BrowserContext, type Page } from '@playwright/test'
import { createHash, randomBytes, randomUUID } from 'node:crypto'
import { E2E } from '../playwright.config'
import { seedOrg, sql } from './support/env'

type People = { domain: string; admin: string; lead: string; ravi: string; cl: string }

async function people(): Promise<People> {
  const { orgId, domain } = await seedOrg('A11y Co')
  const user = async (name: string, role: string, group: string | null) => {
    const id = randomUUID()
    await sql(
      `INSERT INTO users (id, org_id, email, full_name, role, approver_group_id)
       VALUES ($1, $2, $3, $4, $5::user_role, $6)`,
      [id, orgId, `${name.toLowerCase()}@${domain}`, name, role, group],
    )
    return id
  }
  const admin = await user('Admin', 'admin', null)
  const group = randomUUID()
  await sql('INSERT INTO approver_groups (id, org_id, name) VALUES ($1, $2, $3)', [
    group,
    orgId,
    'Eng',
  ])
  const lead = await user('Lena', 'approver', null)
  await sql('INSERT INTO approver_group_members (group_id, user_id, org_id) VALUES ($1, $2, $3)', [
    group,
    lead,
    orgId,
  ])
  const ravi = await user('Ravi', 'employee', group)
  const { rows } = await sql(`SELECT id FROM leave_types WHERE org_id = $1`, [orgId])
  const cl = rows[0].id as string
  await sql(
    `INSERT INTO balance_ledger (id, org_id, user_id, leave_type_id, leave_year, delta, reason)
     VALUES ($1, $2, $3, $4, $5, 12, 'allotment')`,
    [randomUUID(), orgId, ravi, cl, new Date().getFullYear()],
  )
  return { domain, admin, lead, ravi, cl }
}

/** Sign `userId` in by creating a session row directly (no email round trip). */
async function signIn(context: BrowserContext, userId: string) {
  const token = randomBytes(32).toString('base64url')
  const hash = createHash('sha256').update(token).digest()
  const { rows } = await sql('SELECT org_id FROM users WHERE id = $1', [userId])
  await sql(
    `INSERT INTO sessions (id, user_id, org_id, token_hash, keep_signed_in, expires_at, absolute_expires_at)
     VALUES ($1, $2, $3, $4, false, now() + interval '1 day', now() + interval '1 day')`,
    [randomUUID(), userId, rows[0].org_id, hash],
  )
  await context.addCookies([{ name: 'leavo_sid', value: token, url: E2E.baseURL }])
}

async function axe(page: Page, path: string) {
  await page.goto(path)
  await page.waitForLoadState('networkidle')
  await expect(page.locator('main, form, [role="main"]').first()).toBeVisible()
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  return result.violations.map(
    (v) => `${path}: ${v.id} (${v.impact}) — ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`,
  )
}

const EMPLOYEE_ROUTES = [
  '/dashboard',
  '/leave/apply',
  '/leave/history',
  '/comp-offs',
  '/holidays',
  '/team',
  '/profile',
]
const ADMIN_ROUTES = [
  '/approvals',
  '/admin',
  '/admin/employees',
  '/admin/approvers',
  '/admin/holidays',
  '/admin/policy',
  '/admin/org',
  '/admin/reports',
  '/admin/audit',
  '/admin/carry-forward',
  '/admin/import',
  '/admin/instance',
]

test('no WCAG 2.1 A/AA violations on any route', async ({ browser }) => {
  const p = await people()
  const violations: string[] = []

  const anon = await browser.newContext()
  const anonPage = await anon.newPage()
  for (const path of ['/login', '/signup']) violations.push(...(await axe(anonPage, path)))

  const emp = await browser.newContext()
  await signIn(emp, p.ravi)
  const empPage = await emp.newPage()
  for (const path of EMPLOYEE_ROUTES) violations.push(...(await axe(empPage, path)))

  const adm = await browser.newContext()
  await signIn(adm, p.admin)
  const admPage = await adm.newPage()
  for (const path of ADMIN_ROUTES) violations.push(...(await axe(admPage, path)))
  const { rows } = await sql(`SELECT id FROM users WHERE email = $1`, [`ravi@${p.domain}`])
  violations.push(...(await axe(admPage, `/admin/employees/${rows[0].id}`)))

  expect(violations, violations.join('\n')).toEqual([])
  await Promise.all([anon.close(), emp.close(), adm.close()])
})

/** Next Wednesday at least a week away, as YYYY-MM-DD. */
function nextWednesday(): string {
  const d = new Date()
  d.setDate(d.getDate() + 7)
  while (d.getDay() !== 3) d.setDate(d.getDate() + 1)
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

/** Press Tab until `target` has focus (at most 60 times): real keyboard navigation. */
async function tabTo(page: Page, target: ReturnType<Page['getByRole']>) {
  for (let i = 0; i < 60; i++) {
    if (await target.evaluate((el) => el === document.activeElement)) return
    await page.keyboard.press('Tab')
  }
  throw new Error('could not reach the element with Tab')
}

test('apply and approve with the keyboard alone', async ({ browser }) => {
  const p = await people()
  const day = nextWednesday()

  const emp = await browser.newContext()
  await signIn(emp, p.ravi)
  const page = await emp.newPage()
  await page.goto('/leave/apply')
  await expect(page.getByRole('option', { name: /Casual Leave/ })).toBeAttached()
  await tabTo(page, page.getByLabel('Leave type'))
  await page.keyboard.type('Casual') // type-ahead selects the option
  await expect(page.getByLabel('Leave type')).toHaveValue(p.cl)
  // Date fields are reached by Tab; the value is set on the focused field because native date
  // entry order differs by locale.
  await tabTo(page, page.getByLabel('From', { exact: true }))
  await page.getByLabel('From', { exact: true }).fill(day)
  await tabTo(page, page.getByLabel('To', { exact: true }))
  await page.getByLabel('To', { exact: true }).fill(day)
  await tabTo(page, page.getByLabel('Reason'))
  await page.keyboard.type('Family function')
  await expect(page.getByText('1 working day')).toBeVisible()
  await tabTo(page, page.getByRole('button', { name: 'Submit request' }))
  await page.keyboard.press('Enter')
  await expect(page.getByRole('status')).toHaveText(
    'Request submitted. Your approvers have been emailed.',
  )

  const lead = await browser.newContext()
  await signIn(lead, p.lead)
  const leadPage = await lead.newPage()
  await leadPage.goto('/approvals')
  await tabTo(leadPage, leadPage.getByRole('button', { name: 'Approve' }))
  await leadPage.keyboard.press('Enter')
  const dialog = leadPage.getByRole('dialog')
  await expect(dialog).toBeVisible()
  await tabTo(leadPage, dialog.getByRole('button', { name: 'Confirm approval' }))
  await leadPage.keyboard.press('Enter')
  await expect(leadPage.getByRole('status')).toHaveText('Approved Ravi’s request.')
  await Promise.all([emp.close(), lead.close()])
})

test('pages fit a 360 px wide screen', async ({ browser }) => {
  const p = await people()
  const context = await browser.newContext({ viewport: { width: 360, height: 740 } })
  await signIn(context, p.admin)
  const page = await context.newPage()
  const wide: string[] = []
  for (const path of [...EMPLOYEE_ROUTES, ...ADMIN_ROUTES]) {
    await page.goto(path)
    await page.waitForLoadState('networkidle')
    const width = await page.evaluate(() => document.documentElement.scrollWidth)
    if (width > 362) {
      // Name the outermost element that sticks out, so the failure says where to look.
      const culprit = await page.evaluate(() => {
        const clipped = (el: Element) => {
          for (let a = el.parentElement; a; a = a.parentElement) {
            if (getComputedStyle(a).overflowX !== 'visible')
              return a !== document.documentElement && a !== document.body
          }
          return false
        }
        const out = [...document.querySelectorAll('body *')].filter(
          (el) => el.getBoundingClientRect().right > window.innerWidth + 2 && !clipped(el),
        )
        const top = out.find((el) => !out.includes(el.parentElement as Element))
        return top ? `${top.tagName.toLowerCase()}.${[...top.classList].join('.')}` : 'unknown'
      })
      wide.push(`${path}: ${width}px (${culprit})`)
    }
  }
  expect(wide, wide.join('\n')).toEqual([])
  await context.close()
})
