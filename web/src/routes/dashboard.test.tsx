// T129 — employee dashboard: balance cards, who's away (neutral colour for colleagues the
// viewer may not see details of), the "My team" filter, own requests and holidays
// (FR-026, FR-027, FR-058).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../test/render'
import { DashboardPage } from './dashboard'

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date('2026-11-24T09:00:00Z'))
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const CL = { id: 't-cl', code: 'CL', name: 'Casual Leave', color: '#3B82F6' }

const ME = {
  id: 'u-asha',
  email: 'asha@acme.test',
  full_name: 'Asha',
  role: 'employee',
  department: null,
  employee_code: null,
  date_of_joining: null,
  last_notify: [],
  org: {
    id: 'o1',
    name: 'Acme',
    timezone: 'Asia/Kolkata',
    weekend: ['sat', 'sun'],
    leave_year_start_month: 1,
  },
}

const BALANCES = [
  {
    leave_type: { ...CL, is_comp_off: false },
    leave_year: 2026,
    allotted: 12,
    carried: 3,
    taken: 4,
    adjusted: 0,
    pending: 1.5,
    available: 9.5,
  },
]

const CO_CARD = {
  leave_type: { id: 't-co', code: 'CO', name: 'Comp-off', color: '#10B981', is_comp_off: true },
  leave_year: 2026,
  allotted: 0,
  carried: 0,
  taken: 1,
  adjusted: 2.5,
  pending: 0,
  available: 1.5,
  next_expiry: { date: '2027-02-07', days: 0.5 },
}

const AWAY = {
  count_today: 4,
  items: [
    { user: { id: 'u-ravi', full_name: 'Ravi K' }, from: '2026-11-24', to: '2026-11-25', days: 2 },
    {
      user: { id: 'u-meena', full_name: 'Meena' },
      from: '2026-11-24',
      to: '2026-11-24',
      days: 1,
      leave_type: CL,
      reason: 'Wedding',
    },
  ],
}

const HISTORY = {
  next_cursor: null,
  items: [
    {
      id: 'r-pending',
      kind: 'leave',
      leave_type: CL,
      from: '2026-12-14',
      to: '2026-12-15',
      start_half: null,
      end_half: null,
      days: 2,
      status: 'pending',
      requested_at: '2026-11-20T09:00:00Z',
      decided_by: null,
      decided_at: null,
      reason: 'Trip',
      decision_comment: null,
      notify: [],
      attachment: null,
    },
    {
      id: 'r-past',
      kind: 'leave',
      leave_type: CL,
      from: '2026-10-01',
      to: '2026-10-01',
      start_half: null,
      end_half: null,
      days: 1,
      status: 'approved',
      requested_at: '2026-09-20T09:00:00Z',
      decided_by: { id: 'u-p', full_name: 'Priya' },
      decided_at: '2026-09-21T09:00:00Z',
      reason: 'Errand',
      decision_comment: null,
      notify: [],
      attachment: null,
    },
  ],
}

const HOLIDAYS = [
  { id: 'h0', date: '2026-11-08', name: 'Diwali', kind: 'mandatory', location: null },
  { id: 'h1', date: '2026-12-25', name: 'Christmas', kind: 'mandatory', location: null },
  { id: 'h2', date: '2026-12-31', name: "New Year's Eve", kind: 'optional', location: null },
]
const NEXT_YEAR = [
  { id: 'h3', date: '2027-01-26', name: 'Republic Day', kind: 'mandatory', location: null },
  { id: 'h4', date: '2027-03-04', name: 'Holi', kind: 'mandatory', location: null },
]

function api() {
  return mockApi({
    'GET /me': () => jsonResponse(200, ME),
    'GET /me/balances': () => jsonResponse(200, [...BALANCES, CO_CARD]),
    'GET /away': () => jsonResponse(200, AWAY),
    'GET /me/leaves': () => jsonResponse(200, HISTORY),
    'GET /holidays': (_, call) =>
      jsonResponse(200, call.query.includes('year=2027') ? NEXT_YEAR : HOLIDAYS),
  })
}

