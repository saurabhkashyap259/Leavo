// Team calendar (T135, FR-027): a month grid of who is away, with the same privacy rule as
// who's away — type and reason only when the viewer may see them (FR-058).
import { useQuery } from '@tanstack/react-query'
import { addMonths, eachDayOfInterval, endOfMonth, format, getISODay, startOfMonth } from 'date-fns'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { teamCalendarQuery, type CalendarPerson } from '../api/away'
import { errorMessage } from '../api/client'
import { AwayDot } from '../components/WhosAway'

export function TeamPage() {
  const { t } = useTranslation()
  const [month, setMonth] = useState(() => startOfMonth(new Date()))
  const [team, setTeam] = useState(false)
  const key = format(month, 'yyyy-MM')
  const cal = useQuery(teamCalendarQuery(key, team ? 'team' : 'org'))
  const byDate = new Map<string, CalendarPerson[]>(
    (cal.data?.month === key ? cal.data.days : []).map((d) => [d.date, d.people]),
  )
  const days = eachDayOfInterval({ start: month, end: endOfMonth(month) })
  // Monday-first grid: blanks before the 1st.
  const cells: (Date | null)[] = [...Array(getISODay(month) - 1).fill(null), ...days]
  while (cells.length % 7) cells.push(null)
  const weeks = Array.from({ length: cells.length / 7 }, (_, i) => cells.slice(i * 7, i * 7 + 7))

  return (
    <section className="space-y-4">
      <h1 className="text-2xl font-semibold">{t('nav.team')}</h1>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <button
            type="button"
            aria-label={t('team.previous')}
            onClick={() => setMonth((m) => addMonths(m, -1))}
            className="rounded border border-line px-3 py-1"
          >
            ‹
          </button>
          <h2 className="min-w-40 text-center text-lg font-semibold">
            {format(month, 'MMMM yyyy')}
          </h2>
          <button
            type="button"
            aria-label={t('team.next')}
            onClick={() => setMonth((m) => addMonths(m, 1))}
            className="rounded border border-line px-3 py-1"
          >
            ›
          </button>
        </div>
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={team} onChange={(e) => setTeam(e.target.checked)} />
          {t('dashboard.away.myTeam')}
        </label>
      </div>
      {cal.error && <p className="text-red-700">{errorMessage(cal.error, t)}</p>}
      <div className="overflow-x-auto">
        <table className="w-full table-fixed border-collapse text-sm">
          <thead>
            <tr>
              {(['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'] as const).map((d) => (
                <th key={d} scope="col" className="p-1 font-medium">
                  {t(`weekdays.${d}`)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {weeks.map((week, i) => (
              <tr key={i}>
                {week.map((day, j) => {
                  if (!day) return <td key={j} className="border border-line bg-surface" />
                  const iso = format(day, 'yyyy-MM-dd')
                  const people = byDate.get(iso) ?? []
                  return (
                    <td
                      key={j}
                      aria-label={format(day, 'd MMMM')}
                      className="h-24 border border-line p-1 align-top"
                    >
                      <div className="text-xs text-gray-600">{format(day, 'd')}</div>
                      {people.length > 0 && (
                        <ul className="space-y-0.5">
                          {people.map((p) => (
                            <li
                              key={`${p.user.id}-${p.half ?? 'full'}`}
                              className="flex items-center gap-1 truncate"
                              title={
                                p.leave_type
                                  ? `${p.leave_type.name}${p.reason ? ` — ${p.reason}` : ''}`
                                  : undefined
                              }
                            >
                              <AwayDot color={p.leave_type?.color} />
                              <span className="truncate">
                                {p.user.full_name}
                                {p.half && ` (${t(`leave.history.halves.${p.half}`)})`}
                              </span>
                            </li>
                          ))}
                        </ul>
                      )}
                    </td>
                  )
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  )
}
