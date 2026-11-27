// Admin home (T087, T191): the next-year holiday reminder (FR-036), who is away today, what
// waits for a decision, upcoming holidays, the first-run checklist and links to each area.
import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { dashboardQuery } from '../../api/admin'
import { errorMessage } from '../../api/client'
import { SetupChecklist, type ChecklistItem } from '../../components/SetupChecklist'
import { daySpan, shortDay } from '../../lib/dates'
import { seen } from './checklist'

const AREAS = [
  { to: '/admin/employees', label: 'nav.adminEmployees' },
  { to: '/admin/approvers', label: 'nav.adminApprovers' },
  { to: '/admin/holidays', label: 'nav.adminHolidays' },
  { to: '/admin/policy', label: 'nav.adminPolicy' },
  { to: '/admin/org', label: 'nav.adminOrg' },
  { to: '/admin/reports', label: 'nav.adminReports' },
  { to: '/admin/audit', label: 'nav.adminAudit' },
  { to: '/admin/carry-forward', label: 'nav.adminCarryForward' },
  { to: '/admin/import', label: 'nav.adminImport' },
  { to: '/admin/instance', label: 'nav.adminInstance' },
] as const

export function AdminHome() {
  const { t } = useTranslation()
  const dash = useQuery(dashboardQuery)
  const s = seen()
  const facts = dash.data?.setup_checklist
  const items: ChecklistItem[] = [
    { key: 'policy', done: s.has('policy'), to: '/admin/policy' },
    { key: 'domains', done: s.has('domains'), to: '/admin/org' },
    { key: 'holidays', done: s.has('holidays') || !!facts?.holidays, to: '/admin/holidays' },
    { key: 'approvers', done: s.has('approvers') || !!facts?.approvers, to: '/admin/approvers' },
    { key: 'invite', done: s.has('invite') || !!facts?.invite, to: '/admin/employees' },
  ]
  const d = dash.data
  const awayTitle = t('adminHome.away')
  return (
    <section className="max-w-4xl space-y-6">
      <h1 className="text-2xl font-semibold">{t('admin.title')}</h1>
      {d?.next_year_holidays_missing && (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-3 rounded bg-amber-50 p-3 text-amber-900"
        >
          <span>{t('adminHome.holidayBanner')}</span>
          <Link to="/admin/holidays" className="font-medium underline">
            {t('adminHome.addHolidays')}
          </Link>
        </div>
      )}
      {dash.error && <p className="text-red-700">{errorMessage(dash.error, t)}</p>}
      {d && <SetupChecklist items={items} />}
      {d && (
        <div className="grid gap-4 sm:grid-cols-3">
          <section className="rounded border border-line bg-white p-4 sm:col-span-2">
            <h2 className="mb-2 text-lg font-medium">{awayTitle}</h2>
            {d.away_today.length === 0 ? (
              <p className="text-gray-700">{t('adminHome.nobodyAway')}</p>
            ) : (
              <ul aria-label={awayTitle} className="space-y-1">
                {d.away_today.map((a) => (
                  <li key={`${a.user.id}-${a.from}`} className="flex items-center gap-2">
                    <span
                      aria-hidden
                      className="inline-block h-2.5 w-2.5 rounded-full"
                      style={{ backgroundColor: a.leave_type.color }}
                    />
                    <span className="font-medium">{a.user.full_name}</span>
                    <span className="text-sm text-gray-700">
                      {a.leave_type.name} · {daySpan(a.from, a.to)}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </section>
          <section className="space-y-1 rounded border border-line bg-white p-4">
            <h2 className="mb-2 text-lg font-medium">{t('adminHome.waiting')}</h2>
            <p>{t('adminHome.pending', { count: d.pending })}</p>
            <p className={d.pending_over_48h > 0 ? 'text-red-700' : 'text-gray-700'}>
              {t('adminHome.overdue', { count: d.pending_over_48h })}
            </p>
            <Link to="/approvals" className="text-sm text-accent underline">
              {t('adminHome.openApprovals')}
            </Link>
          </section>
          <section className="rounded border border-line bg-white p-4 sm:col-span-3">
            <h2 className="mb-2 text-lg font-medium">{t('dashboard.holidays.title')}</h2>
            {d.upcoming_holidays.length === 0 ? (
              <p className="text-gray-700">{t('dashboard.holidays.none')}</p>
            ) : (
              <ul className="flex flex-wrap gap-4">
                {d.upcoming_holidays.map((h) => (
                  <li key={h.id}>
                    <span className="text-gray-700">{shortDay(h.date)}</span> — {h.name}
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      )}
      <ul className="grid gap-2 sm:grid-cols-2">
        {AREAS.map((a) => (
          <li key={a.to}>
            <Link
              to={a.to}
              className="block rounded border border-line bg-white p-4 hover:bg-surface"
            >
              {t(a.label)}
            </Link>
          </li>
        ))}
      </ul>
    </section>
  )
}
