// T116 — approvals inbox: confirm dialog, reject needs a comment, a lost race shows who
// decided, and email links pre-fill the decision (FR-023, FR-024).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../test/render'
import { ApprovalsPage } from './approvals'

afterEach(() => vi.restoreAllMocks())

const ITEM = {
  id: 'r1',
  kind: 'leave',
  requester: { id: 'u-asha', full_name: 'Asha Rao' },
  leave_type: { id: 't-cl', code: 'CL', name: 'Casual Leave', color: '#3B82F6' },
  from: '2026-11-30',
  to: '2026-12-02',
  start_half: null,
  end_half: null,
  days: 3,
  reason: 'Family function',
  requested_at: '2026-11-24T09:00:00Z',
  version: 1,
  attachment: null,
  balance: { available: 9 },
  team_away: [
    { full_name: 'Ravi K', from: '2026-12-01', to: '2026-12-01' },
    {
      full_name: 'Meena',
      from: '2026-12-02',
      to: '2026-12-02',
      leave_type: { id: 't-cl', code: 'CL', name: 'Casual Leave', color: '#3B82F6' },
      reason: 'Wedding',
    },
  ],
}

const decided = {
  status: 'approved',
  decided_by: { id: 'u-priya', full_name: 'Priya Shah' },
  decided_at: '2026-12-03T10:00:00Z',
}

function api(decide?: Parameters<typeof mockApi>[0][string]) {
  return mockApi({
    'GET /approvals': (_, c) =>
      jsonResponse(200, c.query.includes('kind=') && !c.query.includes('kind=leave') ? [] : [ITEM]),
    'POST /approvals/leave/r1/decide': decide ?? (() => jsonResponse(200, decided)),
  })
}

async function open(user: ReturnType<typeof userEvent.setup>, action: 'Approve' | 'Reject') {
  const card = (await screen.findByText('Asha Rao')).closest('article')!
  await user.click(within(card).getByRole('button', { name: action }))
  return screen.findByRole('dialog')
}

