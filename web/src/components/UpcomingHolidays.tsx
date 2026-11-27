// The next 3 holidays, looking into the next leave year when this one is nearly over.
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { holidaysQuery } from '../api/holidays'
import { leaveYearOf, shortDay, todayIso } from '../lib/dates'

export function UpcomingHolidays({ leaveYearStartMonth }: { leaveYearStartMonth: number }) {
  const { t } = useTranslation()
  const [year] = useState(() => leaveYearOf(new Date(), leaveYearStartMonth))
  const current = useQuery(holidaysQuery(year))
  const next = useQuery(holidaysQuery(year + 1))
  const today = todayIso()
  const upcoming = [...(current.data ?? []), ...(next.data ?? [])]
    .filter((h) => h.date >= today)
    .sort((a, b) => a.date.localeCompare(b.date))
    .slice(0, 3)
  const title = t('dashboard.holidays.title')
  return (
    <section className="space-y-3 rounded border border-line bg-white p-4">
      <h2 className="text-lg font-semibold">{title}</h2>
      {current.data && upcoming.length === 0 && (
        <p className="text-gray-700">{t('dashboard.holidays.none')}</p>
      )}
      {upcoming.length > 0 && (
        <ul aria-label={title} className="space-y-1">
          {upcoming.map((h) => (
            <li key={h.id}>
              <span className="text-gray-700">{shortDay(h.date)}</span> — {h.name}
              {h.kind === 'optional' && (
                <span className="text-sm text-gray-600"> ({t('dashboard.holidays.optional')})</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
