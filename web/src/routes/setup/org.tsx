import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Field, PrimaryButton } from './shared'
import { WEEKDAYS, inputClass, timeZones, type WizardData } from './wizard'

type Org = WizardData['org']

/** Shared by the wizard and /admin/org: time zone, weekend and leave-year fields. */
export function CalendarFields({
  value,
  onChange,
  weekendError,
}: {
  value: Pick<Org, 'timezone' | 'weekend' | 'leave_year_start_month'>
  onChange: (v: Partial<Org>) => void
  weekendError?: string
}) {
  const { t } = useTranslation()
  const zones = timeZones()
  if (!zones.includes(value.timezone)) zones.unshift(value.timezone)
  const toggle = (day: string) =>
    onChange({
      weekend: value.weekend.includes(day)
        ? value.weekend.filter((d) => d !== day)
        : WEEKDAYS.filter((d) => d === day || value.weekend.includes(d)),
    })
  return (
    <>
      <Field label={t('setup.timezone')}>
        {(id) => (
          <select
            id={id}
            value={value.timezone}
            onChange={(e) => onChange({ timezone: e.target.value })}
            className={inputClass}
          >
            {zones.map((z) => (
              <option key={z} value={z}>
                {z}
              </option>
            ))}
          </select>
        )}
      </Field>
      <fieldset>
        <legend className="block text-sm font-medium">{t('setup.weekend')}</legend>
        <div className="mt-1 flex flex-wrap gap-x-4 gap-y-1">
          {WEEKDAYS.map((d) => (
            <label key={d} className="flex items-center gap-1 text-sm">
              <input
                type="checkbox"
                checked={value.weekend.includes(d)}
                onChange={() => toggle(d)}
              />
              {t(`weekdays.${d}`)}
            </label>
          ))}
        </div>
        {weekendError && <p className="mt-1 text-sm text-red-700">{weekendError}</p>}
      </fieldset>
      <Field label={t('setup.yearStart')}>
        {(id) => (
          <select
            id={id}
            value={value.leave_year_start_month}
            onChange={(e) => onChange({ leave_year_start_month: Number(e.target.value) })}
            className={inputClass}
          >
            {Array.from({ length: 12 }, (_, i) => (
              <option key={i + 1} value={i + 1}>
                {t(`months.${i + 1}`)}
              </option>
            ))}
          </select>
        )}
      </Field>
    </>
  )
}

export function OrgStep({ value, onNext }: { value: Org; onNext: (org: Org) => void }) {
  const { t } = useTranslation()
  const [org, setOrg] = useState(value)
  const [errors, setErrors] = useState<{ name?: string; weekend?: string }>({})

  function submit(e: FormEvent) {
    e.preventDefault()
    const errs = {
      name: org.name.trim() ? undefined : t('setup.orgNameRequired'),
      weekend: org.weekend.length < 7 ? undefined : t('setup.weekendAllDays'),
    }
    setErrors(errs)
    if (!errs.name && !errs.weekend) onNext({ ...org, name: org.name.trim() })
  }

  return (
    <form onSubmit={submit} className="space-y-4" noValidate>
      <h2 className="text-lg font-medium">{t('setup.orgTitle')}</h2>
      <Field label={t('setup.orgName')} error={errors.name}>
        {(id, hint) => (
          <input
            id={id}
            aria-describedby={hint}
            aria-invalid={!!errors.name}
            autoFocus
            maxLength={120}
            value={org.name}
            onChange={(e) => setOrg({ ...org, name: e.target.value })}
            className={inputClass}
          />
        )}
      </Field>
      <CalendarFields
        value={org}
        onChange={(v) => setOrg({ ...org, ...v })}
        weekendError={errors.weekend}
      />
      <PrimaryButton type="submit">{t('common.continue')}</PrimaryButton>
    </form>
  )
}
