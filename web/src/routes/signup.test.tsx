// T058 — signup: unregistered domain message, name required.
import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, renderWithRouter } from '../test/render'
import { SignupPage } from './signup'

afterEach(() => vi.restoreAllMocks())

describe('SignupPage', () => {
  it('explains when the company is not on Leavo', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      jsonResponse(422, { error: { code: 'DOMAIN_NOT_REGISTERED', message: 'x' } }),
    )
    await renderWithRouter(<SignupPage onSignedIn={() => {}} />)
    const user = userEvent.setup()
    await user.type(screen.getByLabelText('Full name'), 'Asha Rao')
    await user.type(screen.getByLabelText('Work email'), 'asha@gmail.com')
    await user.click(screen.getByRole('button', { name: 'Send code' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      "Your company isn't on Leavo yet. Ask your HR to register.",
    )
  })

  it('requires a name before sending a code', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch')
    await renderWithRouter(<SignupPage onSignedIn={() => {}} />)
    const user = userEvent.setup()
    await user.type(screen.getByLabelText('Work email'), 'asha@acme.test')
    await user.click(screen.getByRole('button', { name: 'Send code' }))
    expect(await screen.findByText('Enter your full name.')).toBeInTheDocument()
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('sends name and optional joining date, then verifies with purpose signup', async () => {
    const onSignedIn = vi.fn()
    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(jsonResponse(202, { status: 'sent' }))
      .mockResolvedValueOnce(jsonResponse(200, { user: { id: 'u1' } }))
    await renderWithRouter(<SignupPage onSignedIn={onSignedIn} />)
    const user = userEvent.setup()
    await user.type(screen.getByLabelText('Full name'), 'Asha Rao')
    await user.type(screen.getByLabelText('Work email'), 'asha@acme.test')
    await user.type(screen.getByLabelText('Date of joining (optional)'), '2026-11-03')
    await user.click(screen.getByRole('button', { name: 'Send code' }))
    expect(JSON.parse(fetchMock.mock.calls[0][1]!.body as string)).toEqual({
      email: 'asha@acme.test',
      purpose: 'signup',
      full_name: 'Asha Rao',
      date_of_joining: '2026-11-03',
    })
    await user.type(await screen.findByLabelText('6-digit code'), '123456')
    await user.click(screen.getByRole('button', { name: 'Sign in' }))
    expect(JSON.parse(fetchMock.mock.calls[1][1]!.body as string).purpose).toBe('signup')
    expect(onSignedIn).toHaveBeenCalled()
  })
})
