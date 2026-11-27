// T191 — employees: search and filter, invite one, and invite by CSV with a preview (FR-042).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { EmployeesPage } from './employees'

afterEach(() => vi.restoreAllMocks())

const ASHA = {
  id: 'u1',
  email: 'asha@acme.test',
  full_name: 'Asha Rao',
  role: 'employee',
  status: 'active',
  employee_code: 'E-1',
  department: 'Platform',
  date_of_joining: '2026-01-01',
  approver_group: { id: 'g1', name: 'Eng' },
  anonymised: false,
}

function api() {
  return mockApi({
    'GET /admin/users': () => jsonResponse(200, { items: [ASHA], next_cursor: null }),
    'GET /admin/approver-groups': () =>
      jsonResponse(200, [{ id: 'g1', name: 'Eng', members: [], employees: 1 }]),
    'POST /admin/users': (b) =>
      jsonResponse(201, { ...ASHA, id: 'u2', status: 'invited', ...(b as object) }),
    'POST /admin/users/import': (_, c) =>
      jsonResponse(200, {
        dry_run: c.query.includes('dry_run=true'),
        to_create: [
          { row: 2, email: 'b@acme.test', full_name: 'B', role: 'employee', approver_group: 'Ops' },
        ],
        skipped: [{ row: 3, email: 'asha@acme.test', reason: 'exists' }],
        warnings: [
          { row: 2, code: 'GROUP_WILL_BE_CREATED', message: 'Group "Ops" will be created.' },
        ],
        errors: [],
      }),
  })
}

describe('EmployeesPage', () => {
  it('lists people with links and filters by search, role and status', async () => {
    const user = userEvent.setup()
    const { calls } = api()
    await renderWithClient(<EmployeesPage />)
    const row = (await screen.findByText('Asha Rao')).closest('tr')!
    expect(within(row).getByRole('link', { name: 'Asha Rao' })).toHaveAttribute(
      'href',
      '/admin/employees/u1',
    )
    expect(row).toHaveTextContent('Eng')
    await user.type(screen.getByLabelText('Search'), 'ash')
    await user.selectOptions(screen.getByLabelText('Role'), 'Approver')
    await user.selectOptions(screen.getByLabelText('Status'), 'Invited')
    await waitFor(() => {
      const q = calls.filter((c) => c.path === '/admin/users').at(-1)!.query
      expect(q).toContain('q=ash')
      expect(q).toContain('role=approver')
      expect(q).toContain('status=invited')
    })
  })

  it('invites one person', async () => {
    const user = userEvent.setup()
    const { last } = api()
    await renderWithClient(<EmployeesPage />)
    const form = await screen.findByRole('form', { name: 'Invite someone' })
    await user.type(within(form).getByLabelText('Email'), 'b@acme.test')
    await user.type(within(form).getByLabelText('Full name'), 'Bea')
    await user.selectOptions(within(form).getByLabelText('Approver group'), 'Eng')
    await user.click(within(form).getByRole('button', { name: 'Send invite' }))
    await waitFor(() => expect(last('POST /admin/users')).toBeDefined())
    expect(last('POST /admin/users')!.body).toEqual({
      email: 'b@acme.test',
      full_name: 'Bea',
      role: 'employee',
      approver_group_id: 'g1',
    })
    expect(await screen.findByRole('status')).toHaveTextContent('Invite sent to b@acme.test.')
  })

  it('previews a CSV before inviting', async () => {
    const user = userEvent.setup()
    const { calls } = api()
    await renderWithClient(<EmployeesPage />)
    const box = await screen.findByRole('region', { name: 'Invite from a CSV file' })
    await user.upload(
      within(box).getByLabelText('CSV file'),
      new File(['email,full_name\n'], 'people.csv', { type: 'text/csv' }),
    )
    await user.click(within(box).getByRole('button', { name: 'Preview' }))
    expect(await within(box).findByText('1 to invite · 1 already here')).toBeInTheDocument()
    expect(within(box).getByText('Row 2: Group "Ops" will be created.')).toBeInTheDocument()
    await user.click(within(box).getByRole('button', { name: 'Invite 1 person' }))
    await waitFor(() =>
      expect(calls.filter((c) => c.path === '/admin/users/import').at(-1)!.query).toContain(
        'dry_run=false',
      ),
    )
  })
})
