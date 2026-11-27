// T191 — audit log: newest first with before/after, filters and "Load more" (FR-044).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { AuditPage } from './audit'

afterEach(() => vi.restoreAllMocks())

const item = (id: string, action: string) => ({
  id,
  actor: { id: 'u-a', full_name: 'Ada Admin' },
  action,
  entity_kind: 'user',
  entity_id: 'u1',
  before: { role: 'employee' },
  after: { role: 'approver' },
  ip: '127.0.0.1',
  created_at: '2026-11-24T09:00:00Z',
})

describe('AuditPage', () => {
  it('shows entries with what changed, filters and pages', async () => {
    const user = userEvent.setup()
    const { calls } = mockApi({
      'GET /admin/audit-log': (_, c) =>
        jsonResponse(
          200,
          c.query.includes('cursor=')
            ? { items: [item('a2', 'leave.approve')], next_cursor: null }
            : { items: [item('a1', 'user.update')], next_cursor: 'c1' },
        ),
    })
    await renderWithClient(<AuditPage />)
    const list = await screen.findByRole('list', { name: 'Audit log' })
    const first = within(list).getAllByRole('listitem')[0]
    expect(first).toHaveTextContent('user.update')
    expect(first).toHaveTextContent('Ada Admin')
    expect(first).toHaveTextContent('role: employee → approver')
    await user.click(screen.getByRole('button', { name: 'Load more' }))
    expect(await within(list).findByText('leave.approve')).toBeInTheDocument()

    await user.type(screen.getByLabelText('Action'), 'balance.adjust')
    await user.type(screen.getByLabelText('From'), '2026-11-01')
    await user.click(screen.getByRole('button', { name: 'Apply filters' }))
    await waitFor(() => {
      const q = calls.at(-1)!.query
      expect(q).toContain('action=balance.adjust')
      expect(q).toContain('from=2026-11-01')
    })
  })
})
