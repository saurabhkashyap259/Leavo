// T135 — team calendar: a month grid of who is away, neutral colour unless details are
// visible, month navigation and the My team filter (FR-027, FR-058).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../test/render'
import { TeamPage } from './team'

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date('2026-11-24T09:00:00Z'))
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const CL = { id: 't-cl', code: 'CL', name: 'Casual Leave', color: '#3B82F6' }

function api() {
  return mockApi({
    'GET /team/calendar': (_, call) =>
      jsonResponse(200, {
        month: call.query.includes('2026-12') ? '2026-12' : '2026-11',
        days: call.query.includes('2026-12')
          ? []
          : [
              {
                date: '2026-11-24',
                people: [
                  { user: { id: 'u-ravi', full_name: 'Ravi K' }, half: null },
                  {
                    user: { id: 'u-meena', full_name: 'Meena' },
                    half: 'first',
                    leave_type: CL,
                    reason: 'Wedding',
                  },
                ],
              },
            ],
      }),
  })
}

describe('TeamPage', () => {
  it('shows who is away on each day of the month', async () => {
    api()
    await renderWithClient(<TeamPage />)
    expect(await screen.findByRole('heading', { name: 'November 2026' })).toBeInTheDocument()
    const day = await screen.findByRole('cell', { name: /24 November/ })
    const ravi = within(day).getByText('Ravi K').closest('li')!
    expect(within(ravi).getByTestId('away-dot')).toHaveAttribute('data-neutral', 'true')
    const meena = within(day).getByText(/Meena/).closest('li')!
    expect(within(meena).getByTestId('away-dot')).toHaveStyle({ backgroundColor: '#3B82F6' })
    expect(meena).toHaveTextContent('first half')
    expect(meena).toHaveAttribute('title', 'Casual Leave — Wedding')
    expect(
      within(screen.getByRole('cell', { name: /25 November/ })).queryAllByRole('listitem'),
    ).toHaveLength(0)
  })

  it('moves between months and filters to my team', async () => {
    const user = userEvent.setup()
    const { calls } = api()
    await renderWithClient(<TeamPage />)
    await screen.findByRole('heading', { name: 'November 2026' })
    const last = () => calls.filter((c) => c.path === '/team/calendar').at(-1)!.query
    expect(last()).toContain('month=2026-11')
    expect(last()).toContain('scope=org')

    await user.click(screen.getByRole('button', { name: 'Next month' }))
    expect(await screen.findByRole('heading', { name: 'December 2026' })).toBeInTheDocument()
    await waitFor(() => expect(last()).toContain('month=2026-12'))

    await user.click(screen.getByRole('checkbox', { name: 'My team' }))
    await waitFor(() => expect(last()).toContain('scope=team'))
  })
})
