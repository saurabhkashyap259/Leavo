// Audit log (T191, FR-044): who did what and when, with the values before and after.
import { useInfiniteQuery } from '@tanstack/react-query'
import { format, parseISO } from 'date-fns'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { auditPage, type AuditFilters, type AuditItem } from '../../api/admin'
import { errorMessage } from '../../api/client'
import { inputClass } from '../setup/wizard'

const text = (v: unknown) =>
  v === null || v === undefined ? '—' : typeof v === 'object' ? JSON.stringify(v) : String(v)

/** "role: employee → approver" for each key that changed or appears on one side only. */
function changes(item: AuditItem): string[] {
  const before = (item.before ?? {}) as Record<string, unknown>
  const after = (item.after ?? {}) as Record<string, unknown>
  const keys = [...new Set([...Object.keys(before), ...Object.keys(after)])]
  return keys
    .filter((k) => JSON.stringify(before[k]) !== JSON.stringify(after[k]))
    .map((k) =>
      k in before && k in after
        ? `${k}: ${text(before[k])} → ${text(after[k])}`
        : `${k}: ${text(k in after ? after[k] : before[k])}`,
    )
}

export function AuditPage() {
  const { t } = useTranslation()
  const [draft, setDraft] = useState<AuditFilters>({})
  const [filters, setFilters] = useState<AuditFilters>({})
  const log = useInfiniteQuery({
    queryKey: ['admin', 'audit', filters],
    queryFn: ({ pageParam }) => auditPage(filters, pageParam),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.next_cursor ?? undefined,
  })
  const items = log.data?.pages.flatMap((p) => p.items) ?? []
  const set = (k: keyof AuditFilters, v: string) => setDraft((d) => ({ ...d, [k]: v || undefined }))
  const apply = (e: FormEvent) => {
    e.preventDefault()
    setFilters(draft)
  }
  const title = t('audit.title')
  return (
    <section className="max-w-5xl space-y-4">
      <h1 className="text-2xl font-semibold">{title}</h1>
      <form onSubmit={apply} className="flex flex-wrap items-end gap-2">
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('audit.action')}</span>
          <input
            className={inputClass}
            placeholder="leave."
            value={draft.action ?? ''}
            onChange={(e) => set('action', e.target.value)}
          />
        </label>
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('audit.from')}</span>
          <input
            type="date"
            className={inputClass}
            value={draft.from ?? ''}
            onChange={(e) => set('from', e.target.value)}
          />
        </label>
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('audit.to')}</span>
          <input
            type="date"
            className={inputClass}
            value={draft.to ?? ''}
            onChange={(e) => set('to', e.target.value)}
          />
        </label>
        <button type="submit" className="rounded border border-line px-3 py-2">
          {t('audit.apply')}
        </button>
      </form>
      {log.error && <p className="text-red-700">{errorMessage(log.error, t)}</p>}
      {log.data && items.length === 0 && <p className="text-gray-700">{t('audit.none')}</p>}
      {items.length > 0 && (
        <ul aria-label={title} className="divide-y divide-line rounded border border-line bg-white">
          {items.map((i) => (
            <li key={i.id} className="space-y-1 p-3 text-sm">
              <div className="flex flex-wrap items-baseline gap-2">
                <span className="font-mono">{i.action}</span>
                <span className="text-gray-700">{i.actor?.full_name ?? t('employee.system')}</span>
                <span className="ml-auto text-xs text-gray-600">
                  {format(parseISO(i.created_at), 'd MMM yyyy HH:mm')}
                </span>
              </div>
              {changes(i).length > 0 && (
                <ul className="text-gray-700">
                  {changes(i).map((c) => (
                    <li key={c}>{c}</li>
                  ))}
                </ul>
              )}
            </li>
          ))}
        </ul>
      )}
      {log.hasNextPage && (
        <button
          type="button"
          className="rounded border border-line px-4 py-2"
          disabled={log.isFetchingNextPage}
          onClick={() => void log.fetchNextPage()}
        >
          {t('leave.history.more')}
        </button>
      )}
    </section>
  )
}
