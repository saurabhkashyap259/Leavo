import { execFileSync } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { fileURLToPath } from 'node:url'
import pg from 'pg'
import { E2E } from '../../playwright.config'

/** Create an organization with a unique whitelisted domain directly (faster than the wizard). */
export async function seedOrg(name = 'E2E Co'): Promise<{ orgId: string; domain: string }> {
  const domain = `e2e-${randomUUID().slice(0, 8)}.test`
  const client = new pg.Client({ connectionString: E2E.databaseUrl })
  await client.connect()
  try {
    const orgId = randomUUID()
    await client.query('INSERT INTO organizations (id, name) VALUES ($1, $2)', [orgId, name])
    await client.query('INSERT INTO org_domains (id, org_id, domain) VALUES ($1, $2, $3)', [
      randomUUID(),
      orgId,
      domain,
    ])
    await client.query(
      `INSERT INTO leave_types (id, org_id, code, name, color, yearly_quota)
       VALUES ($1, $2, 'CL', 'Casual Leave', '#3B82F6', 12)`,
      [randomUUID(), orgId],
    )
    return { orgId, domain }
  } finally {
    await client.end()
  }
}

/** Run SQL against the E2E database (test setup only). */
export async function sql(query: string, params: unknown[] = []): Promise<pg.QueryResult> {
  const client = new pg.Client({ connectionString: E2E.databaseUrl })
  await client.connect()
  try {
    return await client.query(query, params)
  } finally {
    await client.end()
  }
}

/** Subjects of emails sent to `to` since `after`, waiting until `count` have arrived. */
export async function subjectsTo(
  to: string,
  after: number,
  count = 1,
  timeoutMs = 20_000,
): Promise<string[]> {
  const deadline = Date.now() + timeoutMs
  let subjects: string[] = []
  while (Date.now() < deadline) {
    const res = await fetch(
      `${E2E.mailpit}/api/v1/search?query=${encodeURIComponent(`to:"${to}"`)}`,
    )
    const body = (await res.json()) as { messages?: { Subject: string; Created: string }[] }
    subjects = (body.messages ?? [])
      .filter((m) => Date.parse(m.Created) >= after)
      .map((m) => m.Subject)
    if (subjects.length >= count) return subjects
    await new Promise((r) => setTimeout(r, 500))
  }
  throw new Error(`expected ${count} email(s) to ${to}, got: ${JSON.stringify(subjects)}`)
}

export type Mail = {
  Subject: string
  Text: string
  HTML: string
  Attachments: { FileName: string; ContentType: string }[]
}

/** The newest email to `to` since `after` whose subject matches, with body and attachments. */
export async function mailTo(
  to: string,
  after: number,
  subject: RegExp,
  timeoutMs = 20_000,
): Promise<Mail> {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    const res = await fetch(
      `${E2E.mailpit}/api/v1/search?query=${encodeURIComponent(`to:"${to}"`)}`,
    )
    const body = (await res.json()) as {
      messages?: { ID: string; Subject: string; Created: string }[]
    }
    const hit = (body.messages ?? []).find(
      (m) => Date.parse(m.Created) >= after && subject.test(m.Subject),
    )
    if (hit) return (await (await fetch(`${E2E.mailpit}/api/v1/message/${hit.ID}`)).json()) as Mail
    await new Promise((r) => setTimeout(r, 500))
  }
  throw new Error(`no email to ${to} matching ${subject}`)
}

/** Newest 6-digit code emailed to `to`, waiting up to `timeoutMs` for it to arrive. */
export async function latestCode(to: string, after = 0, timeoutMs = 20_000): Promise<string> {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    const res = await fetch(
      `${E2E.mailpit}/api/v1/search?query=${encodeURIComponent(`to:"${to}"`)}`,
    )
    const body = (await res.json()) as { messages?: { Subject: string; Created: string }[] }
    const fresh = (body.messages ?? []).filter((m) => Date.parse(m.Created) >= after)
    const match = fresh.map((m) => /^(\d{6}) is your Leavo code$/.exec(m.Subject)).find(Boolean)
    if (match) return match[1]
    await new Promise((r) => setTimeout(r, 500))
  }
  throw new Error(`no code email for ${to}`)
}

const SERVER_ENV = {
  DATABASE_URL: E2E.databaseUrl,
  LEAVO_BASE_URL: E2E.baseURL,
  LEAVO_SECRET_KEY: 'e2e-'.repeat(16),
  SMTP_URL: 'smtp://localhost:1025',
  EMAIL_FROM: 'Leavo <leave@leavo.test>',
}

/** Wipe the E2E database back to a fresh install (no organizations, setup pending). */
export async function resetInstance(): Promise<void> {
  const client = new pg.Client({ connectionString: E2E.databaseUrl })
  await client.connect()
  try {
    await client.query(`
      TRUNCATE organizations, login_codes, rate_limits, jobs CASCADE;
      UPDATE instance_settings SET setup_token_hash = NULL, setup_token_failed_attempts = 0,
             setup_token_locked_until = NULL, setup_completed_at = NULL;`)
  } finally {
    await client.end()
  }
}

/** A fresh setup token from the operator CLI (`leavo setup-token`, FR-007). */
export function setupToken(): string {
  const bin = fileURLToPath(new URL('../../../target/debug/leavo', import.meta.url))
  return execFileSync(bin, ['setup-token'], {
    env: { PATH: process.env.PATH ?? '', ...SERVER_ENV },
  })
    .toString()
    .trim()
}
