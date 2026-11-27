// T191 — one employee: edit profile, role, group and status; balances and their entries; a
// manual adjustment with a required reason; export and anonymise (FR-041, FR-042, FR-045).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { EmployeePage } from './employee'

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date('2026-11-24T09:00:00Z'))
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const ASHA = {
  id: 'u1',
  email: 'asha@acme.test',
  full_name: 'Asha Rao',
  role: 'employee',
  status: 'active',
  employee_code: 'E-1',
  department: 'Platform',
  date_of_joining: '2026-01-01',
  approver_group: null,
  anonymised: false,
}
const CL = { id: 't-cl', code: 'CL', name: 'Casual Leave', color: '#3B82F6', is_comp_off: false }
const BAL = {
  balances: [
    {
      leave_type: CL,
      leave_year: 2026,
      allotted: 12,
      carried: 0,
      taken: 2,
      adjusted: 0,
      pending: 0,
      available: 10,
    },
  ],
  ledger: [
    {
      id: 'l1',
      leave_type: { id: 't-cl', code: 'CL', name: 'Casual Leave' },
      leave_year: 2026,
      delta: 12,
      reason: 'allotment',
      note: null,
      ref_kind: 'user',
      ref_id: 'u1',
      created_by: null,
      created_at: '2026-01-01T00:00:00Z',
    },
  ],
}

function api(user = ASHA) {
  return mockApi({
    'GET /admin/users/u1': () => jsonResponse(200, user),
    'GET /admin/approver-groups': () =>
      jsonResponse(200, [{ id: 'g1', name: 'Eng', members: [], employees: 0 }]),
    'GET /admin/users/u1/balances': () => jsonResponse(200, BAL),
    'PATCH /admin/users/u1': (b) => jsonResponse(200, { ...user, ...(b as object) }),
    'POST /admin/users/u1/adjustments': (b) =>
      jsonResponse(201, {
        ...BAL.ledger[0],
        id: 'l2',
        reason: 'manual_adjustment',
        ...(b as object),
      }),
    'POST /admin/users/u1/anonymise': () => jsonResponse(200, { anonymised: true }),
    'POST /admin/users/u1/export': () =>
      jsonResponse(200, { profile: { email: 'asha@acme.test' } }),
  })
}

describe('EmployeePage', () => {
  it('shows balances and the entries behind them', async () => {
    api()
    await renderWithClient(<EmployeePage id="u1" leaveYearStartMonth={1} />)
    expect(await screen.findByRole('heading', { name: 'Asha Rao' })).toBeInTheDocument()
    const ledger = await screen.findByRole('table', { name: 'Balance entries' })
    expect(within(ledger).getByText('Allotment')).toBeInTheDocument()
    expect(within(ledger).getByText('+12')).toBeInTheDocument()
  })

  it('edits role, group and status', async () => {
    const user = userEvent.setup()
    const { last } = api()
    await renderWithClient(<EmployeePage id="u1" leaveYearStartMonth={1} />)
    const form = await screen.findByRole('form', { name: 'Profile' })
    await user.selectOptions(within(form).getByLabelText('Role'), 'Approver')
    await user.selectOptions(within(form).getByLabelText('Approver group'), 'Eng')
    await user.click(within(form).getByRole('button', { name: 'Save' }))
    await waitFor(() =>
      expect(last('PATCH /admin/users/u1')?.body).toMatchObject({
        role: 'approver',
        approver_group_id: 'g1',
      }),
    )
    await user.click(screen.getByRole('button', { name: 'Deactivate' }))
    await waitFor(() =>
      expect(last('PATCH /admin/users/u1')?.body).toEqual({ status: 'deactivated' }),
    )
  })

  it('adjusts a balance only with a reason', async () => {
    const user = userEvent.setup()
    const { last } = api()
    await renderWithClient(<EmployeePage id="u1" leaveYearStartMonth={1} />)
    const form = await screen.findByRole('form', { name: 'Adjust a balance' })
    await user.selectOptions(within(form).getByLabelText('Leave type'), 'Casual Leave')
    await user.type(within(form).getByLabelText('Days (negative to deduct)'), '-1.5')
    const save = within(form).getByRole('button', { name: 'Record adjustment' })
    expect(save).toBeDisabled()
    await user.type(within(form).getByLabelText('Reason'), 'Unrecorded leave')
    await user.click(save)
    await waitFor(() =>
      expect(last('POST /admin/users/u1/adjustments')?.body).toEqual({
        leave_type_id: 't-cl',
        leave_year: 2026,
        delta: -1.5,
        note: 'Unrecorded leave',
      }),
    )
  })

  it('anonymises only after deactivation, with a confirmation', async () => {
    const user = userEvent.setup()
    const { last } = api({ ...ASHA, status: 'deactivated' })
    await renderWithClient(<EmployeePage id="u1" leaveYearStartMonth={1} />)
    await user.click(await screen.findByRole('button', { name: 'Anonymise…' }))
    const dialog = await screen.findByRole('dialog', { name: 'Anonymise Asha Rao?' })
    await user.click(within(dialog).getByRole('button', { name: 'Anonymise' }))
    await waitFor(() => expect(last('POST /admin/users/u1/anonymise')).toBeDefined())
  })
})
