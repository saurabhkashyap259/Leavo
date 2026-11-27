// Who's away today / this week / this month (FR-027). Colleagues whose leave details the
// viewer may not see come without type and reason and get a neutral colour (FR-058).
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { awayQuery, type AwayItem, type AwayRange } from '../api/away'
import { errorMessage } from '../api/client'
import { daySpan } from '../lib/dates'

const RANGES: AwayRange[] = ['today', 'week', 'month']

export function AwayDot({ color }: { color?: string }) {
  return (
    <span
      aria-hidden
      data-testid="away-dot"
      data-neutral={color ? 'false' : 'true'}
      className={`inline-block h-3 w-3 shrink-0 rounded-full ${color ? '' : 'bg-gray-400'}`}
      style={color ? { backgroundColor: color } : undefined}
    />
  )
}

function Row({ item }: { item: AwayItem }) {
  const { t } = useTranslation()
  return (
    <li className="flex items-baseline gap-2">
      <AwayDot color={item.leave_type?.color} />
      <span>
        <span className="font-medium">{item.user.full_name}</span>{' '}
        <span className="text-gray-700">
          {daySpan(item.from, item.to)} · {t('approvals.days', { count: item.days })}
        </span>
        {item.leave_type && (
          <span className="text-gray-700">
            {' '}
            — {item.leave_type.name}
            {item.reason ? `, ${item.reason}` : ''}
          </span>
        )}
      </span>
    </li>
  )
}

export function WhosAway() {
  const { t } = useTranslation()
  const [range, setRange] = useState<AwayRange>('today')
  const [team, setTeam] = useState(false)
  const away = useQuery(awayQuery(range, team ? 'team' : 'org'))
  const title = t('dashboard.away.title')

  return (
    <section className="space-y-3 rounded border border-line bg-white p-4">
      <header className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-lg font-semibold">{title}</h2>
        {away.data && (
          <span className="rounded-full bg-surface px-3 py-1 text-sm">
            {t('dashboard.away.today', { count: away.data.count_today })}
          </span>
        )}
      </header>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div role="tablist" aria-label={title} className="flex gap-1">
          {RANGES.map((r) => (
            <button
              key={r}
              type="button"
              role="tab"
              aria-selected={range === r}
              onClick={() => setRange(r)}
              className={`rounded px-3 py-1 text-sm ${range === r ? 'bg-accent text-white' : 'border border-line'}`}
            >
              {t(`dashboard.away.range.${r}`)}
            </button>
          ))}
        </div>
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={team} onChange={(e) => setTeam(e.target.checked)} />
          {t('dashboard.away.myTeam')}
        </label>
      </div>
      {away.isPending && <p>{t('common.loading')}</p>}
      {away.error && <p className="text-red-700">{errorMessage(away.error, t)}</p>}
      {away.data?.items.length === 0 && (
        <p className="text-gray-700">{t('dashboard.away.nobody')}</p>
      )}
      {away.data && away.data.items.length > 0 && (
        <ul aria-label={title} className="space-y-1">
          {away.data.items.map((i) => (
            <Row key={`${i.user.id}-${i.from}`} item={i} />
          ))}
        </ul>
      )}
    </section>
  )
}