describe('DashboardPage', () => {
  it('shows a card per leave type with its breakdown', async () => {
    api()
    await renderWithClient(<DashboardPage />)
    const card = (await screen.findByRole('heading', { name: 'Casual Leave' })).closest('article')!
    expect(within(card).getByText('9.5')).toBeInTheDocument()
    expect(within(card).getByText('12 allotted + 3 carried')).toBeInTheDocument()
    expect(within(card).getByText(/4 taken/)).toBeInTheDocument()
    expect(within(card).getByText(/1.5 pending/)).toBeInTheDocument()
  })

  it('shows the comp-off card with its next expiry instead of an allotment', async () => {
    api()
    await renderWithClient(<DashboardPage />)
    const card = (await screen.findByRole('heading', { name: 'Comp-off' })).closest('article')!
    expect(within(card).getByText('1.5')).toBeInTheDocument()
    expect(within(card).getByText('0.5 day expires on Sun 7 Feb 2027')).toBeInTheDocument()
    expect(within(card).queryByText(/allotted/)).not.toBeInTheDocument()
  })

  it("shows who's away with a neutral colour unless details are visible", async () => {
    api()
    await renderWithClient(<DashboardPage />)
    expect(await screen.findByText('4 people away today')).toBeInTheDocument()
    const list = screen.getByRole('list', { name: "Who's away" })
    const ravi = within(list).getByText('Ravi K').closest('li')!
    const meena = within(list).getByText('Meena').closest('li')!
    // Colleagues without visible details: neutral dot, no type or reason.
    expect(within(ravi).getByTestId('away-dot')).toHaveAttribute('data-neutral', 'true')
    expect(within(ravi).queryByText('Casual Leave')).not.toBeInTheDocument()
    // Visible details: the type's colour, name and the reason.
    const dot = within(meena).getByTestId('away-dot')
    expect(dot).toHaveAttribute('data-neutral', 'false')
    expect(dot).toHaveStyle({ backgroundColor: '#3B82F6' })
    expect(within(meena).getByText(/Casual Leave/)).toBeInTheDocument()
    expect(within(meena).getByText(/Wedding/)).toBeInTheDocument()
  })

  it('switches range and filters to my team', async () => {
    const user = userEvent.setup()
    const { calls } = api()
    await renderWithClient(<DashboardPage />)
    await screen.findByText('4 people away today')
    const away = () => calls.filter((c) => c.path === '/away').at(-1)!
    expect(away().query).toContain('range=today')
    expect(away().query).toContain('scope=org')

    await user.click(screen.getByRole('tab', { name: 'This week' }))
    await waitFor(() => expect(away().query).toContain('range=week'))

    await user.click(screen.getByRole('checkbox', { name: 'My team' }))
    await waitFor(() => expect(away().query).toContain('scope=team'))
    expect(away().query).toContain('range=week')
  })

  it('lists pending and upcoming requests and the next 3 holidays', async () => {
    api()
    await renderWithClient(<DashboardPage />)
    const mine = await screen.findByRole('list', { name: 'My requests' })
    expect(within(mine).getByText(/14 Dec/)).toBeInTheDocument()
    expect(within(mine).getByText('Pending')).toBeInTheDocument()
    // Past leave is in the history, not here.
    expect(within(mine).queryByText(/1 Oct/)).not.toBeInTheDocument()

    const holidays = await screen.findByRole('list', { name: 'Upcoming holidays' })
    await waitFor(() => expect(within(holidays).getAllByRole('listitem')).toHaveLength(3))
    const names = within(holidays)
      .getAllByRole('listitem')
      .map((li) => li.textContent)
    expect(names[0]).toContain('Christmas')
    expect(names[1]).toContain("New Year's Eve")
    expect(names[2]).toContain('Republic Day')
  })
})
