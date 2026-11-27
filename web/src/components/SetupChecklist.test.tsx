// T087 — the admin's first-run checklist.
import { screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { renderWithRouter } from '../test/render'
import { SetupChecklist } from './SetupChecklist'

describe('SetupChecklist', () => {
  it('marks finished steps and links the rest', async () => {
    await renderWithRouter(
      <SetupChecklist
        items={[
          { key: 'policy', done: true, to: '/admin/policy' },
          { key: 'domains', done: false, to: '/admin/org' },
        ]}
      />,
    )
    expect(screen.getByRole('heading', { name: 'Finish setting up' })).toBeInTheDocument()
    const done = screen.getByText('Review leave types and quotas').closest('li')!
    expect(done).toHaveTextContent('Done')
    expect(screen.getByRole('link', { name: 'Check allowed email domains' })).toHaveAttribute(
      'href',
      '/admin/org',
    )
  })

  it('disappears when everything is done', async () => {
    const { container } = await renderWithRouter(
      <SetupChecklist items={[{ key: 'policy', done: true, to: '/admin/policy' }]} />,
    )
    expect(container).toBeEmptyDOMElement()
  })
})
