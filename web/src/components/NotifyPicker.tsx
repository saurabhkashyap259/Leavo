// Notify members (FR-059): colleagues found by name or email, any valid external address,
// the last request's members as one-click suggestions, at most 20.
import { useQuery } from '@tanstack/react-query'
import { useEffect, useId, useState, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { searchUsers, type NotifyMember, type UserHit } from '../api/leaves'
import { EMAIL_RE, inputClass } from '../routes/setup/wizard'

export const MAX_NOTIFY = 20

export type NotifyEntry = { user_id: string | null; email: string; label: string }

function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value)
  useEffect(() => {
    const id = setTimeout(() => setV(value), ms)
    return () => clearTimeout(id)
  }, [value, ms])
  return v
}

export function NotifyPicker({
  value,
  onChange,
  lastUsed = [],
}: {
  value: NotifyEntry[]
  onChange: (v: NotifyEntry[]) => void
  lastUsed?: NotifyMember[]
}) {
  const { t } = useTranslation()
  const id = useId()
  const [text, setText] = useState('')
  const [invalid, setInvalid] = useState(false)
  const q = useDebounced(text.trim(), 150)
  const full = value.length >= MAX_NOTIFY
  const search = useQuery({
    queryKey: ['users', 'search', q],
    queryFn: ({ signal }) => searchUsers(q, signal),
    enabled: q.length >= 2 && !full,
    staleTime: 30_000,
  })
  const chosen = new Set(value.map((v) => v.email))
  const hits = (search.data ?? []).filter((h) => !chosen.has(h.email))

  const add = (entry: NotifyEntry) => {
    if (full || chosen.has(entry.email)) return
    onChange([...value, entry])
    setText('')
    setInvalid(false)
  }
  const addHit = (h: UserHit) => add({ user_id: h.id, email: h.email, label: h.full_name })

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== 'Enter') return
    e.preventDefault()
    const raw = text.trim().toLowerCase()
    if (raw === '') return
    if (EMAIL_RE.test(raw)) add({ user_id: null, email: raw, label: raw })
    else setInvalid(true)
  }

  const suggestions = lastUsed.filter((m) => !chosen.has(m.email))

  return (
    <div className="space-y-2">
      <label htmlFor={id} className="block text-sm font-medium">
        {t('leave.notify.label')}
      </label>
      {value.length > 0 && (
        <ul className="flex flex-wrap gap-2">
          {value.map((v) => (
            <li
              key={v.email}
              className="flex items-center gap-1 rounded-full bg-surface px-3 py-1 text-sm"
            >
              <span>{v.label}</span>
              <button
                type="button"
                aria-label={t('leave.notify.remove', { name: v.label })}
                onClick={() => onChange(value.filter((x) => x.email !== v.email))}
                className="ml-1 text-gray-600 hover:text-gray-900"
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      )}
      <input
        id={id}
        role="combobox"
        aria-expanded={hits.length > 0}
        aria-controls={`${id}-list`}
        aria-describedby={`${id}-hint`}
        autoComplete="off"
        className={inputClass}
        value={text}
        disabled={full}
        onChange={(e) => {
          setText(e.target.value)
          setInvalid(false)
        }}
        onKeyDown={onKeyDown}
      />
      {hits.length > 0 && (
        <ul id={`${id}-list`} role="listbox" className="rounded border border-line bg-white">
          {hits.map((h) => (
            <li
              key={h.id}
              role="option"
              aria-selected={false}
              tabIndex={-1}
              onClick={() => addHit(h)}
              onKeyDown={(e) => e.key === 'Enter' && addHit(h)}
              className="cursor-pointer px-3 py-2 text-sm hover:bg-surface"
            >
              {h.full_name} <span className="text-gray-600">{h.email}</span>
            </li>
          ))}
        </ul>
      )}
      {invalid && <p className="text-sm text-red-700">{t('leave.notify.invalid')}</p>}
      {full ? (
        <p className="text-sm text-gray-700">{t('leave.notify.max')}</p>
      ) : (
        suggestions.length > 0 && (
          <p className="flex flex-wrap items-center gap-2 text-sm">
            <span className="text-gray-600">{t('leave.notify.lastUsed')}</span>
            {suggestions.map((m) => (
              <button
                key={m.email}
                type="button"
                aria-label={t('leave.notify.add', { name: m.email })}
                onClick={() => add({ user_id: m.user_id ?? null, email: m.email, label: m.email })}
                className="rounded-full border border-line px-2 py-0.5"
              >
                + {m.email}
              </button>
            ))}
          </p>
        )
      )}
      <p id={`${id}-hint`} className="text-xs text-gray-600">
        {t('leave.notify.hint')}
      </p>
    </div>
  )
}
