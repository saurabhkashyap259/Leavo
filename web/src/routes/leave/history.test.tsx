// T134 — leave history: every column, filters, paging and a CSV link with the same filters
// (FR-028).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { HistoryPage } from './history'

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date('2026-11-24T09:00:00Z'))
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const CL = { id: 't-cl', code: 'CL', name: 'Casual Leave', color: '#3B82F6' }
const row = (id: string, from: string, extra: Record<string, unknown> = {}) => ({
  id,
  kind: 'leave',
  leave_type: CL,
  from,
  to: from,
  start_half: null,
  end_half: null,
  days: 1,
  status: 'approved',
  requested_at: '2026-11-20T09:00:00Z',
  decided_by: { id: 'u-p', full_name: 'Priya Lead' },
  decided_at: '2026-11-21T09:00:00Z',
  reason: 'Family function',
  decision_comment: 'Enjoy!',
  notify: ['Ravi K', 'client@x.com'],
  attachment: null,
  ...extra,
})

function api() {
  return mockApi({
    'GET /me/balances': () =>
      jsonResponse(200, [
        {
          leave_type: { ...CL, is_comp_off: false },
          leave_year: 2026,
          allotted: 12,
          carried: 0,
          taken: 1,
          adjusted: 0,
          pending: 0,
          available: 11,
        },
      ]),
    'GET /me/leaves': (_, call) =>
      jsonResponse(
        200,
        call.query.includes('cursor=c1')
          ? { items: [row('r3', '2026-10-01')], next_cursor: null }
          : {
              items: [
                row('r1', '2026-12-15', {
                  to: '2026-12-16',
                  end_half: 'first',
                  days: 1.5,
                  status: 'pending',
                  decided_by: null,
                  decided_at: null,
                  decision_comment: null,
                }),
                row('r2', '2026-12-01'),
              ],
              next_cursor: 'c1',
            },
      ),
  })
}

describe('HistoryPage', () => {
  it('shows every history field', async () => {
    api()
    await renderWithClient(<HistoryPage />)
    const table = await screen.findByRole('table')
    const r2 = within(table).getByText('Tue 1 Dec 2026').closest('tr')!
    for (const text of ['Casual Leave', 'Approved', 'Priya Lead', 'Family function', 'Enjoy!']) {
      expect(within(r2).getByText(text)).toBeInTheDocument()
    }
    expect(within(r2).getByText('Ravi K, client@x.com')).toBeInTheDocument()
    const r1 = within(table)
      .getByText(/Tue 15 Dec 2026/)
      .closest('tr')!
    expect(within(r1).getByText('Last day: first half')).toBeInTheDocument()
    expect(within(r1).getByText('Pending')).toBeInTheDocument()
  })

  it('filters, keeps the CSV link in step and loads more', async () => {
    const user = userEvent.setup()
    const { calls } = api()
    await renderWithClient(<HistoryPage />)
    await screen.findByRole('table')
    const csv = screen.getByRole('link', { name: 'Download CSV' })
    expect(csv).toHaveAttribute('href', '/api/v1/me/leaves.csv')

    await user.selectOptions(screen.getByLabelText('Status'), 'approved')
    await user.selectOptions(screen.getByLabelText('Year'), '2026')
    await user.selectOptions(screen.getByLabelText('Leave type'), 't-cl')
    await waitFor(() => {
      const q = calls.filter((c) => c.path === '/me/leaves').at(-1)!.query
      expect(q).toContain('status=approved')
      expect(q).toContain('year=2026')
      expect(q).toContain('type=t-cl')
    })
    expect(csv.getAttribute('href')).toBe(
      '/api/v1/me/leaves.csv?year=2026&type=t-cl&status=approved',
    )

    await user.click(await screen.findByRole('button', { name: 'Load more' }))
    expect(await screen.findByText('Thu 1 Oct 2026')).toBeInTheDocument()
    expect(calls.filter((c) => c.path === '/me/leaves').at(-1)!.query).toContain('cursor=c1')
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument()
  })
})
