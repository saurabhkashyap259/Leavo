// T098 — apply for leave: live day count, balances in the type list, notify picker,
// attachment hint and one idempotency key per submission (FR-016, FR-017, FR-019, FR-059).
import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { jsonResponse, mockApi, renderWithClient } from '../../test/render'
import { ApplyLeavePage } from './apply'

afterEach(() => vi.restoreAllMocks())

const card = (id: string, code: string, name: string, available: number, isCompOff = false) => ({
  leave_type: { id, code, name, color: '#3B82F6', is_comp_off: isCompOff },
  leave_year: 2026,
  allotted: 12,
  carried: 0,
  taken: 0,
  adjusted: 0,
  pending: 0,
  available,
})

const ME = {
  id: 'u-asha',
  email: 'asha@acme.test',
  full_name: 'Asha Rao',
  role: 'employee',
  department: null,
  employee_code: null,
  date_of_joining: null,
  org: {
    id: 'o',
    name: 'Acme',
    timezone: 'Asia/Kolkata',
    weekend: ['saturday', 'sunday'],
    leave_year_start_month: 1,
  },
  last_notify: [{ user_id: 'u-meera', email: 'meera@acme.test' }],
}

const PREVIEW = {
  days: 5,
  excluded: [
    { date: '2026-12-25', reason: 'holiday', name: 'Christmas' },
    { date: '2026-12-26', reason: 'weekend' },
    { date: '2026-12-27', reason: 'weekend' },
  ],
  by_leave_year: [{ leave_year: 2026, days: 5, available_after: 4 }],
  errors: [],
}

function api(overrides: Record<string, Parameters<typeof mockApi>[0][string]> = {}) {
  return mockApi({
    'GET /me': () => jsonResponse(200, ME),
    'GET /me/balances': () =>
      jsonResponse(200, [
        card('t-cl', 'CL', 'Casual Leave', 9),
        card('t-sl', 'SL', 'Sick Leave', 6),
        card('t-co', 'CO', 'Comp-off', 1.5, true),
      ]),
    'POST /leaves/preview': () => jsonResponse(200, PREVIEW),
    'GET /users/search': (_b, call) =>
      jsonResponse(
        200,
        new URLSearchParams(call.query).get('q') === 'ra'
          ? [{ id: 'u-ravi', full_name: 'Ravi Kumar', email: 'ravi@acme.test' }]
          : [],
      ),
    ...overrides,
  })
}

async function fillDates(from: string, to: string) {
  fireEvent.change(await screen.findByLabelText('From'), { target: { value: from } })
  fireEvent.change(screen.getByLabelText('To'), { target: { value: to } })
}

async function chooseType(user: ReturnType<typeof userEvent.setup>, label: string) {
  const select = await screen.findByLabelText('Leave type')
  await screen.findByRole('option', { name: label })
  await user.selectOptions(select, screen.getByRole('option', { name: label }))
}

