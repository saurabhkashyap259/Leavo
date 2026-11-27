// T191 — admin dashboard: who is away, what waits, upcoming holidays, the next-year holiday
// banner (FR-036) and the setup checklist (FR-044).
import { screen, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { AdminHome } from './index'

beforeEach(() => localStorage.clear())
afterEach(() => vi.restoreAllMocks())

const DASH = {
  away_today: [
    {
      user: { id: 'u1', full_name: 'Ravi K' },
      from: '2026-11-24',
      to: '2026-11-25',
      days: 2,
      leave_type: { id: 't', code: 'CL', name: 'Casual Leave', color: '#3B82F6' },
    },
  ],
  pending: 3,
  pending_over_48h: 1,
  upcoming_holidays: [
    { id: 'h', date: '2026-12-25', name: 'Christmas', kind: 'mandatory', location: null },
  ],
  next_year_holidays_missing: true,
  setup_checklist: { holidays: true, approvers: false, invite: true },
}

describe('AdminHome', () => {
  it('shows the day at a glance and the holiday reminder', async () => {
    mockApi({ 'GET /admin/dashboard': () => jsonResponse(200, DASH) })
    await renderWithClient(<AdminHome />)
    const banner = await screen.findByRole('alert')
    expect(banner).toHaveTextContent("Next year's holiday list is empty.")
    expect(within(banner).getByRole('link', { name: 'Add holidays' })).toHaveAttribute(
      'href',
      '/admin/holidays',
    )
    const away = screen.getByRole('list', { name: 'Away today' })
    expect(within(away).getByText(/Ravi K/)).toBeInTheDocument()
    expect(within(away).getByText(/Casual Leave/)).toBeInTheDocument()
    expect(screen.getByText('3 waiting for a decision')).toBeInTheDocument()
    expect(screen.getByText('1 waiting more than 48 hours')).toBeInTheDocument()
    expect(screen.getByText(/Christmas/)).toBeInTheDocument()
    // Checklist: server facts tick holidays and invites; approvers is still to do.
    expect(screen.getByRole('link', { name: 'Set up approver groups' })).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: "Add this year's holidays" })).not.toBeInTheDocument()
  })

  it('has no banner when next year is ready', async () => {
    mockApi({
      'GET /admin/dashboard': () =>
        jsonResponse(200, { ...DASH, next_year_holidays_missing: false }),
    })
    await renderWithClient(<AdminHome />)
    await screen.findByRole('list', { name: 'Away today' })
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })
})
