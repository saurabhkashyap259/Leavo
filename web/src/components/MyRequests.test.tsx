// T138 — cancelling from "My requests": the action and its confirmation say what will happen
// for a pending request, approved leave that has not started, and leave already under way
// (FR-020).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../test/render'
import { MyRequests } from './MyRequests'

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date('2026-11-24T09:00:00Z'))
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const CL = { id: 't-cl', code: 'CL', name: 'Casual Leave', color: '#3B82F6' }

const item = (id: string, from: string, to: string, status: string, days: number) => ({
  id,
  kind: 'leave',
  leave_type: CL,
  from,
  to,
  start_half: null,
  end_half: null,
  days,
  status,
  requested_at: '2026-11-20T09:00:00Z',
  decided_by: null,
  decided_at: null,
  reason: 'Trip',
  decision_comment: null,
  notify: [],
  attachment: null,
})

const HISTORY = {
  next_cursor: null,
  items: [
    item('r-pending', '2026-12-14', '2026-12-15', 'pending', 2),
    item('r-future', '2026-12-01', '2026-12-03', 'approved', 3),
    item('r-started', '2026-11-23', '2026-11-27', 'approved', 5),
    item('r-asked', '2026-11-20', '2026-11-25', 'cancel_requested', 4),
  ],
}

function api(status = 'cancelled') {
  return mockApi({
    'GET /me/leaves': () => jsonResponse(200, HISTORY),
    'POST /leaves/r-pending/cancel': () => jsonResponse(200, { status }),
    'POST /leaves/r-future/cancel': () => jsonResponse(200, { status }),
    'POST /leaves/r-started/cancel': () => jsonResponse(200, { status }),
  })
}

const row = async (dates: RegExp) => (await screen.findByText(dates)).closest('li') as HTMLElement

describe('MyRequests cancel', () => {
  it('labels the action by what cancelling will do', async () => {
    api()
    await renderWithClient(<MyRequests />)
    expect(within(await row(/14 Dec/)).getByRole('button', { name: 'Withdraw' })).toBeVisible()
    expect(within(await row(/1 Dec/)).getByRole('button', { name: 'Cancel leave' })).toBeVisible()
    expect(
      within(await row(/23 Nov/)).getByRole('button', { name: 'Request cancellation' }),
    ).toBeVisible()
    // Already waiting for an approver: no second request.
    const asked = await row(/20 Nov/)
    expect(within(asked).queryByRole('button')).not.toBeInTheDocument()
    expect(within(asked).getByText('Cancellation requested')).toBeVisible()
  })

  it('withdraws a pending request after confirming', async () => {
    const user = userEvent.setup()
    const { last } = api()
    await renderWithClient(<MyRequests />)
    await user.click(within(await row(/14 Dec/)).getByRole('button', { name: 'Withdraw' }))
    const dialog = await screen.findByRole('dialog')
    expect(dialog).toHaveTextContent('The 2 days are released straight away.')
    await user.click(within(dialog).getByRole('button', { name: 'Withdraw request' }))
    await waitFor(() => expect(last('POST /leaves/r-pending/cancel')).toBeDefined())
    expect(last('POST /leaves/r-pending/cancel')!.body).toEqual({})
    expect(await screen.findByRole('status')).toHaveTextContent('Request withdrawn.')
  })

  it('cancels approved leave that has not started, with an optional reason', async () => {
    const user = userEvent.setup()
    const { last } = api()
    await renderWithClient(<MyRequests />)
    await user.click(within(await row(/1 Dec/)).getByRole('button', { name: 'Cancel leave' }))
    const dialog = await screen.findByRole('dialog')
    expect(dialog).toHaveTextContent(
      'All 3 days go back to your balance. Your approvers and the people you kept informed are emailed.',
    )
    await user.type(within(dialog).getByLabelText('Reason (optional)'), 'Plans changed')
    await user.click(within(dialog).getByRole('button', { name: 'Cancel leave' }))
    await waitFor(() =>
      expect(last('POST /leaves/r-future/cancel')?.body).toEqual({ reason: 'Plans changed' }),
    )
    expect(await screen.findByRole('status')).toHaveTextContent('Leave cancelled.')
  })

  it('asks an approver to accept when the leave has started', async () => {
    const user = userEvent.setup()
    const { last } = api('cancel_requested')
    await renderWithClient(<MyRequests />)
    await user.click(
      within(await row(/23 Nov/)).getByRole('button', { name: 'Request cancellation' }),
    )
    const dialog = await screen.findByRole('dialog')
    expect(dialog).toHaveTextContent(
      'This leave has started, so an approver must accept. Days from Tue 24 Nov onward go back to your balance; earlier days stay taken.',
    )
    await user.click(within(dialog).getByRole('button', { name: 'Send request' }))
    await waitFor(() => expect(last('POST /leaves/r-started/cancel')).toBeDefined())
    expect(await screen.findByRole('status')).toHaveTextContent(
      'Cancellation sent to your approvers.',
    )
  })
})
