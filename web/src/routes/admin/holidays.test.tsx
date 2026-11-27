// T165 — admin holidays: add, edit, delete, and import a pack, CSV or last year's list with a
// preview first (FR-034).
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { AdminHolidaysPage } from './holidays'

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date('2026-11-24T09:00:00Z'))
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const XMAS = { id: 'h1', date: '2026-12-25', name: 'Christmas', kind: 'mandatory', location: null }

const PREVIEW = {
  dry_run: true,
  added: [
    { date: '2027-01-26', name: 'Republic Day', kind: 'mandatory', location: null },
    { date: '2027-03-23', name: 'Holi', kind: 'mandatory', location: null },
  ],
  skipped: [{ date: '2027-08-15', name: 'Independence Day', reason: 'exists' }],
  errors: [],
}

function api(over: Parameters<typeof mockApi>[0] = {}) {
  return mockApi({
    'GET /holidays': (_, c) => jsonResponse(200, c.query.includes('year=2026') ? [XMAS] : []),
    'GET /holiday-packs': () =>
      jsonResponse(200, [{ id: 'IN/2027', name: 'India — national (2027)', count: 17 }]),
    'POST /admin/holidays': (b) =>
      jsonResponse(201, { id: 'h2', location: null, ...(b as object) }),
    'PATCH /admin/holidays/h1': (b) => jsonResponse(200, { ...XMAS, ...(b as object) }),
    'DELETE /admin/holidays/h1': () => new Response(null, { status: 204 }),
    'POST /admin/holidays/import': (_, c) =>
      jsonResponse(200, { ...PREVIEW, dry_run: c.query.includes('dry_run=true') }),
    ...over,
  })
}

describe('AdminHolidaysPage', () => {
  it('adds a holiday', async () => {
    const user = userEvent.setup()
    const { last } = api()
    await renderWithClient(<AdminHolidaysPage leaveYearStartMonth={1} />)
    const form = await screen.findByRole('form', { name: 'Add a holiday' })
    await user.type(within(form).getByLabelText('Date'), '2026-12-24')
    await user.type(within(form).getByLabelText('Name'), 'Christmas Eve')
    await user.selectOptions(within(form).getByLabelText('Kind'), 'Optional')
    await user.click(within(form).getByRole('button', { name: 'Add holiday' }))
    await waitFor(() => expect(last('POST /admin/holidays')).toBeDefined())
    expect(last('POST /admin/holidays')!.body).toEqual({
      date: '2026-12-24',
      name: 'Christmas Eve',
      kind: 'optional',
    })
    expect(await screen.findByRole('status')).toHaveTextContent(
      'Holiday added. Leave booked on that day is updated.',
    )
  })

  it('edits and deletes a holiday', async () => {
    const user = userEvent.setup()
    const { last } = api()
    await renderWithClient(<AdminHolidaysPage leaveYearStartMonth={1} />)
    const row = (await screen.findByText('Christmas')).closest('tr')!
    await user.click(within(row).getByRole('button', { name: 'Edit' }))
    const dialog = await screen.findByRole('dialog', { name: 'Edit Christmas' })
    await user.selectOptions(within(dialog).getByLabelText('Kind'), 'Optional')
    await user.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() =>
      expect(last('PATCH /admin/holidays/h1')?.body).toMatchObject({ kind: 'optional' }),
    )

    await user.click(within(row).getByRole('button', { name: 'Delete' }))
    const confirm = await screen.findByRole('dialog', { name: 'Delete Christmas?' })
    await user.click(within(confirm).getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(last('DELETE /admin/holidays/h1')).toBeDefined())
  })

  it('previews a pack before importing it', async () => {
    const user = userEvent.setup()
    const { calls } = api()
    await renderWithClient(<AdminHolidaysPage leaveYearStartMonth={1} />)
    const box = await screen.findByRole('region', { name: 'Import holidays' })
    await user.selectOptions(
      await within(box).findByLabelText('Country pack'),
      'India — national (2027)',
    )
    await user.click(within(box).getByRole('button', { name: 'Preview pack' }))
    expect(await within(box).findByText('2 to add · 1 already in the calendar')).toBeInTheDocument()
    expect(within(box).getByText(/Republic Day/)).toBeInTheDocument()
    const imports = () => calls.filter((c) => c.path === '/admin/holidays/import')
    expect(imports()[0].query).toContain('dry_run=true')
    expect(imports()[0].body).toEqual({ pack: 'IN/2027' })
    await user.click(within(box).getByRole('button', { name: 'Import 2 holidays' }))
    await waitFor(() => expect(imports()).toHaveLength(2))
    expect(imports()[1].query).toContain('dry_run=false')
  })

  it('copies last year and shows CSV errors without allowing the import', async () => {
    const user = userEvent.setup()
    const { calls } = api({
      'POST /admin/holidays/import': (body) =>
        body instanceof FormData
          ? jsonResponse(200, {
              dry_run: true,
              added: [],
              skipped: [],
              errors: [{ row: 3, code: 'INVALID_DATE', message: 'Use a real date as YYYY-MM-DD.' }],
            })
          : jsonResponse(200, PREVIEW),
    })
    await renderWithClient(<AdminHolidaysPage leaveYearStartMonth={1} />)
    const box = await screen.findByRole('region', { name: 'Import holidays' })
    await user.click(within(box).getByRole('button', { name: 'Copy 2025 into 2026' }))
    await waitFor(() =>
      expect(calls.find((c) => c.path === '/admin/holidays/import')?.body).toEqual({
        copy_from_year: 2025,
        to_year: 2026,
      }),
    )

    const file = new File(['date,name\n2027-02-30,Nope\n'], 'h.csv', { type: 'text/csv' })
    await user.upload(within(box).getByLabelText('CSV file'), file)
    await user.click(within(box).getByRole('button', { name: 'Preview file' }))
    expect(
      await within(box).findByText('Row 3: Use a real date as YYYY-MM-DD.'),
    ).toBeInTheDocument()
    expect(within(box).queryByRole('button', { name: /^Import/ })).not.toBeInTheDocument()
  })
})
