import { Link } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

export type ChecklistItem = {
  key: 'policy' | 'domains' | 'holidays' | 'approvers' | 'invite'
  done: boolean
  to: string
}

/** The admin's first-run checklist; hidden once every step is done. */
export function SetupChecklist({ items }: { items: ChecklistItem[] }) {
  const { t } = useTranslation()
  if (items.every((i) => i.done)) return null
  return (
    <section className="rounded border border-line bg-white p-4">
      <h2 className="mb-2 text-lg font-medium">{t('admin.checklist.title')}</h2>
      <ul className="space-y-1">
        {items.map((i) => (
          <li key={i.key} className="flex items-center gap-2 text-sm">
            <span aria-hidden className={i.done ? 'text-green-700' : 'text-gray-400'}>
              {i.done ? '✓' : '○'}
            </span>
            {i.done ? (
              <>
                <span className="text-gray-600 line-through">{t(`admin.checklist.${i.key}`)}</span>
                <span className="sr-only">{t('common.done')}</span>
              </>
            ) : (
              <Link to={i.to} className="underline">
                {t(`admin.checklist.${i.key}`)}
              </Link>
            )}
          </li>
        ))}
      </ul>
    </section>
  )
}
