// Reports (T191, FR-044): days taken in a leave year by type, employee or month, with CSV.
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { reportQuery, type GroupBy } from '../../api/admin'
import { errorMessage } from '../../api/client'
import { leaveYearOf } from '../../lib/dates'

const TABS: GroupBy[] = ['type', 'user', 'month']

export function ReportsPage({ leaveYearStartMonth }: { leaveYearStartMonth: number }) {
  const { t } = useTranslation()
  const [current] = useState(() => leaveYearOf(new Date(), leaveYearStartMonth))
  const [year, setYear] = useState(current)
  const [by, setBy] = useState<GroupBy>('type')
  const rows = useQuery(reportQuery(by, year))
  const total = (rows.data ?? []).reduce((s, r) => s + r.days, 0)
  const title = t('reports.tableLabel', { year })
  return (
    <section className="max-w-4xl space-y-4">
      <h1 className="text-2xl font-semibold">{t('reports.title')}</h1>
      <div className="flex flex-wrap items-center gap-4">
        <div role="tablist" className="flex gap-1">
          {TABS.map((g) => (
            <button
              key={g}
              role="tab"
              type="button"
              aria-selected={g === by}
              onClick={() => setBy(g)}
              className={`rounded px-3 py-1 ${g === by ? 'bg-accent text-white' : 'border border-line'}`}
            >
              {t(`reports.by.${g}`)}
            </button>
          ))}
        </div>
        <label className="text-sm">
          <span className="mr-2">{t('reports.year')}</span>
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
          href={`/api/v1/admin/reports/leaves.csv?group_by=${by}&year=${year}`}
        >
          {t('carryForward.download')}
        </a>
      </div>
      {rows.error && <p className="text-red-700">{errorMessage(rows.error, t)}</p>}
      {rows.data?.length === 0 && <p className="text-gray-700">{t('reports.none', { year })}</p>}
      {rows.data && rows.data.length > 0 && (
        <>
          <p className="text-sm font-medium">{t('reports.total', { days: total })}</p>
          <div className="overflow-x-auto">
            <table
              aria-label={title}
              className="w-full rounded border border-line bg-white text-left text-sm"
            >
              <thead className="text-gray-600">
                <tr>
                  <th className="p-2">{t(`reports.by.${by}`)}</th>
                  <th className="p-2">{t('carryForward.type')}</th>
                  <th className="p-2 text-right">{t('employee.days')}</th>
                </tr>
              </thead>
              <tbody>
                {rows.data.map((r) => (
                  <tr key={`${r.key}-${r.leave_type.id}`} className="border-t border-line">
                    <td className="p-2">{r.group}</td>
                    <td className="p-2">{r.leave_type.name}</td>
                    <td className="p-2 text-right">{r.days}</td>
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
