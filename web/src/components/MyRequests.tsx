// The user's pending and upcoming requests; the full list lives on the history page. Each one
// can be withdrawn or cancelled from here (FR-020).
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { recentRequestsQuery, type HistoryItem } from '../api/leaves'
import { daySpan, todayIso } from '../lib/dates'
import { cancelKind, type CancelKind } from '../lib/cancel'
import { CancelDialog } from './CancelDialog'

const isCurrent = (r: HistoryItem, today: string) =>
  r.status === 'pending' ||
  r.status === 'cancel_requested' ||
  (r.status === 'approved' && r.to >= today)

export function MyRequests() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const recent = useQuery(recentRequestsQuery)
  const today = todayIso()
  const items = (recent.data?.items ?? []).filter((r) => isCurrent(r, today)).reverse()
  const [open, setOpen] = useState<{ item: HistoryItem; kind: CancelKind } | null>(null)
  const [done, setDone] = useState<string | null>(null)
  const title = t('dashboard.mine.title')
  return (
    <section className="space-y-3 rounded border border-line bg-white p-4">
      <header className="flex items-center justify-between">
        <h2 className="text-lg font-semibold">{title}</h2>
        <Link to="/leave/history" className="text-sm text-accent underline">
          {t('dashboard.mine.all')}
        </Link>
      </header>
      {done && (
        <p role="status" className="rounded bg-green-50 p-2 text-sm text-green-800">
          {done}
        </p>
      )}
      {recent.data && items.length === 0 && (
        <p className="text-gray-700">{t('dashboard.mine.none')}</p>
      )}
      {items.length > 0 && (
        <ul aria-label={title} className="space-y-1">
          {items.map((r) => {
            const kind = cancelKind(r, today)
            return (
              <li key={r.id} className="flex flex-wrap items-baseline gap-2">
                <span className="font-medium">{r.leave_type.name}</span>
                <span className="text-gray-700">
                  {daySpan(r.from, r.to)} · {t('approvals.days', { count: r.days })}
                </span>
                <span className="rounded bg-surface px-2 text-xs">{t(`status.${r.status}`)}</span>
                {kind && (
                  <button
                    type="button"
                    className="ml-auto text-sm text-red-800 underline"
                    onClick={() => setOpen({ item: r, kind })}
                  >
                    {t(`cancel.action.${kind}`)}
                  </button>
                )}
              </li>
            )
          })}
        </ul>
      )}
      {open && (
        <CancelDialog
          item={open.item}
          kind={open.kind}
          today={today}
          onClose={() => setOpen(null)}
          onDone={() => {
            setDone(t(`cancel.${open.kind}.done`))
            setOpen(null)
            void queryClient.invalidateQueries({ queryKey: ['me'] })
            void queryClient.invalidateQueries({ queryKey: ['away'] })
          }}
        />
      )}
    </section>
  )
}
