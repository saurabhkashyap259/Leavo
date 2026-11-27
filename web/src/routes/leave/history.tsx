// Leave history (T134, FR-028): every request with filters by leave year, type and status,
// paged, and a CSV export of the same filters and fields.
import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { format, parseISO } from 'date-fns'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { errorMessage } from '../../api/client'
import {
  balancesQuery,
  historyPage,
  historyParams,
  type HistoryFilters,
  type HistoryItem,
} from '../../api/leaves'
import { inputClass } from '../setup/wizard'

const STATUSES = ['pending', 'approved', 'rejected', 'cancelled', 'cancel_requested'] as const

const longDate = (d: string) => format(parseISO(d), 'EEE d MMM yyyy')
const dates = (r: HistoryItem) =>
  r.from === r.to ? longDate(r.from) : `${longDate(r.from)} – ${longDate(r.to)}`

function HalfDay({ item }: { item: HistoryItem }) {
  const { t } = useTranslation()
  const name = (h: string) => t(`leave.history.halves.${h}`)
  if (item.from === item.to) {
    const h = item.start_half ?? item.end_half
    return h ? <>{t('leave.history.halfOnly', { half: name(h) })}</> : null
  }
  const parts = [
    item.start_half && t('leave.history.firstDayHalf', { half: name(item.start_half) }),
    item.end_half && t('leave.history.lastDayHalf', { half: name(item.end_half) }),
  ].filter(Boolean)
  return <>{parts.join('; ')}</>
}

export function HistoryPage() {
  const { t } = useTranslation()
  const [filters, setFilters] = useState<HistoryFilters>({ year: '', type: '', status: '' })
  const types = useQuery(balancesQuery)
  const history = useInfiniteQuery({
    queryKey: ['me', 'leaves', 'history', filters],
    queryFn: ({ pageParam }) => historyPage(filters, pageParam),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.next_cursor ?? undefined,
  })
  const [thisYear] = useState(() => new Date().getFullYear())
  const years = [thisYear + 1, thisYear, thisYear - 1, thisYear - 2]
  const set = (k: keyof HistoryFilters) => (e: React.ChangeEvent<HTMLSelectElement>) =>
    setFilters((f) => ({ ...f, [k]: e.target.value }))
  const rows = history.data?.pages.flatMap((p) => p.items) ?? []

  return (
    <section className="space-y-4">
      <header className="flex flex-wrap items-center justify-between gap-2">
        <h1 className="text-2xl font-semibold">{t('nav.history')}</h1>
        <a
          href={`/api/v1/me/leaves.csv${historyParams(filters)}`}
          download
          className="rounded border border-line px-3 py-1 text-sm"
        >
          {t('leave.history.csv')}
        </a>
      </header>
      <div className="flex flex-wrap gap-3">
        <div>
          <label htmlFor="h-year" className="mb-1 block text-sm font-medium">
            {t('leave.history.year')}
          </label>
          <select id="h-year" className={inputClass} value={filters.year} onChange={set('year')}>
            <option value="">{t('leave.history.allYears')}</option>
            {years.map((y) => (
              <option key={y} value={String(y)}>
                {y}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label htmlFor="h-type" className="mb-1 block text-sm font-medium">
            {t('leave.apply.type')}
          </label>
          <select id="h-type" className={inputClass} value={filters.type} onChange={set('type')}>
            <option value="">{t('leave.history.allTypes')}</option>
            {types.data?.map((b) => (
              <option key={b.leave_type.id} value={b.leave_type.id}>
                {b.leave_type.name}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label htmlFor="h-status" className="mb-1 block text-sm font-medium">
            {t('leave.history.status')}
          </label>
          <select
            id="h-status"
            className={inputClass}
            value={filters.status}
            onChange={set('status')}
          >
            <option value="">{t('leave.history.allStatuses')}</option>
            {STATUSES.map((s) => (
              <option key={s} value={s}>
                {t(`status.${s}`)}
              </option>
            ))}
          </select>
        </div>
      </div>

      {history.isPending && <p>{t('common.loading')}</p>}
      {history.error && <p className="text-red-700">{errorMessage(history.error, t)}</p>}
      {history.data && rows.length === 0 && (
        <p className="text-gray-700">{t('leave.history.empty')}</p>
      )}
      {rows.length > 0 && (
        <div className="overflow-x-auto rounded border border-line bg-white">
          <div className="overflow-x-auto">
            <table className="min-w-full text-left text-sm">
              <thead className="bg-surface">
                <tr>
                  {(
                    [
                      'type',
                      'dates',
                      'days',
                      'halfDay',
                      'status',
                      'requestedOn',
                      'decidedBy',
                      'reason',
                      'comment',
                      'notified',
                    ] as const
                  ).map((c) => (
                    <th key={c} scope="col" className="px-3 py-2 font-medium">
                      {t(`leave.history.columns.${c}`)}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.id} className="border-t border-line align-top">
                    <td className="px-3 py-2">{r.leave_type.name}</td>
                    <td className="px-3 py-2 whitespace-nowrap">{dates(r)}</td>
                    <td className="px-3 py-2">{r.days}</td>
                    <td className="px-3 py-2">
                      <HalfDay item={r} />
                    </td>
                    <td className="px-3 py-2">{t(`status.${r.status}`)}</td>
                    <td className="px-3 py-2 whitespace-nowrap">
                      {format(parseISO(r.requested_at), 'd MMM yyyy')}
                    </td>
                    <td className="px-3 py-2">{r.decided_by?.full_name}</td>
                    <td className="px-3 py-2">{r.reason}</td>
                    <td className="px-3 py-2">{r.decision_comment}</td>
                    <td className="px-3 py-2">{r.notify.join(', ')}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
      {history.hasNextPage && (
        <button
          type="button"
          onClick={() => void history.fetchNextPage()}
          disabled={history.isFetchingNextPage}
          className="rounded border border-line px-4 py-2"
        >
          {t('leave.history.more')}
        </button>
      )}
    </section>
  )
}