describe('ApprovalsPage', () => {
  it('lists requests with balance and who else is away', async () => {
    api()
    await renderWithClient(<ApprovalsPage />)
    const card = (await screen.findByText('Asha Rao')).closest('article')!
    expect(within(card).getByText('Casual Leave')).toBeInTheDocument()
    expect(within(card).getByText('Mon 30 Nov – Wed 2 Dec 2026 · 3 days')).toBeInTheDocument()
    expect(within(card).getByText('Family function')).toBeInTheDocument()
    expect(within(card).getByText('9 days available')).toBeInTheDocument()
    const away = within(card).getByRole('list', { name: 'Also away' })
    expect(within(away).getByText('Ravi K: Tue 1 Dec')).toBeInTheDocument()
    expect(within(away).getByText('Meena: Wed 2 Dec — Casual Leave, Wedding')).toBeInTheDocument()
  })

  it('shows an empty state', async () => {
    mockApi({ 'GET /approvals': () => jsonResponse(200, []) })
    await renderWithClient(<ApprovalsPage />)
    expect(await screen.findByText('Nothing waiting for you.')).toBeInTheDocument()
  })

  it('approves after confirming, with an idempotency key', async () => {
    const user = userEvent.setup()
    const mock = api()
    await renderWithClient(<ApprovalsPage />)
    const dialog = await open(user, 'Approve')
    expect(dialog).toHaveAccessibleName('Approve Asha Rao’s request')
    await user.click(within(dialog).getByRole('button', { name: 'Confirm approval' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    const call = mock.last('POST /approvals/leave/r1/decide')!
    expect(call.body).toEqual({ decision: 'approve', version: 1 })
    expect(call.headers.get('Idempotency-Key')).toMatch(/^[0-9a-f-]{36}$/)
    expect(await screen.findByRole('status')).toHaveTextContent('Approved Asha Rao’s request.')
  })

  it('needs a comment to reject', async () => {
    const user = userEvent.setup()
    const mock = api(() => jsonResponse(200, { ...decided, status: 'rejected' }))
    await renderWithClient(<ApprovalsPage />)
    const dialog = await open(user, 'Reject')
    const confirm = within(dialog).getByRole('button', { name: 'Confirm rejection' })
    expect(confirm).toBeDisabled()
    await user.type(within(dialog).getByLabelText('Comment (required)'), '   ')
    expect(confirm).toBeDisabled()
    await user.type(within(dialog).getByLabelText('Comment (required)'), 'Release week')
    await user.click(confirm)
    await waitFor(() =>
      expect(mock.last('POST /approvals/leave/r1/decide')?.body).toEqual({
        decision: 'reject',
        version: 1,
        comment: 'Release week',
      }),
    )
  })

  it('says who decided when someone was faster', async () => {
    const user = userEvent.setup()
    api(() =>
      jsonResponse(409, {
        error: {
          code: 'ALREADY_DECIDED',
          message: 'This request has already been decided.',
          details: decided,
        },
      }),
    )
    await renderWithClient(<ApprovalsPage />)
    const dialog = await open(user, 'Approve')
    await user.click(within(dialog).getByRole('button', { name: 'Confirm approval' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent(
      'Approved by Priya Shah on 3 Dec 2026',
    )
  })

  it('opens the decision from an email link and passes its token', async () => {
    const user = userEvent.setup()
    const mock = api()
    await renderWithClient(
      <ApprovalsPage prefill={{ request: 'r1', decision: 'approve', token: 'tok-1' }} />,
    )
    const dialog = await screen.findByRole('dialog')
    expect(dialog).toHaveAccessibleName('Approve Asha Rao’s request')
    await user.click(within(dialog).getByRole('button', { name: 'Confirm approval' }))
    await waitFor(() =>
      expect(mock.last('POST /approvals/leave/r1/decide')?.body).toEqual({
        decision: 'approve',
        version: 1,
        action_token: 'tok-1',
      }),
    )
  })

  it('explains used and expired links', async () => {
    api()
    const { unmount } = await renderWithClient(<ApprovalsPage link="used" />)
    expect(
      await screen.findByText('This link was already used. The request has been decided.'),
    ).toBeInTheDocument()
    unmount()
    await renderWithClient(<ApprovalsPage link="expired" />)
    expect(
      await screen.findByText('This link has expired. You can still decide below.'),
    ).toBeInTheDocument()
  })
})

const CANCEL = {
  ...ITEM,
  id: 'r9',
  kind: 'cancel',
  from: '2026-11-30',
  to: '2026-12-04',
  days: 5,
  version: 3,
  team_away: [],
  cancel: { requested_at: '2026-12-02T04:30:00Z', reason: 'Back early', days_returned: 3 },
}

function cancelApi() {
  return mockApi({
    'GET /approvals': (_, c) => jsonResponse(200, c.query.includes('kind=cancel') ? [CANCEL] : []),
    'POST /approvals/cancel/r9/decide': (body) =>
      jsonResponse(200, {
        ...decided,
        status: (body as { decision: string }).decision === 'approve' ? 'cancelled' : 'approved',
      }),
  })
}

describe('ApprovalsPage cancellations (US6)', () => {
  it('lists requests to cancel started leave with the days that would go back', async () => {
    cancelApi()
    await renderWithClient(<ApprovalsPage />)
    const list = await screen.findByRole('region', { name: 'Cancellation requests' })
    const card = within(list).getByText('Asha Rao').closest('article')!
    expect(within(card).getByText('Mon 30 Nov – Fri 4 Dec 2026 · 5 days')).toBeInTheDocument()
    expect(
      within(card).getByText('Asked on Wed 2 Dec to cancel: 3 days go back to the balance.'),
    ).toBeInTheDocument()
    expect(within(card).getByText('Back early')).toBeInTheDocument()
    expect(screen.queryByText('Nothing waiting for you.')).not.toBeInTheDocument()
  })

  it('accepts a cancellation', async () => {
    const user = userEvent.setup()
    const { last } = cancelApi()
    await renderWithClient(<ApprovalsPage />)
    const list = await screen.findByRole('region', { name: 'Cancellation requests' })
    await user.click(within(list).getByRole('button', { name: 'Accept cancellation' }))
    const dialog = await screen.findByRole('dialog')
    await user.click(within(dialog).getByRole('button', { name: 'Accept cancellation' }))
    await waitFor(() => expect(last('POST /approvals/cancel/r9/decide')).toBeDefined())
    expect(last('POST /approvals/cancel/r9/decide')!.body).toEqual({
      decision: 'approve',
      version: 3,
    })
    expect(await screen.findByRole('status')).toHaveTextContent("Asha Rao's cancellation accepted.")
  })

  it('keeping the leave needs a comment', async () => {
    const user = userEvent.setup()
    const { last } = cancelApi()
    await renderWithClient(<ApprovalsPage />)
    const list = await screen.findByRole('region', { name: 'Cancellation requests' })
    await user.click(within(list).getByRole('button', { name: 'Keep leave' }))
    const dialog = await screen.findByRole('dialog')
    const confirm = within(dialog).getByRole('button', { name: 'Keep leave' })
    expect(confirm).toBeDisabled()
    await user.type(within(dialog).getByLabelText('Comment (required)'), 'Release week')
    await user.click(confirm)
    await waitFor(() =>
      expect(last('POST /approvals/cancel/r9/decide')?.body).toMatchObject({
        decision: 'reject',
        comment: 'Release week',
      }),
    )
    expect(await screen.findByRole('status')).toHaveTextContent("Asha Rao's leave stays approved.")
  })
})

const COMP_OFF = {
  ...ITEM,
  id: 'r7',
  kind: 'comp_off',
  leave_type: { id: 't-co', code: 'CO', name: 'Comp-off', color: '#10B981' },
  from: '2026-11-21',
  to: '2026-11-22',
  days: 1.5,
  reason: 'Release weekend',
  team_away: [],
  dates: [
    { date: '2026-11-21', portion: 1 },
    { date: '2026-11-22', portion: 0.5 },
  ],
}

describe('ApprovalsPage comp-off requests (US7)', () => {
  it('lists worked dates and approves through the comp-off endpoint', async () => {
    const user = userEvent.setup()
    const { last } = mockApi({
      'GET /approvals': (_, c) =>
        jsonResponse(200, c.query.includes('kind=comp_off') ? [COMP_OFF] : []),
      'POST /approvals/comp_off/r7/decide': () => jsonResponse(200, decided),
    })
    await renderWithClient(<ApprovalsPage />)
    const region = await screen.findByRole('region', { name: 'Comp-off requests' })
    const card = within(region).getByText('Asha Rao').closest('article')!
    expect(
      within(card).getByText('Worked: Sat 21 Nov, Sun 22 Nov (half day) · 1.5 days'),
    ).toBeInTheDocument()
    expect(within(card).queryByText(/available/)).not.toBeInTheDocument()
    await user.click(within(card).getByRole('button', { name: 'Approve' }))
    const dialog = await screen.findByRole('dialog')
    await user.click(within(dialog).getByRole('button', { name: 'Confirm approval' }))
    await waitFor(() => expect(last('POST /approvals/comp_off/r7/decide')).toBeDefined())
  })
})
