// Comp-offs (T156, US7): balance and next expiry, credits with their status, requests, and a
// form to claim worked weekend days or mandatory holidays (FR-030–FR-032).
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { format, parseISO } from 'date-fns'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, errorMessage, newIdempotencyKey } from '../api/client'
import {
  compOffsQuery,
  createCompOff,
  withdrawCompOff,
  type CompOffRequestItem,
  type CreditItem,
} from '../api/comp-offs'
import { NotifyPicker, type NotifyEntry } from '../components/NotifyPicker'
import { expiryKey, workedText } from '../lib/comp-off'
import { PrimaryButton } from './setup/shared'
import { inputClass } from './setup/wizard'

const long = (d: string) => format(parseISO(d), 'EEE d MMM yyyy')
const short = (d: string) => format(parseISO(d), 'EEE d MMM')

type Row = { date: string; portion: 1 | 0.5 }

function CreditsTable({ credits }: { credits: CreditItem[] }) {
  const { t } = useTranslation()
  const title = t('compOffs.credits.title')
  if (credits.length === 0) return <p className="text-gray-700">{t('compOffs.credits.none')}</p>
  return (
    <div className="overflow-x-auto">
      <table aria-label={title} className="w-full text-left text-sm">
        <thead className="text-gray-600">
          <tr>
            <th className="py-1 pr-3">{t('compOffs.credits.worked')}</th>
            <th className="py-1 pr-3">{t('compOffs.credits.days')}</th>
            <th className="py-1 pr-3">{t('compOffs.credits.status')}</th>
            <th className="py-1 pr-3">{t('compOffs.credits.expires')}</th>
            <th className="py-1 pr-3">{t('compOffs.credits.usedOn')}</th>
            <th className="py-1">{t('compOffs.credits.approvedBy')}</th>
          </tr>
        </thead>
        <tbody>
          {credits.map((c) => (
            <tr key={c.id} className="border-t border-line">
              <td className="py-1 pr-3">{long(c.worked_date)}</td>
              <td className="py-1 pr-3">{c.days}</td>
              <td className="py-1 pr-3">{t(`compOffs.status.${c.status}`)}</td>
              <td className="py-1 pr-3">{long(c.expires_on)}</td>
              <td className="py-1 pr-3">
                {c.used_on
                  .map((u) =>
                    u.from === u.to ? short(u.from) : `${short(u.from)} – ${short(u.to)}`,
                  )
                  .join(', ')}
              </td>
              <td className="py-1">{c.approved_by?.full_name ?? ''}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function Requests({ requests }: { requests: CompOffRequestItem[] }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const withdraw = useMutation({
    mutationFn: withdrawCompOff,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['comp-offs'] }),
  })
  const title = t('compOffs.requests.title')
  if (requests.length === 0) return null
  return (
    <section className="space-y-2">
      <h2 className="text-lg font-semibold">{title}</h2>
      <ul aria-label={title} className="space-y-2">
        {requests.map((r) => (
          <li
            key={r.id}
            className="flex flex-wrap items-baseline gap-2 rounded border border-line bg-white p-3"
          >
            <span className="font-medium">{workedText(r.dates, t('compOffs.halfDay'))}</span>
            <span className="text-gray-700">{t('approvals.days', { count: r.days })}</span>
            <span className="rounded bg-surface px-2 text-xs">{t(`status.${r.status}`)}</span>
            {r.decision_comment && (
              <span className="text-sm text-gray-700">“{r.decision_comment}”</span>
            )}
            {r.status === 'pending' && (
              <button
                type="button"
                className="ml-auto text-sm text-red-800 underline"
                disabled={withdraw.isPending}
                onClick={() => withdraw.mutate(r.id)}
              >
                {t('cancel.action.withdraw')}
              </button>
            )}
          </li>
        ))}
      </ul>
    </section>
  )
}

function RequestForm() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [rows, setRows] = useState<Row[]>([{ date: '', portion: 1 }])
  const [reason, setReason] = useState('')
  const [notify, setNotify] = useState<NotifyEntry[]>([])
  const [key, setKey] = useState(newIdempotencyKey)
  const [done, setDone] = useState(false)
  const mutation = useMutation({
    mutationFn: () =>
      createCompOff(
        {
          dates: rows.filter((r) => r.date).map((r) => ({ date: r.date, portion: r.portion })),
          reason: reason.trim(),
          notify: notify.map((n) => (n.user_id ? { user_id: n.user_id } : { email: n.email })),
        },
        key,
      ),
    onSuccess: () => {
      setDone(true)
      setRows([{ date: '', portion: 1 }])
      setReason('')
      setNotify([])
      setKey(newIdempotencyKey())
      void queryClient.invalidateQueries({ queryKey: ['comp-offs'] })
    },
  })
  const title = t('compOffs.form.title')
  const update = (i: number, patch: Partial<Row>) =>
    setRows(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)))
  const submit = (e: FormEvent) => {
    e.preventDefault()
    setDone(false)
    mutation.mutate()
  }
  let error: string | null = null
  if (mutation.error instanceof ApiError && mutation.error.code === 'COMP_OFF_DATE_INVALID') {
    const date = (mutation.error.details as { date?: string } | undefined)?.date
    error = date
      ? t('compOffs.form.badDate', { date: long(date), message: t('errors.COMP_OFF_DATE_INVALID') })
      : errorMessage(mutation.error, t)
  } else if (mutation.error) {
    error = errorMessage(mutation.error, t)
  }
  return (
    <form
      aria-label={title}
      onSubmit={submit}
      className="space-y-3 rounded border border-line bg-white p-4"
    >
      <h2 className="text-lg font-semibold">{title}</h2>
      <p className="text-sm text-gray-700">{t('compOffs.form.help')}</p>
      {rows.map((r, i) => (
        <div key={i} className="flex flex-wrap items-end gap-2">
          <label className="block text-sm">
            <span className="mb-1 block font-medium">{t('compOffs.form.date', { n: i + 1 })}</span>
            <input
              type="date"
              className={inputClass}
              value={r.date}
              onChange={(e) => update(i, { date: e.target.value })}
            />
          </label>
          <label className="block text-sm">
            <span className="mb-1 block font-medium">
              {t('compOffs.form.portion', { n: i + 1 })}
            </span>
            <select
              className={inputClass}
              value={r.portion}
              onChange={(e) => update(i, { portion: Number(e.target.value) === 0.5 ? 0.5 : 1 })}
            >
              <option value={1}>{t('compOffs.form.full')}</option>
              <option value={0.5}>{t('compOffs.form.half')}</option>
            </select>
          </label>
          {rows.length > 1 && (
            <button
              type="button"
              className="text-sm underline"
              onClick={() => setRows(rows.filter((_, j) => j !== i))}
            >
              {t('compOffs.form.remove')}
            </button>
          )}
        </div>
      ))}
      <button
        type="button"
        className="text-sm text-accent underline"
        onClick={() => setRows([...rows, { date: '', portion: 1 }])}
      >
        {t('compOffs.form.add')}
      </button>
      <label className="block text-sm">
        <span className="mb-1 block font-medium">{t('compOffs.form.reason')}</span>
        <textarea
          className={inputClass}
          rows={2}
          maxLength={500}
          value={reason}
          onChange={(e) => setReason(e.target.value)}
        />
      </label>
      <NotifyPicker value={notify} onChange={setNotify} />
      {error && (
        <p role="alert" className="rounded bg-red-50 p-2 text-sm text-red-800">
          {error}
        </p>
      )}
      {done && (
        <p role="status" className="rounded bg-green-50 p-2 text-sm text-green-800">
          {t('compOffs.form.done')}
        </p>
      )}
      <PrimaryButton
        type="submit"
        busy={mutation.isPending}
        disabled={!rows.some((r) => r.date) || reason.trim() === ''}
      >
        {t('compOffs.form.submit')}
      </PrimaryButton>
    </form>
  )
}

export function CompOffsPage() {
  const { t } = useTranslation()
  const summary = useQuery(compOffsQuery)
  const s = summary.data
  return (
    <section className="max-w-4xl space-y-6">
      <h1 className="text-2xl font-semibold">{t('compOffs.title')}</h1>
      {summary.isPending && <p>{t('common.loading')}</p>}
      {summary.error && <p className="text-red-700">{errorMessage(summary.error, t)}</p>}
      {s && (
        <>
          <article className="rounded border border-line bg-white p-4">
            <p className="text-4xl font-semibold">{s.available}</p>
            <p className="text-sm text-gray-600">{t('dashboard.balance.available')}</p>
            {s.next_expiry && (
              <p className="mt-2 text-sm text-amber-900">
                {t(expiryKey(s.next_expiry.days), {
                  days: s.next_expiry.days,
                  date: long(s.next_expiry.date),
                })}
              </p>
            )}
          </article>
          <section className="space-y-2">
            <h2 className="text-lg font-semibold">{t('compOffs.credits.title')}</h2>
            <CreditsTable credits={s.credits} />
          </section>
          <Requests requests={s.requests} />
        </>
      )}
      <RequestForm />
    </section>
  )
}
