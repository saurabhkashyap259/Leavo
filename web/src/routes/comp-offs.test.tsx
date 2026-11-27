// T156 — comp-offs page: balance and next expiry, credits, requests, the request form with
// full and half days, and withdrawing a pending request (FR-030–FR-032).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../test/render'
import { CompOffsPage } from './comp-offs'

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date('2026-11-24T09:00:00Z'))
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const PRIYA = { id: 'u-priya', full_name: 'Priya Shah' }

const SUMMARY = {
  available: 1.5,
  next_expiry: { date: '2027-02-07', days: 0.5 },
  credits: [
    {
      id: 'c1',
      worked_date: '2026-11-07',
      days: 1,
      days_used: 1,
      days_reserved: 0,
      days_expired: 0,
      status: 'used',
      approved_by: PRIYA,
      expires_on: '2027-02-05',
      used_on: [{ leave_request_id: 'l1', from: '2026-11-30', to: '2026-11-30', days: 1 }],
      revoke_reason: null,
    },
    {
      id: 'c2',
      worked_date: '2026-11-09',
      days: 0.5,
      days_used: 0,
      days_reserved: 0,
      days_expired: 0,
      status: 'active',
      approved_by: PRIYA,
      expires_on: '2027-02-07',
      used_on: [],
      revoke_reason: null,
    },
  ],
  requests: [
    {
      id: 'r2',
      status: 'pending',
      reason: 'Release weekend',
      dates: [{ date: '2026-11-21', portion: 1 }],
      days: 1,
      requested_at: '2026-11-23T09:00:00Z',
      decided_by: null,
      decided_at: null,
      decision_comment: null,
      version: 1,
    },
    {
      id: 'r1',
      status: 'approved',
      reason: 'Diwali on-call',
      dates: [
        { date: '2026-11-07', portion: 1 },
        { date: '2026-11-09', portion: 0.5 },
      ],
      days: 1.5,
      requested_at: '2026-11-10T09:00:00Z',
      decided_by: PRIYA,
      decided_at: '2026-11-11T09:00:00Z',
      decision_comment: null,
      version: 2,
    },
  ],
}

function api(create?: Parameters<typeof mockApi>[0][string]) {
  return mockApi({
    'GET /comp-offs': () => jsonResponse(200, SUMMARY),
    'POST /comp-offs':
      create ?? (() => jsonResponse(201, { id: 'r3', status: 'pending', days: 1.5 })),
    'POST /comp-offs/r2/cancel': () => jsonResponse(200, { status: 'cancelled' }),
  })
}

describe('CompOffsPage', () => {
  it('shows the balance, the next expiry and each credit', async () => {
    api()
    await renderWithClient(<CompOffsPage />)
    expect(await screen.findByText('1.5')).toBeInTheDocument()
    expect(screen.getByText('0.5 day expires on Sun 7 Feb 2027')).toBeInTheDocument()
    const table = screen.getByRole('table', { name: 'Credits' })
    const rows = within(table).getAllByRole('row').slice(1)
    expect(rows).toHaveLength(2)
    expect(rows[0]).toHaveTextContent('Sat 7 Nov 2026')
    expect(rows[0]).toHaveTextContent('Used')
    expect(rows[0]).toHaveTextContent('Mon 30 Nov')
    expect(rows[1]).toHaveTextContent('Mon 9 Nov 2026')
    expect(rows[1]).toHaveTextContent('0.5')
    expect(rows[1]).toHaveTextContent('Active')
    expect(rows[1]).toHaveTextContent('Sun 7 Feb 2027')
    expect(rows[1]).toHaveTextContent('Priya Shah')
  })

  it('lists requests and withdraws a pending one', async () => {
    const user = userEvent.setup()
    const { last } = api()
    await renderWithClient(<CompOffsPage />)
    const list = await screen.findByRole('list', { name: 'Comp-off requests' })
    const pending = within(list).getByText('Sat 21 Nov 2026').closest('li')!
    expect(within(pending).getByText('Pending')).toBeInTheDocument()
    const approved = within(list)
      .getByText('Sat 7 Nov 2026, Mon 9 Nov 2026 (half day)')
      .closest('li')!
    expect(within(approved).queryByRole('button')).not.toBeInTheDocument()
    await user.click(within(pending).getByRole('button', { name: 'Withdraw' }))
    await waitFor(() => expect(last('POST /comp-offs/r2/cancel')).toBeDefined())
  })

  it('requests comp-off for several dates with half days', async () => {
    const user = userEvent.setup()
    const { last } = api()
    await renderWithClient(<CompOffsPage />)
    const form = await screen.findByRole('form', { name: 'Request comp-off' })
    await user.type(within(form).getByLabelText('Date worked 1'), '2026-11-21')
    await user.click(within(form).getByRole('button', { name: 'Add another date' }))
    await user.type(within(form).getByLabelText('Date worked 2'), '2026-11-22')
    await user.selectOptions(within(form).getByLabelText('Portion 2'), 'Half day')
    await user.type(within(form).getByLabelText('Reason'), 'Release weekend')
    await user.click(within(form).getByRole('button', { name: 'Request comp-off' }))
    await waitFor(() => expect(last('POST /comp-offs')).toBeDefined())
    const call = last('POST /comp-offs')!
    expect(call.body).toEqual({
      dates: [
        { date: '2026-11-21', portion: 1 },
        { date: '2026-11-22', portion: 0.5 },
      ],
      reason: 'Release weekend',
      notify: [],
    })
    expect(call.headers.get('Idempotency-Key')).toBeTruthy()
    expect(await screen.findByRole('status')).toHaveTextContent('Comp-off requested.')
  })

  it('names the date that cannot be claimed', async () => {
    const user = userEvent.setup()
    api(() =>
      jsonResponse(422, {
        error: {
          code: 'COMP_OFF_DATE_INVALID',
          message: 'x',
          details: { date: '2026-11-18' },
        },
      }),
    )
    await renderWithClient(<CompOffsPage />)
    const form = await screen.findByRole('form', { name: 'Request comp-off' })
    await user.type(within(form).getByLabelText('Date worked 1'), '2026-11-18')
    await user.type(within(form).getByLabelText('Reason'), 'Oops')
    await user.click(within(form).getByRole('button', { name: 'Request comp-off' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Wed 18 Nov 2026: comp-off can only be claimed for a past weekend or mandatory holiday not already claimed.',
    )
  })
})
