// T191 — "Download my data" (FR-045).
import { screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { renderWithClient } from '../test/render'
import { ProfilePage } from './profile'

describe('ProfilePage', () => {
  it('offers the personal data download', async () => {
    await renderWithClient(<ProfilePage onSignedOut={() => {}} />)
    expect(await screen.findByRole('link', { name: 'Download my data' })).toHaveAttribute(
      'href',
      '/api/v1/me/export',
    )
  })
})