describe('ApplyLeavePage', () => {
  it('lists leave types with what is available', async () => {
    api()
    await renderWithClient(<ApplyLeavePage />)
    expect(await screen.findByRole('option', { name: 'Casual Leave (9 available)' })).toBeVisible()
    expect(screen.getByRole('option', { name: 'Sick Leave (6 available)' })).toBeInTheDocument()
    expect(screen.getByRole('option', { name: 'Comp-off (1.5 available)' })).toBeInTheDocument()
  })

  it('shows the working-day count and the excluded dates as the dates change', async () => {
    const user = userEvent.setup()
    const mock = api()
    await renderWithClient(<ApplyLeavePage />)
    await chooseType(user, 'Casual Leave (9 available)')
    await fillDates('2026-12-21', '2026-12-28')

    expect(await screen.findByText('5 working days')).toBeInTheDocument()
    const excluded = screen.getByRole('list', { name: 'Not counted' })
    expect(within(excluded).getByText('Fri 25 Dec 2026 — Christmas')).toBeInTheDocument()
    expect(within(excluded).getByText('Sat 26 Dec 2026 — Weekend')).toBeInTheDocument()
    expect(screen.getByText('4 days left after this request')).toBeInTheDocument()
    expect(mock.last('POST /leaves/preview')?.body).toMatchObject({
      leave_type_id: 't-cl',
      from_date: '2026-12-21',
      to_date: '2026-12-28',
    })
  })

  it('shows one line per leave year when the range crosses into the next one', async () => {
    const user = userEvent.setup()
    api({
      'POST /leaves/preview': () =>
        jsonResponse(200, {
          days: 4,
          excluded: [],
          by_leave_year: [
            { leave_year: 2026, days: 3, available_after: 6 },
            { leave_year: 2027, days: 1, available_after: 11 },
          ],
          errors: [],
        }),
    })
    await renderWithClient(<ApplyLeavePage />)
    await chooseType(user, 'Casual Leave (9 available)')
    await fillDates('2026-12-29', '2027-01-02')
    expect(await screen.findByText('2026: 3 days, 6 left after')).toBeInTheDocument()
    expect(screen.getByText('2027: 1 day, 11 left after')).toBeInTheDocument()
  })

  it('suggests colleagues, accepts external emails and stops at 20 people', async () => {
    const user = userEvent.setup()
    const mock = api()
    await renderWithClient(<ApplyLeavePage />)
    const input = await screen.findByLabelText('Notify members')

    await user.type(input, 'ra')
    await user.click(await screen.findByRole('option', { name: /Ravi Kumar/ }))
    expect(screen.getByRole('button', { name: 'Remove Ravi Kumar' })).toBeInTheDocument()
    expect(mock.last('GET /users/search')?.query).toBe('q=ra')

    await user.type(input, 'not-an-email{Enter}')
    expect(screen.getByText('Enter a colleague or a valid email address.')).toBeInTheDocument()
    await user.clear(input)
    await user.type(input, 'Client@External.com{Enter}')
    expect(screen.getByRole('button', { name: 'Remove client@external.com' })).toBeInTheDocument()

    // Last used: one click adds Meera.
    await user.click(screen.getByRole('button', { name: 'Add meera@acme.test' }))
    expect(screen.getByRole('button', { name: 'Remove meera@acme.test' })).toBeInTheDocument()

    for (let i = 0; i < 17; i++) await user.type(input, `p${i}@x.com{Enter}`)
    expect(screen.getAllByRole('button', { name: /^Remove / })).toHaveLength(20)
    expect(input).toBeDisabled()
    expect(screen.getByText('You can notify at most 20 people.')).toBeInTheDocument()
  })

  it('asks for a document when the type requires one', async () => {
    const user = userEvent.setup()
    api({
      'POST /leaves/preview': () =>
        jsonResponse(200, {
          ...PREVIEW,
          days: 3,
          errors: [
            {
              code: 'ATTACHMENT_REQUIRED',
              message: 'A supporting document is required for this request.',
            },
          ],
        }),
    })
    await renderWithClient(<ApplyLeavePage />)
    await chooseType(user, 'Sick Leave (6 available)')
    await fillDates('2026-12-01', '2026-12-03')
    const hint = await screen.findByText('A supporting document is required for this request.')
    expect(hint.closest('[data-field="attachment"]')).not.toBeNull()
    expect(screen.getByRole('button', { name: 'Submit request' })).toBeDisabled()
  })

  it('uploads a document and sends its id', async () => {
    const user = userEvent.setup()
    const mock = api({
      'POST /attachments': () =>
        jsonResponse(201, {
          id: 'a-1',
          name: 'note.pdf',
          content_type: 'application/pdf',
          size: 9,
        }),
      'POST /leaves': () => jsonResponse(201, { id: 'r-1', status: 'pending', days: 5 }),
    })
    await renderWithClient(<ApplyLeavePage />)
    await chooseType(user, 'Casual Leave (9 available)')
    await fillDates('2026-12-21', '2026-12-28')
    const file = new File(['%PDF-1.4'], 'note.pdf', { type: 'application/pdf' })
    await user.upload(screen.getByLabelText('Supporting document'), file)
    expect(await screen.findByText('note.pdf')).toBeInTheDocument()
    expect(mock.last('POST /attachments')?.body).toBeInstanceOf(FormData)
    await waitFor(() =>
      expect(mock.last('POST /leaves/preview')?.body).toMatchObject({ attachment_id: 'a-1' }),
    )
  })

  it('submits once per submission, reusing the key when retried', async () => {
    const user = userEvent.setup()
    let attempts = 0
    const onSubmitted = vi.fn()
    const mock = api({
      'POST /leaves': () =>
        ++attempts === 1
          ? jsonResponse(500, { error: { code: 'INTERNAL', message: 'Something went wrong.' } })
          : jsonResponse(201, { id: 'r-1', status: 'pending', days: 5 }),
    })
    await renderWithClient(<ApplyLeavePage onSubmitted={onSubmitted} />)
    await chooseType(user, 'Casual Leave (9 available)')
    await fillDates('2026-12-21', '2026-12-28')
    await user.type(screen.getByLabelText('Reason'), 'Family trip')
    await screen.findByText('5 working days')

    const submit = screen.getByRole('button', { name: 'Submit request' })
    await user.click(submit)
    expect(await screen.findByRole('alert')).toHaveTextContent('Something went wrong')
    await user.click(submit)
    await waitFor(() => expect(onSubmitted).toHaveBeenCalledWith('r-1'))

    const posts = mock.calls.filter((c) => c.method === 'POST' && c.path === '/leaves')
    expect(posts).toHaveLength(2)
    const k1 = posts[0].headers.get('Idempotency-Key')
    expect(k1).toMatch(/^[0-9a-f-]{36}$/)
    expect(posts[1].headers.get('Idempotency-Key')).toBe(k1)
    expect(posts[1].body).toMatchObject({
      leave_type_id: 't-cl',
      from_date: '2026-12-21',
      to_date: '2026-12-28',
      reason: 'Family trip',
      start_half: null,
      end_half: null,
      notify: [],
    })
  })

  it('shows the server error for a refused request', async () => {
    const user = userEvent.setup()
    api({
      'POST /leaves': () =>
        jsonResponse(422, {
          error: { code: 'OVERLAP', message: 'You already have leave on some of these dates.' },
        }),
    })
    await renderWithClient(<ApplyLeavePage />)
    await chooseType(user, 'Casual Leave (9 available)')
    await fillDates('2026-12-21', '2026-12-28')
    await user.type(screen.getByLabelText('Reason'), 'Trip')
    await screen.findByText('5 working days')
    await user.click(screen.getByRole('button', { name: 'Submit request' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'You already have leave on some of these dates.',
    )
  })
})
