// T075 — leave policy: editing a quota forces a "now / next leave year" choice (FR-056).
// FR-011 — admins define leave types: an organization set up without the defaults (or one that
// needs another type) can add one from the policy page.
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { PolicyPage } from './policy'

afterEach(() => vi.restoreAllMocks())

const CL = {
  id: 't-cl',
  code: 'CL',
  name: 'Casual Leave',
  color: '#3B82F6',
  yearly_quota: 12,
  pending_yearly_quota: null,
  is_comp_off: false,
  receives_carry_forward: true,
  allow_half_day: true,
  allow_negative: false,
  negative_limit: 5,
  max_days_per_request: null,
  min_notice_days: 0,
  max_backdate_days: 0,
  attachment_required_above_days: null,
  availability: 'all',
  active: true,
  sort_order: 1,
  granted_count: 0,
}
const ML = {
  ...CL,
  id: 't-ml',
  code: 'ML',
  name: 'Maternity Leave',
  yearly_quota: 130,
  availability: 'restricted',
  granted_count: 0,
}

function api() {
  return mockApi({
    'GET /admin/leave-types': () => jsonResponse(200, [CL, ML]),
    'PATCH /admin/leave-types/t-cl': (b) => jsonResponse(200, { ...CL, ...(b as object) }),
    'GET /admin/leave-types/t-ml/grants': () => jsonResponse(200, []),
  })
}

async function openEdit(user: ReturnType<typeof userEvent.setup>, name: string) {
  const row = (await screen.findByText(name)).closest('tr')!
  await user.click(within(row).getByRole('button', { name: 'Edit' }))
  return screen.findByRole('dialog', { name: `Edit ${name}` })
}

describe('PolicyPage', () => {
  it('lists types and marks restricted ones with their grant count', async () => {
    api()
    await renderWithClient(<PolicyPage />)
    expect(await screen.findByText('Casual Leave')).toBeInTheDocument()
    expect(screen.getByText('Restricted — 0 employees granted')).toBeInTheDocument()
  })

  it('asks whether a quota change applies now or from next leave year', async () => {
    const mock = api()
    const user = userEvent.setup()
    await renderWithClient(<PolicyPage />)
    const drawer = await openEdit(user, 'Casual Leave')
    const quota = within(drawer).getByLabelText('Yearly quota (days)')
    await user.clear(quota)
    await user.type(quota, '18')
    await user.click(within(drawer).getByRole('button', { name: 'Save' }))

    const choice = await screen.findByRole('alertdialog', {
      name: 'When does the new quota apply?',
    })
    const confirm = within(choice).getByRole('button', { name: 'Apply change' })
    expect(confirm).toBeDisabled()
    expect(mock.last('PATCH /admin/leave-types/t-cl')).toBeUndefined()

    await user.click(within(choice).getByLabelText(/From now/))
    await user.click(confirm)
    await waitFor(() => expect(mock.last('PATCH /admin/leave-types/t-cl')).toBeDefined())
    expect(mock.last('PATCH /admin/leave-types/t-cl')!.body).toMatchObject({
      yearly_quota: 18,
      apply_from: 'now',
    })
  })

  it('saves other edits without asking', async () => {
    const mock = api()
    const user = userEvent.setup()
    await renderWithClient(<PolicyPage />)
    const drawer = await openEdit(user, 'Casual Leave')
    const name = within(drawer).getByLabelText('Name')
    await user.clear(name)
    await user.type(name, 'Casual')
    await user.click(within(drawer).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(mock.last('PATCH /admin/leave-types/t-cl')).toBeDefined())
    const body = mock.last('PATCH /admin/leave-types/t-cl')!.body as Record<string, unknown>
    expect(body.name).toBe('Casual')
    expect(body).not.toHaveProperty('apply_from')
    expect(body).not.toHaveProperty('yearly_quota')
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
  })

  it('explains an empty policy and adds the first leave type', async () => {
    let types: (typeof CL)[] = []
    const mock = mockApi({
      'GET /admin/leave-types': () => jsonResponse(200, types),
      'POST /admin/leave-types': () => {
        types = [CL]
        return jsonResponse(201, CL)
      },
    })
    const user = userEvent.setup()
    await renderWithClient(<PolicyPage />)
    expect(
      await screen.findByText('No leave types yet. Add one so people can request leave.'),
    ).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Add leave type' }))
    const drawer = await screen.findByRole('dialog', { name: 'Add leave type' })
    await user.type(screen.getByLabelText('Name'), 'Casual Leave')
    await user.type(screen.getByLabelText('Short code'), 'CL')
    await user.type(screen.getByLabelText('Yearly quota (days)'), '12')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    expect(await screen.findByRole('cell', { name: 'Casual Leave' })).toBeInTheDocument()
    expect(drawer).not.toBeInTheDocument()
    expect(mock.last('POST /admin/leave-types')!.body).toMatchObject({
      name: 'Casual Leave',
      code: 'CL',
      yearly_quota: 12,
      availability: 'all',
      active: true,
    })
  })

  it('shows the field the server refused', async () => {
    mockApi({
      'GET /admin/leave-types': () => jsonResponse(200, []),
      'POST /admin/leave-types': () =>
        jsonResponse(422, {
          error: { code: 'VALIDATION', message: 'x', details: { field: 'code' } },
        }),
    })
    const user = userEvent.setup()
    await renderWithClient(<PolicyPage />)
    await user.click(await screen.findByRole('button', { name: 'Add leave type' }))
    await user.type(screen.getByLabelText('Name'), 'Casual Leave')
    await user.type(screen.getByLabelText('Yearly quota (days)'), '12')
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(await screen.findByLabelText('Short code')).toHaveAccessibleDescription(/./)
  })
})
