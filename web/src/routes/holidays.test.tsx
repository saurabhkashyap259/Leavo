// T160 — holiday calendar: current and next leave year tabs, past holidays dimmed, the next one
// highlighted with a countdown, and a month view (FR-033).
import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../test/render'
import { HolidaysPage } from './holidays'

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date('2026-11-24T09:00:00Z'))
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const Y2026 = [
  { id: 'h1', date: '2026-10-02', name: 'Gandhi Jayanti', kind: 'mandatory', location: null },
  { id: 'h2', date: '2026-12-06', name: 'Diwali', kind: 'mandatory', location: null },
  { id: 'h3', date: '2026-12-25', name: 'Christmas', kind: 'optional', location: 'Goa' },
]
const Y2027 = [
  { id: 'h4', date: '2027-01-26', name: 'Republic Day', kind: 'mandatory', location: null },
]

function api() {
  return mockApi({
    'GET /holidays': (_, c) => jsonResponse(200, c.query.includes('year=2027') ? Y2027 : Y2026),
  })
}

describe('HolidaysPage', () => {
  it('lists the year with past holidays dimmed and a countdown to the next', async () => {
    api()
    await renderWithClient(<HolidaysPage leaveYearStartMonth={1} />)
    const list = await screen.findByRole('list', { name: 'Holidays 2026' })
    const items = within(list).getAllByRole('listitem')
    expect(items).toHaveLength(3)
    expect(items[0]).toHaveAttribute('data-past', 'true')
    expect(items[1]).toHaveAttribute('data-past', 'false')
    expect(items[1]).toHaveAttribute('aria-current', 'date')
    expect(screen.getByText('Diwali in 12 days')).toBeInTheDocument()
    expect(within(items[2]).getByText('Optional')).toBeInTheDocument()
    expect(within(items[2]).getByText(/Goa/)).toBeInTheDocument()
  })

  it('switches to next year', async () => {
    const user = userEvent.setup()
    const { calls } = api()
    await renderWithClient(<HolidaysPage leaveYearStartMonth={1} />)
    await screen.findByRole('list', { name: 'Holidays 2026' })
    expect(screen.getByRole('tab', { name: '2026' })).toHaveAttribute('aria-selected', 'true')
    await user.click(screen.getByRole('tab', { name: '2027' }))
    const list = await screen.findByRole('list', { name: 'Holidays 2027' })
    expect(within(list).getByText('Republic Day')).toBeInTheDocument()
    expect(calls.some((c) => c.query.includes('year=2027'))).toBe(true)
  })

  it('shows a month view', async () => {
    const user = userEvent.setup()
    api()
    await renderWithClient(<HolidaysPage leaveYearStartMonth={1} />)
    await screen.findByRole('list', { name: 'Holidays 2026' })
    await user.click(screen.getByRole('button', { name: 'Month view' }))
    const december = screen.getByRole('table', { name: 'December 2026' })
    expect(within(december).getByRole('cell', { name: /6 December: Diwali/ })).toBeInTheDocument()
    expect(screen.getAllByRole('table')).toHaveLength(12)
  })
})
