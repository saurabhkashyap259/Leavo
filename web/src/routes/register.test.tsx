// T086 — multi-org registration: verify the founder's email, then create the organization.
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithRouter } from '../test/render'
import { RegisterPage } from './register'

afterEach(() => vi.restoreAllMocks())

describe('RegisterPage', () => {
  it('verifies the email, then creates the organization with the ticket', async () => {
    const mock = mockApi({
      'POST /auth/code': () => jsonResponse(202, { status: 'sent' }),
      'POST /auth/verify': () => jsonResponse(200, { verified: true, ticket: 'tkt' }),
      'POST /orgs': () => jsonResponse(201, { org: { id: 'o1' }, user: { id: 'u1' } }),
    })
    const onDone = vi.fn()
    const user = userEvent.setup()
    await renderWithRouter(<RegisterPage onDone={onDone} />)
    await user.type(screen.getByLabelText('Work email'), 'chen@newco.test')
    await user.click(screen.getByRole('button', { name: 'Send code' }))
    await user.type(await screen.findByLabelText('6-digit code'), '123456')
    await user.click(screen.getByRole('button', { name: 'Continue' }))

    await user.type(await screen.findByLabelText('Organization name'), 'NewCo')
    await user.type(screen.getByLabelText('Your full name'), 'Chen Li')
    await user.click(screen.getByRole('button', { name: 'Create organization' }))
    await waitFor(() => expect(onDone).toHaveBeenCalled())

    expect(mock.last('POST /auth/code')!.body).toEqual({
      email: 'chen@newco.test',
      purpose: 'register_org',
    })
    expect(mock.last('POST /auth/verify')!.body).toMatchObject({
      code: '123456',
      purpose: 'register_org',
    })
    expect(mock.last('POST /orgs')!.body).toMatchObject({
      ticket: 'tkt',
      name: 'NewCo',
      full_name: 'Chen Li',
      leave_year_start_month: 1,
      weekend: ['sat', 'sun'],
    })
  })

  it('explains a taken domain', async () => {
    mockApi({
      'POST /auth/code': () => jsonResponse(409, { error: { code: 'DOMAIN_TAKEN', message: 'x' } }),
    })
    const user = userEvent.setup()
    await renderWithRouter(<RegisterPage onDone={() => {}} />)
    await user.type(screen.getByLabelText('Work email'), 'boss@acme.test')
    await user.click(screen.getByRole('button', { name: 'Send code' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('This domain is already registered.')
  })
})
