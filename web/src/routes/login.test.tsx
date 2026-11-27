// T058 — login: identical "code sent" screen, 30 s resend, lock message.
import { act, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, renderWithRouter } from '../test/render'
import { LoginPage } from './login'

afterEach(() => {
  vi.restoreAllMocks()
  vi.useRealTimers()
})

async function requestCode(email: string, fakeTimers = false) {
  const user = fakeTimers
    ? userEvent.setup({ advanceTimers: vi.advanceTimersByTime })
    : userEvent.setup()
  await user.type(screen.getByLabelText('Work email'), email)
  await user.click(screen.getByRole('button', { name: 'Send code' }))
  return user
}

describe('LoginPage', () => {
  it('shows the same "code sent" screen for any email', async () => {
    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockImplementation(async () => jsonResponse(202, { status: 'sent' }))
    const texts: string[] = []
    for (const email of ['asha@acme.test', 'nobody@acme.test']) {
      const { unmount } = await renderWithRouter(<LoginPage onSignedIn={() => {}} />)
      await requestCode(email)
      const msg = await screen.findByRole('status')
      texts.push(msg.textContent!.replace(email, '<email>'))
      unmount()
    }
    expect(texts[0]).toBe(texts[1])
    expect(texts[0]).toContain('<email>')
    expect(JSON.parse(fetchMock.mock.calls[0][1]!.body as string)).toEqual({
      email: 'asha@acme.test',
      purpose: 'login',
    })
  })

  it('allows resending only after 30 seconds', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    vi.spyOn(globalThis, 'fetch').mockImplementation(async () =>
      jsonResponse(202, { status: 'sent' }),
    )
    await renderWithRouter(<LoginPage onSignedIn={() => {}} />)
    await requestCode('asha@acme.test', true)
    const resend = await screen.findByRole('button', { name: /Resend code/ })
    expect(resend).toBeDisabled()
    expect(resend).toHaveTextContent('30')
    await act(async () => {
      vi.advanceTimersByTime(30_000)
    })
    await waitFor(() => expect(screen.getByRole('button', { name: 'Resend code' })).toBeEnabled())
  })

  it('verifies the code and reports sign-in', async () => {
    const onSignedIn = vi.fn()
    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(jsonResponse(202, { status: 'sent' }))
      .mockResolvedValueOnce(jsonResponse(200, { user: { id: 'u1' } }))
    await renderWithRouter(<LoginPage onSignedIn={onSignedIn} />)
    const user = await requestCode('asha@acme.test')
    await user.type(await screen.findByLabelText('6-digit code'), '042917')
    await user.click(screen.getByLabelText('Keep me signed in on this device'))
    await user.click(screen.getByRole('button', { name: 'Sign in' }))
    await waitFor(() => expect(onSignedIn).toHaveBeenCalled())
    expect(JSON.parse(fetchMock.mock.calls[1][1]!.body as string)).toEqual({
      email: 'asha@acme.test',
      code: '042917',
      purpose: 'login',
      keep_signed_in: true,
    })
  })

  it('shows the lock message after too many wrong codes', async () => {
    vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(jsonResponse(202, { status: 'sent' }))
      .mockResolvedValueOnce(jsonResponse(429, { error: { code: 'LOCKED', message: 'x' } }))
    await renderWithRouter(<LoginPage onSignedIn={() => {}} />)
    const user = await requestCode('asha@acme.test')
    await user.type(await screen.findByLabelText('6-digit code'), '000000')
    await user.click(screen.getByRole('button', { name: 'Sign in' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Too many wrong attempts. Try again in 15 minutes.',
    )
  })

  it('shows a wrong-code message', async () => {
    vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(jsonResponse(202, { status: 'sent' }))
      .mockResolvedValueOnce(jsonResponse(422, { error: { code: 'CODE_INVALID', message: 'x' } }))
    await renderWithRouter(<LoginPage onSignedIn={() => {}} />)
    const user = await requestCode('asha@acme.test')
    await user.type(await screen.findByLabelText('6-digit code'), '000000')
    await user.click(screen.getByRole('button', { name: 'Sign in' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('That code is wrong or has expired.')
  })
})
