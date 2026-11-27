// T075 — first-run setup wizard: token → organization → admin → test email → defaults.
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithRouter } from '../../test/render'
import { SetupWizard } from './index'

const PACKS = [
  { id: 'IN/2026', name: 'India — national (2026)', count: 17 },
  { id: 'IN/2027', name: 'India — national (2027)', count: 17 },
]

afterEach(() => vi.restoreAllMocks())

function api(overrides: Record<string, (b: unknown) => Response> = {}) {
  return mockApi({
    'POST /setup/verify-token': (b) =>
      (b as { setup_token: string }).setup_token === 'good-token'
        ? new Response(null, { status: 204 })
        : jsonResponse(403, { error: { code: 'SETUP_TOKEN_INVALID', message: 'x' } }),
    'POST /auth/code': () => jsonResponse(202, { status: 'sent' }),
    'POST /setup/test-email': () => jsonResponse(200, { ok: true }),
    'POST /setup/complete': () =>
      jsonResponse(200, { user: { id: 'u1', role: 'admin', email: 'asha@acme.test' } }),
    ...overrides,
  })
}

async function passToken(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText('Setup token'), 'good-token')
  await user.click(screen.getByRole('button', { name: 'Continue' }))
  await screen.findByLabelText('Organization name')
}

async function fillOrg(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText('Organization name'), 'Acme')
  await user.click(screen.getByRole('button', { name: 'Continue' }))
  await screen.findByLabelText('Your full name')
}

async function fillAdmin(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText('Your full name'), 'Asha Rao')
  await user.type(screen.getByLabelText('Work email'), 'asha@acme.test')
  await user.click(screen.getByRole('button', { name: 'Send code' }))
  await user.type(await screen.findByLabelText('6-digit code'), '042917')
  await user.click(screen.getByRole('button', { name: 'Continue' }))
  await screen.findByRole('button', { name: 'Send test email' })
}

describe('SetupWizard', () => {
  it('rejects a wrong setup token and accepts the right one', async () => {
    api()
    const user = userEvent.setup()
    await renderWithRouter(<SetupWizard packs={PACKS} onDone={() => {}} />)
    await user.type(screen.getByLabelText('Setup token'), 'nope')
    await user.click(screen.getByRole('button', { name: 'Continue' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('The setup token is not valid.')
    await user.clear(screen.getByLabelText('Setup token'))
    await passToken(user)
    expect(screen.getByText('Step 2 of 5')).toBeInTheDocument()
  })

  it('validates the organization step', async () => {
    api()
    const user = userEvent.setup()
    await renderWithRouter(<SetupWizard packs={PACKS} onDone={() => {}} />)
    await passToken(user)
    await user.click(screen.getByRole('button', { name: 'Continue' }))
    expect(await screen.findByText('Enter the organization name.')).toBeInTheDocument()

    await user.type(screen.getByLabelText('Organization name'), 'Acme')
    for (const day of ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday'])
      await user.click(screen.getByLabelText(day))
    await user.click(screen.getByRole('button', { name: 'Continue' }))
    expect(await screen.findByText('Leave at least one working day.')).toBeInTheDocument()
    expect(screen.queryByLabelText('Your full name')).not.toBeInTheDocument()
  })

  it('validates the admin step and sends the code with the setup token', async () => {
    const mock = api()
    const user = userEvent.setup()
    await renderWithRouter(<SetupWizard packs={PACKS} onDone={() => {}} />)
    await passToken(user)
    await fillOrg(user)
    await user.type(screen.getByLabelText('Work email'), 'not-an-email')
    await user.click(screen.getByRole('button', { name: 'Send code' }))
    expect(await screen.findByText('Enter your full name.')).toBeInTheDocument()
    expect(screen.getByText('Enter a valid work email.')).toBeInTheDocument()

    await user.clear(screen.getByLabelText('Work email'))
    await fillAdmin(user)
    expect(mock.last('POST /auth/code')!.body).toEqual({
      email: 'asha@acme.test',
      purpose: 'setup',
      setup_token: 'good-token',
    })
  })

  it('shows the SMTP error from the test email', async () => {
    api({
      'POST /setup/test-email': () => jsonResponse(200, { ok: false, error: '535 auth failed' }),
    })
    const user = userEvent.setup()
    await renderWithRouter(<SetupWizard packs={PACKS} onDone={() => {}} />)
    await passToken(user)
    await fillOrg(user)
    await fillAdmin(user)
    await user.click(screen.getByRole('button', { name: 'Send test email' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('535 auth failed')
  })

  it('completes setup with everything collected', async () => {
    const mock = api()
    const onDone = vi.fn()
    const user = userEvent.setup()
    await renderWithRouter(<SetupWizard packs={PACKS} onDone={onDone} />)
    await passToken(user)
    await fillOrg(user)
    await fillAdmin(user)
    await user.click(screen.getByRole('button', { name: 'Send test email' }))
    expect(await screen.findByRole('status')).toHaveTextContent('Test email sent to asha@acme.test')
    await user.click(screen.getByRole('button', { name: 'Continue' }))

    await user.selectOptions(screen.getByLabelText('Holiday list'), 'IN/2026')
    await user.type(screen.getByLabelText('Other email domains (optional)'), 'acme.in, acme.co')
    await user.click(screen.getByRole('button', { name: 'Finish setup' }))
    await waitFor(() => expect(onDone).toHaveBeenCalled())
    const body = mock.last('POST /setup/complete')!.body as Record<string, unknown>
    expect(body).toMatchObject({
      setup_token: 'good-token',
      org: { name: 'Acme', weekend: ['sat', 'sun'], leave_year_start_month: 1 },
      admin: { full_name: 'Asha Rao', email: 'asha@acme.test', code: '042917' },
      extra_domains: ['acme.in', 'acme.co'],
      holiday_pack: 'IN/2026',
      seed_leave_types: true,
    })
    expect(typeof (body.org as { timezone: string }).timezone).toBe('string')
  })

  it('goes back to the admin step when the code was wrong', async () => {
    api({
      'POST /setup/complete': () =>
        jsonResponse(422, { error: { code: 'CODE_INVALID', message: 'x' } }),
    })
    const user = userEvent.setup()
    await renderWithRouter(<SetupWizard packs={PACKS} onDone={() => {}} />)
    await passToken(user)
    await fillOrg(user)
    await fillAdmin(user)
    await user.click(screen.getByRole('button', { name: 'Continue' }))
    await user.click(screen.getByRole('button', { name: 'Finish setup' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('That code is wrong or has expired.')
    expect(screen.getByText('Step 3 of 5')).toBeInTheDocument()
  })
})
