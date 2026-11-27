// Approvals inbox (T123, US4): oldest first, with the requester's balance and who else on the
// team is away; email links land here with the decision pre-filled (FR-022–FR-024, FR-058).
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { format, parseISO } from 'date-fns'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  approvalsQuery,
  cancelApprovalsQuery,
  compOffApprovalsQuery,
  type ApprovalItem,
  type Decision,
} from '../api/approvals'
import { errorMessage } from '../api/client'
import { DecisionDialog } from '../components/DecisionDialog'
import { workedText } from '../lib/comp-off'

export type Prefill = { request: string; decision: Decision; token?: string }
export type LinkNotice = 'used' | 'expired' | 'invalid'

const day = (d: string) => format(parseISO(d), 'EEE d MMM')
const span = (from: string, to: string) => (from === to ? day(from) : `${day(from)} – ${day(to)}`)
/** "Mon 30 Nov – Wed 2 Dec 2026" */
const fullSpan = (from: string, to: string) =>
  from === to
    ? format(parseISO(from), 'EEE d MMM yyyy')
    : `${day(from)} – ${format(parseISO(to), 'EEE d MMM yyyy')}`

function RequestCard({ item, onDecide }: { item: ApprovalItem; onDecide: (d: Decision) => void }) {
  const { t } = useTranslation()
  return (
    <article className="space-y-2 rounded border border-line bg-white p-4">
      <header className="flex flex-wrap items-baseline justify-between gap-2">
        <h2 className="text-lg font-semibold">{item.requester.full_name}</h2>
        <span className="text-xs text-gray-600">
          {t('approvals.requested', { date: format(parseISO(item.requested_at), 'd MMM yyyy') })}
        </span>
      </header>
      <p className="flex items-center gap-2">
        <span
          className="inline-block h-3 w-3 rounded-full"
          style={{ background: item.leave_type.color }}
        />
        <span>{item.leave_type.name}</span>
      </p>
      {item.dates ? (
        <p>
          {t('approvals.compOff.worked', {
            dates: workedText(item.dates, t('compOffs.halfDay'), 'EEE d MMM'),
            days: t('approvals.days', { count: item.days }),
          })}
        </p>
      ) : (
        <p>
          {fullSpan(item.from, item.to)} · {t('approvals.days', { count: item.days })}
        </p>
      )}
      <p className="text-gray-800">{item.reason}</p>
      {item.cancel && (
        <div className="rounded bg-amber-50 p-2 text-sm text-amber-900">
          <p>
            {t('approvals.cancel.asked', {
              date: day(item.cancel.requested_at),
              count: item.cancel.days_returned,
            })}
          </p>
          {item.cancel.reason && <p>{item.cancel.reason}</p>}
        </div>
      )}
      {item.kind !== 'comp_off' && (
        <p className="text-sm text-gray-700">
          {t('approvals.available', { count: item.balance.available })}
        </p>
      )}
      {item.attachment && (
        <p className="text-sm">
          {t('approvals.attachment')}: {item.attachment.name}
        </p>
      )}
      <div className="text-sm">
        <p className="font-medium">{t('approvals.alsoAway')}</p>
        {item.team_away.length === 0 ? (
          <p className="text-gray-600">{t('approvals.noneAway')}</p>
        ) : (
          <ul aria-label={t('approvals.alsoAway')} className="text-gray-700">
            {item.team_away.map((a, i) => (
              <li key={`${a.full_name}-${a.from}-${i}`}>
                {a.full_name}: {span(a.from, a.to)}
                {a.leave_type && ` — ${a.leave_type.name}${a.reason ? `, ${a.reason}` : ''}`}
              </li>
            ))}
          </ul>
        )}
      </div>
      <div className="flex gap-2 pt-1">
        <button
          type="button"
          onClick={() => onDecide('approve')}
          className="rounded bg-green-700 px-4 py-2 font-medium text-white"
        >
          {item.kind === 'cancel' ? t('approvals.cancel.approve') : t('approvals.approve')}
        </button>
        <button
          type="button"
          onClick={() => onDecide('reject')}
          className="rounded border border-red-700 px-4 py-2 font-medium text-red-800"
        >
          {item.kind === 'cancel' ? t('approvals.cancel.reject') : t('approvals.reject')}
        </button>
      </div>
    </article>
  )
}

export function ApprovalsPage({ prefill, link }: { prefill?: Prefill; link?: LinkNotice }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const items = useQuery(approvalsQuery)
  const cancels = useQuery(cancelApprovalsQuery)
  const compOffs = useQuery(compOffApprovalsQuery)
  const all = [...(items.data ?? []), ...(cancels.data ?? []), ...(compOffs.data ?? [])]
  const [open, setOpen] = useState<{ id: string; decision: Decision; token?: string } | null>(
    prefill ? { id: prefill.request, decision: prefill.decision, token: prefill.token } : null,
  )
  const [done, setDone] = useState<string | null>(null)
  const openItem = open && all.find((i) => i.id === open.id)
  const card = (item: ApprovalItem) => (
    <RequestCard
      key={`${item.kind}-${item.id}`}
      item={item}
      onDecide={(decision) => setOpen({ id: item.id, decision })}
    />
  )

  return (
    <section className="max-w-3xl space-y-4">
      <h1 className="text-2xl font-semibold">{t('approvals.title')}</h1>
      {link && (
        <p className="rounded bg-amber-50 p-3 text-amber-900">{t(`approvals.link.${link}`)}</p>
      )}
      {done && (
        <p role="status" className="rounded bg-green-50 p-3 text-green-800">
          {done}
        </p>
      )}
      {items.isPending && <p>{t('common.loading')}</p>}
      {items.error && <p className="text-red-700">{errorMessage(items.error, t)}</p>}
      {items.data?.length === 0 && cancels.data?.length === 0 && compOffs.data?.length === 0 && (
        <p className="text-gray-700">{t('approvals.empty')}</p>
      )}
      {items.data?.map(card)}
      {compOffs.data && compOffs.data.length > 0 && (
        <section aria-labelledby="comp-off-requests" className="space-y-4">
          <h2 id="comp-off-requests" className="text-xl font-semibold">
            {t('approvals.compOff.title')}
          </h2>
          {compOffs.data.map(card)}
        </section>
      )}
      {cancels.data && cancels.data.length > 0 && (
        <section aria-labelledby="cancel-requests" className="space-y-4">
          <h2 id="cancel-requests" className="text-xl font-semibold">
            {t('approvals.cancel.title')}
          </h2>
          {cancels.data.map(card)}
        </section>
      )}
      {openItem && open && (
        <DecisionDialog
          key={`${open.id}-${open.decision}`}
          item={openItem}
          decision={open.decision}
          token={open.token}
          onClose={() => setOpen(null)}
          onDecided={(d) => {
            setOpen(null)
            const name = openItem.requester.full_name
            setDone(
              openItem.kind === 'cancel'
                ? d.status === 'cancelled'
                  ? t('approvals.cancel.acceptedDone', { name })
                  : t('approvals.cancel.declinedDone', { name })
                : d.status === 'approved'
                  ? t('approvals.approvedDone', { name })
                  : t('approvals.rejectedDone', { name }),
            )
            void queryClient.invalidateQueries({ queryKey: ['approvals'] })
          }}
        />
      )}
    </section>
  )
}
