import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import type { PackInfo } from '../../api/setup'
import { Field, PrimaryButton } from './shared'
import { inputClass, type WizardData } from './wizard'

type Defaults = Pick<WizardData, 'holidayPack' | 'seedLeaveTypes' | 'extraDomains'>

export function DefaultsStep({
  value,
  packs,
  busy,
  error,
  onFinish,
}: {
  value: Defaults
  packs: PackInfo[]
  busy: boolean
  error: string | null
  onFinish: (v: Defaults) => void
}) {
  const { t } = useTranslation()
  const [v, setV] = useState(value)

  function submit(e: FormEvent) {
    e.preventDefault()
    onFinish(v)
  }

  return (
    <form onSubmit={submit} className="space-y-4" noValidate>
      <h2 className="text-lg font-medium">{t('setup.defaultsTitle')}</h2>
      <Field label={t('setup.holidayList')}>
        {(id) => (
          <select
            id={id}
            value={v.holidayPack}
            onChange={(e) => setV({ ...v, holidayPack: e.target.value })}
            className={inputClass}
          >
            <option value="">{t('setup.noHolidays')}</option>
            {packs.map((p) => (
              <option key={p.id} value={p.id}>
                {t('setup.holidayCount', { name: p.name, count: p.count })}
              </option>
            ))}
          </select>
        )}
      </Field>
      <label className="flex items-start gap-2 text-sm">
        <input
          type="checkbox"
          className="mt-1"
          checked={v.seedLeaveTypes}
          onChange={(e) => setV({ ...v, seedLeaveTypes: e.target.checked })}
        />
        {t('setup.seedTypes')}
      </label>
      <Field label={t('setup.extraDomains')} help={t('setup.extraDomainsHelp')}>
        {(id, hint) => (
          <input
            id={id}
            aria-describedby={hint}
            value={v.extraDomains}
            onChange={(e) => setV({ ...v, extraDomains: e.target.value })}
            className={inputClass}
          />
        )}
      </Field>
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
      <PrimaryButton type="submit" busy={busy}>
        {t('setup.finish')}
      </PrimaryButton>
    </form>
  )
}
