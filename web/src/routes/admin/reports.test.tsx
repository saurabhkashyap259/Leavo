// T191 — reports: days taken by type, employee or month for a leave year, with CSV (FR-044).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { ReportsPage } from './reports'

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date('2026-11-24T09:00:00Z'))
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const row = (group: string, code: string, days: number) => ({
  group,
  key: group,
  leave_type: { id: code, code, name: code === 'CL' ? 'Casual Leave' : 'Sick Leave' },
  days,
})

describe('ReportsPage', () => {
  it('groups by type, employee or month and links the CSV', async () => {
    const user = userEvent.setup()
    const { calls } = mockApi({
      'GET /admin/reports/leaves': (_, c) =>
        jsonResponse(
          200,
          c.query.includes('group_by=user')
            ? [row('Asha Rao', 'CL', 3), row('Asha Rao', 'SL', 1)]
            : [row('Casual Leave', 'CL', 3.5)],
        ),
    })
    await renderWithClient(<ReportsPage leaveYearStartMonth={1} />)
    const table = await screen.findByRole('table', { name: 'Leave taken in 2026' })
    expect(within(table).getByText('3.5')).toBeInTheDocument()
    expect(screen.getByText('Total: 3.5 days')).toBeInTheDocument()
    await user.click(screen.getByRole('tab', { name: 'By employee' }))
    await waitFor(() =>
      expect(within(screen.getByRole('table')).getAllByText('Asha Rao')).toHaveLength(2),
    )
    expect(calls.at(-1)!.query).toContain('group_by=user')
    expect(screen.getByRole('link', { name: 'Download CSV' })).toHaveAttribute(
      'href',
      '/api/v1/admin/reports/leaves.csv?group_by=user&year=2026',
    )
  })
})
