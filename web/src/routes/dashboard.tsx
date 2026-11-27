// Employee dashboard (T133, US5): balance cards, who's away, own requests, next holidays
// (FR-026, FR-027).
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { errorMessage } from '../api/client'
import { balancesQuery } from '../api/leaves'
import { meQuery } from '../api/me'
import { BalanceCard } from '../components/BalanceCard'
import { MyRequests } from '../components/MyRequests'
import { UpcomingHolidays } from '../components/UpcomingHolidays'
import { WhosAway } from '../components/WhosAway'

export function DashboardPage() {
  const { t } = useTranslation()
  const me = useQuery(meQuery)
  const balances = useQuery(balancesQuery)
  return (
    <div className="max-w-5xl space-y-6">
      <h1 className="text-2xl font-semibold">{t('nav.dashboard')}</h1>
      <section aria-label={t('dashboard.balance.title')}>
        {balances.isPending && <p>{t('common.loading')}</p>}
        {balances.error && <p className="text-red-700">{errorMessage(balances.error, t)}</p>}
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {balances.data?.map((b) => (
            <BalanceCard key={b.leave_type.id} balance={b} />
          ))}
        </div>
      </section>
      <div className="grid gap-4 lg:grid-cols-2">
        <WhosAway />
        <div className="space-y-4">
          <MyRequests />
          {me.data && <UpcomingHolidays leaveYearStartMonth={me.data.org.leave_year_start_month} />}
        </div>
      </div>
    </div>
  )
}
