// Holiday calendar (T165, FR-033): the current and next leave year as a list (past holidays
// dimmed, the next one highlighted with a countdown) or as month grids.
import { useQuery } from '@tanstack/react-query'
import { differenceInCalendarDays, format, parseISO } from 'date-fns'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { errorMessage } from '../api/client'
import { holidaysQuery, type Holiday } from '../api/holidays'
import { MonthGrid } from '../components/MonthGrid'
import { leaveYearOf, monthsOfLeaveYear, todayIso } from '../lib/dates'

function HolidayList({ year, items, today }: { year: number; items: Holiday[]; today: string }) {
  const { t } = useTranslation()
  const next = items.find((h) => h.date >= today)
  return (
    <ul
      aria-label={t('holidays.listLabel', { year })}
      className="divide-y divide-line rounded border border-line bg-white"
    >
      {items.map((h) => {
        const past = h.date < today
        const isNext = h === next
        const days = differenceInCalendarDays(parseISO(h.date), parseISO(today))
        return (
          <li
            key={h.id}
            data-past={past}
            aria-current={isNext ? 'date' : undefined}
            className={`flex flex-wrap items-baseline gap-3 p-3 ${past ? 'opacity-50' : ''} ${isNext ? 'bg-accent/10' : ''}`}
          >
            <span className="w-32 text-gray-700">{format(parseISO(h.date), 'EEE d MMM')}</span>
            <span className="font-medium">{h.name}</span>
            {h.kind === 'optional' && (
              <span className="rounded bg-surface px-2 text-xs">{t('holidays.optional')}</span>
            )}
            {h.location && <span className="text-sm text-gray-600">· {h.location}</span>}
            {isNext && (
              <span className="ml-auto text-sm font-medium text-accent">
                {days === 0
                  ? t('holidays.today', { name: h.name })
                  : t('holidays.countdown', { name: h.name, count: days })}
              </span>
            )}
          </li>
        )
      })}
    </ul>
  )
}

export function HolidaysPage({ leaveYearStartMonth }: { leaveYearStartMonth: number }) {
  const { t } = useTranslation()
  const [current] = useState(() => leaveYearOf(new Date(), leaveYearStartMonth))
  const [year, setYear] = useState(current)
  const [view, setView] = useState<'list' | 'month'>('list')
  const holidays = useQuery(holidaysQuery(year))
  const today = todayIso()
  return (
    <section className="max-w-4xl space-y-4">
      <h1 className="text-2xl font-semibold">{t('holidays.title')}</h1>
      <div className="flex flex-wrap items-center gap-4">
        <div role="tablist" className="flex gap-1">
          {[current, current + 1].map((y) => (
            <button
              key={y}
              role="tab"
              type="button"
              aria-selected={y === year}
              onClick={() => setYear(y)}
              className={`rounded px-3 py-1 ${y === year ? 'bg-accent text-white' : 'border border-line'}`}
            >
              {y}
            </button>
          ))}
        </div>
        <button
          type="button"
          className="text-sm text-accent underline"
          onClick={() => setView(view === 'list' ? 'month' : 'list')}
        >
          {view === 'list' ? t('holidays.monthView') : t('holidays.listView')}
        </button>
      </div>
      {holidays.isPending && <p>{t('common.loading')}</p>}
      {holidays.error && <p className="text-red-700">{errorMessage(holidays.error, t)}</p>}
      {holidays.data?.length === 0 && <p className="text-gray-700">{t('holidays.none')}</p>}
      {holidays.data && holidays.data.length > 0 && view === 'list' && (
        <HolidayList year={year} items={holidays.data} today={today} />
      )}
      {holidays.data && view === 'month' && (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {monthsOfLeaveYear(year, leaveYearStartMonth).map((m) => (
            <MonthGrid key={m} month={m} holidays={holidays.data} today={today} />
          ))}
        </div>
      )}
    </section>
  )
}
