// T087 — organization settings and whitelisted domains (FR-009, FR-010, FR-062).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { OrgPage } from './org'

afterEach(() => vi.restoreAllMocks())

const ORG = {
  id: 'o1',
  name: 'Acme',
  timezone: 'Asia/Kolkata',
  weekend: ['sat', 'sun'],
  leave_year_start_month: 1,
  carry_forward_cap: 5,
  comp_off_expiry_days: 90,
}

describe('OrgPage', () => {
  it('saves settings and explains a locked leave year', async () => {
    const mock = mockApi({
      'GET /admin/org': () => jsonResponse(200, ORG),
      'GET /admin/org/domains': () => jsonResponse(200, [{ id: 'd1', domain: 'acme.test' }]),
      'PATCH /admin/org': () =>
        jsonResponse(409, { error: { code: 'LEAVE_YEAR_LOCKED', message: 'x' } }),
    })
    const user = userEvent.setup()
    await renderWithClient(<OrgPage />)
    await user.selectOptions(await screen.findByLabelText('Leave year starts in'), '4')
    await user.click(screen.getByRole('button', { name: 'Save settings' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      "The leave year start can't change once leave data exists.",
    )
    expect(mock.last('PATCH /admin/org')!.body).toMatchObject({ leave_year_start_month: 4 })
  })

  it('adds and removes domains, showing why a domain is refused', async () => {
    let domains = [{ id: 'd1', domain: 'acme.test' }]
    mockApi({
      'GET /admin/org': () => jsonResponse(200, ORG),
      'GET /admin/org/domains': () => jsonResponse(200, domains),
      'POST /admin/org/domains': (b) => {
        const d = (b as { domain: string }).domain
        if (d === 'gmail.com')
          return jsonResponse(422, { error: { code: 'PUBLIC_DOMAIN', message: 'x' } })
        domains = [...domains, { id: 'd2', domain: d }]
        return jsonResponse(201, { id: 'd2', domain: d })
      },
      'DELETE /admin/org/domains/d1': () => {
        domains = domains.filter((d) => d.id !== 'd1')
        return new Response(null, { status: 204 })
      },
    })
    const user = userEvent.setup()
    await renderWithClient(<OrgPage />)
    const list = await screen.findByRole('list', { name: 'Allowed email domains' })
    expect(within(list).getByText('acme.test')).toBeInTheDocument()

    await user.type(screen.getByLabelText('Add a domain'), 'gmail.com')
    await user.click(screen.getByRole('button', { name: 'Add domain' }))
    expect(await screen.findByText("Public email domains can't be used.")).toBeInTheDocument()

    await user.clear(screen.getByLabelText('Add a domain'))
    await user.type(screen.getByLabelText('Add a domain'), 'acme.in')
    await user.click(screen.getByRole('button', { name: 'Add domain' }))
    await waitFor(() => expect(within(list).getByText('acme.in')).toBeInTheDocument())

    await user.click(within(list).getByRole('button', { name: 'Remove acme.test' }))
    await waitFor(() => expect(within(list).queryByText('acme.test')).not.toBeInTheDocument())
  })
})
