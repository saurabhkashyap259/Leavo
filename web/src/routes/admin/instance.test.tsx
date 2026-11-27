// T213 — Instance page: version and update check, email health, storage, migrations, jobs, and
// a test email (FR-051).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { InstancePage } from './instance'

afterEach(() => vi.restoreAllMocks())

const STATUS = {
  version: '1.2.0',
  mode: 'single',
  update_check: {
    enabled: true,
    latest_version: '1.3.0',
    last_checked_at: '2026-11-24T00:15:00Z',
    update_available: true,
  },
  email: {
    sent_24h: 42,
    failed_24h: 1,
    last_error: '550 mailbox unavailable',
    last_error_at: '2026-11-24T08:00:00Z',
  },
  jobs: { queued: 3, dead: 1, oldest_queued_seconds: 720 },
  storage: 's3',
  migrations: { applied: 9, pending: 0 },
}

describe('InstancePage', () => {
  it('shows the health of the instance', async () => {
    mockApi({ 'GET /admin/instance': () => jsonResponse(200, STATUS) })
    await renderWithClient(<InstancePage />)
    expect(await screen.findByText('Version 1.2.0')).toBeInTheDocument()
    expect(screen.getByText('Version 1.3.0 is available.')).toBeInTheDocument()
    expect(screen.getByText('42 sent, 1 failed in the last 24 hours')).toBeInTheDocument()
    expect(screen.getByText(/550 mailbox unavailable/)).toBeInTheDocument()
    expect(screen.getByText('S3-compatible bucket')).toBeInTheDocument()
    expect(screen.getByText('9 applied, 0 pending')).toBeInTheDocument()
    expect(screen.getByText('3 waiting (oldest 12 min) · 1 failed')).toBeInTheDocument()
  })

  it('hides operator details in multi-organization mode', async () => {
    const { storage: _s, migrations: _m, ...multi } = STATUS
    mockApi({
      'GET /admin/instance': () =>
        jsonResponse(200, {
          ...multi,
          mode: 'multi',
          update_check: { ...STATUS.update_check, enabled: false },
        }),
    })
    await renderWithClient(<InstancePage />)
    await screen.findByText('Version 1.2.0')
    expect(screen.queryByText('Storage')).not.toBeInTheDocument()
    expect(screen.queryByText('Migrations')).not.toBeInTheDocument()
    expect(screen.getByText('Update check is off (LEAVO_UPDATE_CHECK=false).')).toBeInTheDocument()
  })

  it('sends a test email', async () => {
    const user = userEvent.setup()
    const { last } = mockApi({
      'GET /admin/instance': () => jsonResponse(200, STATUS),
      'POST /admin/instance/test-email': () => jsonResponse(202, { to: 'admin@acme.test' }),
    })
    await renderWithClient(<InstancePage />)
    await user.click(await screen.findByRole('button', { name: 'Send a test email' }))
    await waitFor(() => expect(last('POST /admin/instance/test-email')).toBeDefined())
    expect(await screen.findByRole('status')).toHaveTextContent(
      'Test email queued to admin@acme.test.',
    )
  })
})

describe('InstancePage sign everyone out', () => {
  it('asks first, then ends every session in the organization', async () => {
    const user = userEvent.setup()
    const signedOut = vi.fn()
    const { last } = mockApi({
      'GET /admin/instance': () => jsonResponse(200, STATUS),
      'POST /admin/org/sign-out-all': () => new Response(null, { status: 204 }),
    })
    await renderWithClient(<InstancePage onSignedOut={signedOut} />)
    await user.click(await screen.findByRole('button', { name: 'Sign everyone out…' }))
    const dialog = await screen.findByRole('dialog', { name: 'Sign everyone out?' })
    expect(dialog).toHaveTextContent('including you')
    await user.click(within(dialog).getByRole('button', { name: 'Sign everyone out' }))
    await waitFor(() => expect(last('POST /admin/org/sign-out-all')).toBeDefined())
    await waitFor(() => expect(signedOut).toHaveBeenCalled())
  })
})
