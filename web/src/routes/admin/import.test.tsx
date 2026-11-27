// T201 — import from Keka or a generic CSV: upload, map Keka leave types, review errors and
// warnings; Import stays disabled while errors exist (FR-043).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { ImportPage } from './import'

afterEach(() => vi.restoreAllMocks())

const TYPES = [
  { id: 't-cl', code: 'CL', name: 'Casual Leave' },
  { id: 't-sl', code: 'SL', name: 'Sick Leave' },
]

const report = (over: object = {}) => ({
  dry_run: true,
  users_to_create: [{ email: 'kiran@acme.test', full_name: 'Kiran', approver_group: null }],
  groups_to_create: [],
  balances_to_set: [
    { row: 2, email: 'asha@acme.test', leave_type_code: 'CL', leave_year: 2026, available_days: 7 },
    {
      row: 3,
      email: 'kiran@acme.test',
      leave_type_code: 'SL',
      leave_year: 2026,
      available_days: 4,
    },
  ],
  history_rows: [],
  errors: [],
  warnings: [{ file: 'balances', row: 2, code: 'USER_EXISTS', message: 'Already a user.' }],
  keka_leave_types: [
    { name: 'Casual Leave', code: 'CL' },
    { name: 'Privilege Leave', code: null },
  ],
  skipped_history: null,
  ...over,
})

const csv = (name: string) => new File(['x'], name, { type: 'text/csv' })

describe('ImportPage', () => {
  it('maps Keka leave types, blocks the import while errors exist, then applies', async () => {
    const user = userEvent.setup()
    const { calls } = mockApi({
      'GET /admin/leave-types': () => jsonResponse(200, TYPES),
      'POST /admin/import/keka': (body) => {
        const map = (body as FormData).get('leave_type_map')
        const mapped = typeof map === 'string' && map.includes('Privilege Leave')
        return jsonResponse(
          200,
          mapped
            ? report({
                keka_leave_types: [
                  { name: 'Casual Leave', code: 'CL' },
                  { name: 'Privilege Leave', code: 'SL' },
                ],
              })
            : report({
                errors: [
                  {
                    file: 'balances',
                    row: 3,
                    code: 'UNKNOWN_LEAVE_TYPE',
                    message: 'Keka leave type "Privilege Leave" is not mapped.',
                  },
                ],
              }),
        )
      },
    })
    await renderWithClient(<ImportPage leaveYearStartMonth={1} />)
    await user.upload(screen.getByLabelText('Balances file'), csv('balances.csv'))
    await user.click(screen.getByRole('button', { name: 'Preview' }))

    const errors = await screen.findByRole('table', { name: 'Errors' })
    expect(within(errors).getByText('UNKNOWN_LEAVE_TYPE')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Import' })).toBeDisabled()
    expect(screen.getByText('Fix the errors to import.')).toBeInTheDocument()

    // Map the unknown type and preview again.
    await user.selectOptions(screen.getByLabelText('Privilege Leave'), 'Sick Leave')
    await user.click(screen.getByRole('button', { name: 'Preview' }))
    await waitFor(() =>
      expect(screen.queryByRole('table', { name: 'Errors' })).not.toBeInTheDocument(),
    )
    expect(screen.getByText('1 person to create · 2 balances · 0 history rows')).toBeInTheDocument()
    expect(
      within(screen.getByRole('table', { name: 'Warnings' })).getByText('USER_EXISTS'),
    ).toBeInTheDocument()
    const last = calls.filter((c) => c.path === '/admin/import/keka').at(-1)!
    expect(JSON.parse((last.body as FormData).get('leave_type_map') as string)).toEqual({
      'Casual Leave': 'CL',
      'Privilege Leave': 'SL',
    })
    expect(last.query).toContain('dry_run=true')

    await user.click(screen.getByRole('button', { name: 'Import' }))
    await waitFor(() =>
      expect(calls.filter((c) => c.path === '/admin/import/keka').at(-1)!.query).toContain(
        'dry_run=false',
      ),
    )
    expect(await screen.findByRole('status')).toHaveTextContent(
      'Import applied: 1 person created, 2 balances set.',
    )
  })

  it('imports a generic CSV with history', async () => {
    const user = userEvent.setup()
    const { calls } = mockApi({
      'GET /admin/leave-types': () => jsonResponse(200, TYPES),
      'POST /admin/import/csv': () => jsonResponse(200, report({ keka_leave_types: null })),
    })
    await renderWithClient(<ImportPage leaveYearStartMonth={1} />)
    await user.click(screen.getByRole('tab', { name: 'Generic CSV' }))
    await user.upload(screen.getByLabelText('Balances file'), csv('b.csv'))
    await user.upload(screen.getByLabelText('History file (optional)'), csv('h.csv'))
    await user.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByText('1 person to create · 2 balances · 0 history rows')
    const body = calls.find((c) => c.path === '/admin/import/csv')!.body as FormData
    expect((body.get('balances') as File).name).toBe('b.csv')
    expect((body.get('history') as File).name).toBe('h.csv')
    expect(body.get('leave_type_map')).toBeNull()
    expect(screen.queryByLabelText('Privilege Leave')).not.toBeInTheDocument()
  })
})
