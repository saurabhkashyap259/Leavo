// From/To dates with an optional half day on the first and last day (FR-015).
import { useTranslation } from 'react-i18next'
import type { DayHalf } from '../api/leaves'
import { inputClass } from '../routes/setup/wizard'

export type DateRange = {
  from: string
  to: string
  startHalf: DayHalf | null
  endHalf: DayHalf | null
}

function HalfSelect({
  id,
  label,
  value,
  onChange,
}: {
  id: string
  label: string
  value: DayHalf | null
  onChange: (v: DayHalf | null) => void
}) {
  const { t } = useTranslation()
  return (
    <div>
      <label htmlFor={id} className="mb-1 block text-sm font-medium">
        {label}
      </label>
      <select
        id={id}
        className={inputClass}
        value={value ?? ''}
        onChange={(e) => onChange((e.target.value || null) as DayHalf | null)}
      >
        <option value="">{t('leave.apply.fullDay')}</option>
        <option value="first">{t('leave.apply.firstHalf')}</option>
        <option value="second">{t('leave.apply.secondHalf')}</option>
      </select>
    </div>
  )
}

export function DateRangePicker({
  value,
  onChange,
}: {
  value: DateRange
  onChange: (v: DateRange) => void
}) {
  const { t } = useTranslation()
  const set = (patch: Partial<DateRange>) => onChange({ ...value, ...patch })
  const singleDay = value.from !== '' && value.from === value.to
  return (
    <fieldset className="grid gap-3 sm:grid-cols-2">
      <div>
        <label htmlFor="leave-from" className="mb-1 block text-sm font-medium">
          {t('leave.apply.from')}
        </label>
        <input
          id="leave-from"
          type="date"
          className={inputClass}
          value={value.from}
          onChange={(e) => {
            const from = e.target.value
            // Moving the start past the end drags the end along.
            set({ from, to: value.to === '' || value.to < from ? from : value.to })
          }}
        />
      </div>
      <div>
        <label htmlFor="leave-to" className="mb-1 block text-sm font-medium">
          {t('leave.apply.to')}
        </label>
        <input
          id="leave-to"
          type="date"
          className={inputClass}
          value={value.to}
          min={value.from || undefined}
          onChange={(e) => set({ to: e.target.value })}
        />
      </div>
      {value.from !== '' &&
        (singleDay ? (
          <HalfSelect
            id="leave-half"
            label={t('leave.apply.halfDay')}
            value={value.startHalf}
            onChange={(startHalf) => set({ startHalf, endHalf: null })}
          />
        ) : (
          <>
            <HalfSelect
              id="leave-start-half"
              label={t('leave.apply.firstDay')}
              value={value.startHalf}
              onChange={(startHalf) => set({ startHalf })}
            />
            <HalfSelect
              id="leave-end-half"
              label={t('leave.apply.lastDay')}
              value={value.endHalf}
              onChange={(endHalf) => set({ endHalf })}
            />
          </>
        ))}
    </fieldset>
  )
}
