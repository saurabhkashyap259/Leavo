// Organization settings and whitelisted domains (T087, FR-009, FR-010, FR-062).
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import {
  addDomain,
  domainsQuery,
  orgQuery,
  patchOrg,
  removeDomain,
  type OrgSettings,
} from '../../api/admin'
import { errorMessage } from '../../api/client'
import { meQuery } from '../../api/me'
import { CalendarFields } from '../setup/org'
import { Field } from '../setup/shared'
import { inputClass } from '../setup/wizard'

export function OrgPage() {
  const { t } = useTranslation()
  const org = useQuery(orgQuery)
  return (
    <section className="max-w-2xl space-y-8">
      <h1 className="text-2xl font-semibold">{t('admin.org.title')}</h1>
      {org.data ? <SettingsForm initial={org.data} /> : <p>{t('common.loading')}</p>}
      <Domains />
    </section>
  )
}

function SettingsForm({ initial }: { initial: OrgSettings }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [form, setForm] = useState(initial)
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null)
  const save = useMutation({
    mutationFn: () =>
      patchOrg({
        name: form.name,
        timezone: form.timezone,
        weekend: form.weekend,
        carry_forward_cap: Number(form.carry_forward_cap),
        comp_off_expiry_days: Number(form.comp_off_expiry_days),
        ...(form.leave_year_start_month !== initial.leave_year_start_month
          ? { leave_year_start_month: form.leave_year_start_month }
          : {}),
      }),
    onSuccess: async (updated) => {
      qc.setQueryData(orgQuery.queryKey, updated)
      await qc.invalidateQueries({ queryKey: meQuery.queryKey })
      setResult({ ok: true, text: t('admin.org.saved') })
    },
    onError: (e) => setResult({ ok: false, text: errorMessage(e, t) }),
  })

  function submit(e: FormEvent) {
    e.preventDefault()
    setResult(null)
    save.mutate()
  }

  return (
    <form
      onSubmit={submit}
      className="space-y-4 rounded border border-line bg-white p-4"
      noValidate
    >
      <h2 className="text-lg font-medium">{t('admin.org.settings')}</h2>
      <Field label={t('admin.org.name')}>
        {(id) => (
          <input
            id={id}
            value={form.name}
            maxLength={120}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            className={inputClass}
          />
        )}
      </Field>
      <CalendarFields value={form} onChange={(v) => setForm({ ...form, ...v })} />
      <p className="text-sm text-gray-600">{t('admin.org.changesApplyLater')}</p>
      <Field label={t('admin.org.carryForwardCap')}>
        {(id) => (
          <input
            id={id}
            inputMode="decimal"
            value={form.carry_forward_cap}
            onChange={(e) =>
              setForm({ ...form, carry_forward_cap: e.target.value as unknown as number })
            }
            className={inputClass}
          />
        )}
      </Field>
      <Field label={t('admin.org.compOffExpiry')}>
        {(id) => (
          <input
            id={id}
            inputMode="numeric"
            value={form.comp_off_expiry_days}
            onChange={(e) =>
              setForm({ ...form, comp_off_expiry_days: e.target.value as unknown as number })
            }
            className={inputClass}
          />
        )}
      </Field>
      {result &&
        (result.ok ? (
          <p role="status" className="text-sm text-green-800">
            {result.text}
          </p>
        ) : (
          <p role="alert" className="text-sm text-red-700">
            {result.text}
          </p>
        ))}
      <button
        type="submit"
        disabled={save.isPending}
        className="rounded bg-accent px-4 py-2 font-medium text-white"
      >
        {t('admin.org.save')}
      </button>
    </form>
  )
}

function Domains() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const domains = useQuery(domainsQuery)
  const [value, setValue] = useState('')
  const [error, setError] = useState<string | null>(null)
  const refresh = () => qc.invalidateQueries({ queryKey: domainsQuery.queryKey })
  const add = useMutation({
    mutationFn: () => addDomain(value),
    onSuccess: async () => {
      setValue('')
      setError(null)
      await refresh()
    },
    onError: (e) => setError(errorMessage(e, t)),
  })
  const remove = useMutation({ mutationFn: removeDomain, onSuccess: refresh })

  return (
    <section className="space-y-3 rounded border border-line bg-white p-4">
      <h2 id="domains-heading" className="text-lg font-medium">
        {t('admin.org.domains')}
      </h2>
      <p className="text-sm text-gray-600">{t('admin.org.domainsHelp')}</p>
      {domains.data && (
        <ul aria-labelledby="domains-heading" className="divide-y divide-line">
          {domains.data.map((d) => (
            <li key={d.id} className="flex items-center justify-between py-2">
              <span>{d.domain}</span>
              <button
                type="button"
                aria-label={t('admin.org.removeDomain', { domain: d.domain })}
                onClick={() => remove.mutate(d.id)}
                className="text-sm text-red-700 underline"
              >
                {t('common.remove')}
              </button>
            </li>
          ))}
        </ul>
      )}
      <form
        onSubmit={(e) => {
          e.preventDefault()
          if (value.trim()) add.mutate()
        }}
        className="flex items-end gap-2"
      >
        <div className="flex-1">
          <Field label={t('admin.org.addDomain')} error={error ?? undefined}>
            {(id, hint) => (
              <input
                id={id}
                aria-describedby={hint}
                value={value}
                onChange={(e) => setValue(e.target.value)}
                className={inputClass}
              />
            )}
          </Field>
        </div>
        <button
          type="submit"
          disabled={add.isPending}
          className="rounded border border-line px-3 py-2"
        >
          {t('admin.org.add')}
        </button>
      </form>
    </section>
  )
}
