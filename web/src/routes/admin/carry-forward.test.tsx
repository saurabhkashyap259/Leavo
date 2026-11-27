// T173 — carry-forward preview: each employee's unused, carried and lapsed days for the year
// being closed, totals, and the CSV download (FR-039).
import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { CarryForwardPage } from './carry-forward'

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date('2026-11-24T09:00:00Z'))
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const CL = { id: 't-cl', code: 'CL', name: 'Casual Leave', color: '#3B82F6' }
const ROWS = [
  {
    user: { id: 'u1', full_name: 'Asha Rao', email: 'asha@acme.test' },
    leave_type: CL,
    unused: 8,
    carry: 5,
    lapse: 3,
  },
  {
    user: { id: 'u2', full_name: 'Ravi K', email: 'ravi@acme.test' },
    leave_type: CL,
    unused: -2,
    carry: -2,
    lapse: 0,
  },
]

describe('CarryForwardPage', () => {
  it('lists each employee with totals and a CSV link', async () => {
    mockApi({ 'GET /admin/carry-forward/preview': () => jsonResponse(200, ROWS) })
    await renderWithClient(<CarryForwardPage leaveYearStartMonth={1} carryForwardCap={5} />)
    const table = await screen.findByRole('table', { name: 'Carry forward 2026 → 2027' })
    const rows = within(table).getAllByRole('row')
    expect(rows[1]).toHaveTextContent('Asha Rao')
    expect(rows[1]).toHaveTextContent('asha@acme.test')
    expect(
      within(rows[1])
        .getAllByRole('cell')
        .map((c) => c.textContent),
    ).toEqual(['Asha Rao', 'asha@acme.test', 'Casual Leave', '8', '5', '3'])
    expect(within(rows[2]).getAllByRole('cell').at(4)).toHaveTextContent('-2')
    expect(screen.getByText('5 days carried · 3 days lapse · cap 5 days')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Download CSV' })).toHaveAttribute(
      'href',
      '/api/v1/admin/carry-forward/preview.csv?year=2026',
    )
  })

  it('can look at last year', async () => {
    const user = userEvent.setup()
    const { calls } = mockApi({ 'GET /admin/carry-forward/preview': () => jsonResponse(200, []) })
    await renderWithClient(<CarryForwardPage leaveYearStartMonth={1} carryForwardCap={5} />)
    expect(await screen.findByText('No carry-forward balances for 2026.')).toBeInTheDocument()
    await user.selectOptions(screen.getByLabelText('Leave year ending'), '2025')
    expect(await screen.findByText('No carry-forward balances for 2025.')).toBeInTheDocument()
    expect(calls.at(-1)!.query).toContain('year=2025')
  })
})
