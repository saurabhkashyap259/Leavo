// One leave type's balance (FR-026): available in large type, then allotted + carried,
// taken and pending.
import { format, parseISO } from 'date-fns'
import { useTranslation } from 'react-i18next'
import type { Balance } from '../api/leaves'
import { expiryKey } from '../lib/comp-off'

export function BalanceCard({ balance }: { balance: Balance }) {
  const { t } = useTranslation()
  const b = balance
  const allotted = t('dashboard.balance.allotted', { count: b.allotted })
  const carried =
    b.carried !== 0 ? ` + ${t('dashboard.balance.carried', { count: b.carried })}` : ''
  return (
    <article className="rounded border border-line bg-white p-4">
      <h3 className="flex items-center gap-2 font-medium">
        <span
          aria-hidden
          className="inline-block h-3 w-3 rounded-full"
          style={{ backgroundColor: b.leave_type.color }}
        />
        {b.leave_type.name}
      </h3>
      <p className="mt-2 text-4xl font-semibold">{b.available}</p>
      <p className="text-sm text-gray-600">{t('dashboard.balance.available')}</p>
      {b.leave_type.is_comp_off ? (
        b.next_expiry && (
          <p className="mt-2 text-sm text-amber-900">
            {t(expiryKey(b.next_expiry.days), {
              days: b.next_expiry.days,
              date: format(parseISO(b.next_expiry.date), 'EEE d MMM yyyy'),
            })}
          </p>
        )
      ) : (
        <p className="mt-2 text-sm text-gray-800">{`${allotted}${carried}`}</p>
      )}
      <p className="text-sm text-gray-800">
        {t('dashboard.balance.taken', { count: b.taken })} ·{' '}
        {t('dashboard.balance.pending', { count: b.pending })}
        {b.adjusted !== 0 &&
          ` · ${t('dashboard.balance.adjusted', { value: b.adjusted > 0 ? `+${b.adjusted}` : b.adjusted })}`}
      </p>
    </article>
  )
}
