// Carry-forward preview (T173, FR-039): for the leave year being closed, each employee's unused
// days of carry-forward types, what carries into next year (up to the cap) and what lapses.
// The same report once year end has run; downloadable as CSV.
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { carryForwardQuery } from '../../api/admin'
import { errorMessage } from '../../api/client'
import { leaveYearOf } from '../../lib/dates'

export function CarryForwardPage({
  leaveYearStartMonth,
  carryForwardCap,
}: {
  leaveYearStartMonth: number
  carryForwardCap: number
}) {
  const { t } = useTranslation()
  const [current] = useState(() => leaveYearOf(new Date(), leaveYearStartMonth))
  const [year, setYear] = useState(current)
  const rows = useQuery(carryForwardQuery(year))
  const carried = (rows.data ?? []).reduce((s, r) => s + Math.max(r.carry, 0), 0)
  const lapsed = (rows.data ?? []).reduce((s, r) => s + r.lapse, 0)
  const title = t('carryForward.tableLabel', { from: year, to: year + 1 })
  return (
    <section className="max-w-4xl space-y-4">
      <h1 className="text-2xl font-semibold">{t('carryForward.title')}</h1>
      <p className="text-sm text-gray-700">{t('carryForward.help')}</p>
      <div className="flex flex-wrap items-end gap-4">
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('carryForward.year')}</span>
          <select
            className="rounded border border-line px-2 py-1"
            value={year}
            onChange={(e) => setYear(Number(e.target.value))}
          >
            {[current, current - 1, current - 2].map((y) => (
              <option key={y} value={y}>
                {y}
              </option>
            ))}
          </select>
        </label>
        <a
          className="text-sm text-accent underline"
          href={`/api/v1/admin/carry-forward/preview.csv?year=${year}`}
        >
          {t('carryForward.download')}
        </a>
      </div>
      {rows.isPending && <p>{t('common.loading')}</p>}
      {rows.error && <p className="text-red-700">{errorMessage(rows.error, t)}</p>}
      {rows.data?.length === 0 && (
        <p className="text-gray-700">{t('carryForward.none', { year })}</p>
      )}
      {rows.data && rows.data.length > 0 && (
        <>
          <p className="text-sm font-medium">
            {t('carryForward.totals', { carried, lapsed, cap: carryForwardCap })}
          </p>
          <div className="relative overflow-x-auto">
            <table
              aria-label={title}
              className="w-full rounded border border-line bg-white text-left text-sm"
            >
              <caption className="sr-only">{title}</caption>
              <thead className="text-gray-600">
                <tr>
                  <th className="p-2">{t('carryForward.name')}</th>
                  <th className="p-2">{t('carryForward.email')}</th>
                  <th className="p-2">{t('carryForward.type')}</th>
                  <th className="p-2 text-right">{t('carryForward.unused')}</th>
                  <th className="p-2 text-right">{t('carryForward.carry')}</th>
                  <th className="p-2 text-right">{t('carryForward.lapse')}</th>
                </tr>
              </thead>
              <tbody>
                {rows.data.map((r) => (
                  <tr key={`${r.user.id}-${r.leave_type.id}`} className="border-t border-line">
                    <td className="p-2">{r.user.full_name}</td>
                    <td className="p-2 text-gray-700">{r.user.email}</td>
                    <td className="p-2">{r.leave_type.name}</td>
                    <td className="p-2 text-right">{r.unused}</td>
                    <td className={`p-2 text-right ${r.carry < 0 ? 'text-red-700' : ''}`}>
                      {r.carry}
                    </td>
                    <td className="p-2 text-right">{r.lapse}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </section>
  )
}
