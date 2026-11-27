// T191 — approver groups: create with members chosen from approvers and admins, edit, delete
// (FR-021).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { ApproversPage } from './approvers'

afterEach(() => vi.restoreAllMocks())

const PRIYA = { id: 'u-p', full_name: 'Priya Shah', email: 'priya@acme.test', role: 'approver' }
const GROUPS = [{ id: 'g1', name: 'Eng', members: [PRIYA], employees: 4 }]
const person = (id: string, full_name: string, role: string) => ({
  id,
  email: `${id}@acme.test`,
  full_name,
  role,
  status: 'active',
  employee_code: null,
  department: null,
  date_of_joining: null,
  approver_group: null,
  anonymised: false,
})

function api() {
  return mockApi({
    'GET /admin/approver-groups': () => jsonResponse(200, GROUPS),
    'GET /admin/users': (_, c) =>
      jsonResponse(200, {
        items: c.query.includes('role=admin')
          ? [person('u-a', 'Ada Admin', 'admin')]
          : [person('u-p', 'Priya Shah', 'approver')],
        next_cursor: null,
      }),
    'POST /admin/approver-groups': (b) =>
      jsonResponse(201, { id: 'g2', members: [], employees: 0, ...(b as object) }),
    'PATCH /admin/approver-groups/g1': (b) => jsonResponse(200, { ...GROUPS[0], ...(b as object) }),
    'DELETE /admin/approver-groups/g1': () => new Response(null, { status: 204 }),
  })
}

describe('ApproversPage', () => {
  it('lists groups with members and employee counts', async () => {
    api()
    await renderWithClient(<ApproversPage />)
    const card = (await screen.findByRole('heading', { name: 'Eng' })).closest('article')!
    expect(card).toHaveTextContent('Priya Shah')
    expect(card).toHaveTextContent('4 employees')
  })

  it('creates a group choosing approvers and admins', async () => {
    const user = userEvent.setup()
    const { last } = api()
    await renderWithClient(<ApproversPage />)
    const form = await screen.findByRole('form', { name: 'New group' })
    await user.type(within(form).getByLabelText('Name'), 'Sales')
    await user.click(await within(form).findByRole('checkbox', { name: 'Ada Admin' }))
    await user.click(within(form).getByRole('checkbox', { name: 'Priya Shah' }))
    await user.click(within(form).getByRole('button', { name: 'Create group' }))
    await waitFor(() => expect(last('POST /admin/approver-groups')).toBeDefined())
    const body = last('POST /admin/approver-groups')!.body as { name: string; member_ids: string[] }
    expect(body.name).toBe('Sales')
    expect([...body.member_ids].sort()).toEqual(['u-a', 'u-p'])
  })

  it('deletes a group after confirming', async () => {
    const user = userEvent.setup()
    const { last } = api()
    await renderWithClient(<ApproversPage />)
    const card = (await screen.findByRole('heading', { name: 'Eng' })).closest('article')!
    await user.click(within(card).getByRole('button', { name: 'Delete' }))
    const dialog = await screen.findByRole('dialog', { name: 'Delete Eng?' })
    await user.click(within(dialog).getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(last('DELETE /admin/approver-groups/g1')).toBeDefined())
  })
})
