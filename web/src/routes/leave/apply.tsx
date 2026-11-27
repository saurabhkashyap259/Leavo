// Apply for leave (T108, US3): type with what is available, dates with half days, live
// working-day count via /leaves/preview, reason, notify members, supporting document.
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { format, parseISO } from 'date-fns'
import { useRef, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { errorMessage, newIdempotencyKey } from '../../api/client'
import {
  balancesQuery,
  createLeave,
  previewLeave,
  type LeaveInput,
  type Preview,
  type Uploaded,
} from '../../api/leaves'
import { meQuery } from '../../api/me'
import { AttachmentUpload } from '../../components/AttachmentUpload'
import { DateRangePicker, type DateRange } from '../../components/DateRangePicker'
import { NotifyPicker, type NotifyEntry } from '../../components/NotifyPicker'
import { PrimaryButton } from '../setup/shared'
import { inputClass } from '../setup/wizard'

const longDate = (d: string) => format(parseISO(d), 'EEE d MMM yyyy')

function PreviewPanel({ preview }: { preview: Preview }) {
  const { t } = useTranslation()
  const years = preview.by_leave_year
  return (
    <div className="space-y-2 rounded border border-line bg-surface p-3 text-sm" aria-live="polite">
      <p className="text-base font-semibold">
        {t('leave.apply.workingDays', { count: preview.days })}
      </p>
      {years.length === 1 && (
        <p>{t('leave.apply.leftAfter', { count: years[0].available_after })}</p>
      )}
      {years.length > 1 && (
        <ul>
          {years.map((y) => (
            <li key={y.leave_year}>
              {t('leave.apply.yearLine', {
                year: y.leave_year,
                count: y.days,
                left: y.available_after,
              })}
            </li>
          ))}
        </ul>
      )}
      {preview.excluded.length > 0 && (
        <div>
          <p className="font-medium">{t('leave.apply.notCounted')}</p>
          <ul aria-label={t('leave.apply.notCounted')} className="text-gray-700">
            {preview.excluded.map((e) => (
              <li key={e.date}>
                {longDate(e.date)} — {e.reason === 'holiday' ? e.name : t('leave.apply.weekend')}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  )
}

export function ApplyLeavePage({ onSubmitted }: { onSubmitted?: (id: string) => void }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const balances = useQuery(balancesQuery)
  const me = useQuery(meQuery)
  const [typeId, setTypeId] = useState('')
  const [range, setRange] = useState<DateRange>({
    from: '',
    to: '',
    startHalf: null,
    endHalf: null,
  })
  const [reason, setReason] = useState('')
  const [notify, setNotify] = useState<NotifyEntry[]>([])
  const [attachment, setAttachment] = useState<Uploaded | null>(null)
  // One key per submission: a retry of the same details reuses it (FR-019).
  const submission = useRef<{ key: string; body: string } | null>(null)

  const body: LeaveInput = {
    leave_type_id: typeId,
    from_date: range.from,
    to_date: range.to,
    start_half: range.startHalf,
    end_half: range.endHalf,
    reason,
    attachment_id: attachment?.id ?? null,
    notify: notify.map((n) => (n.user_id ? { user_id: n.user_id } : { email: n.email })),
  }
  const ready = typeId !== '' && range.from !== '' && range.to !== ''
  // The preview ignores the reason text so typing it does not re-run the check.
  const previewBody = { ...body, reason: '' }
  const preview = useQuery({
    queryKey: ['leaves', 'preview', previewBody],
    queryFn: ({ signal }) => previewLeave(previewBody, signal),
    enabled: ready,
    placeholderData: keepPreviousData,
  })

  const submit = useMutation({
    mutationFn: () => {
      const serial = JSON.stringify(body)
      if (submission.current?.body !== serial) {
        submission.current = { key: newIdempotencyKey(), body: serial }
      }
      return createLeave(body, submission.current.key)
    },
    onSuccess: (created) => {
      submission.current = null
      setRange({ from: '', to: '', startHalf: null, endHalf: null })
      setReason('')
      setNotify([])
      setAttachment(null)
      void queryClient.invalidateQueries({ queryKey: ['me'] })
      onSubmitted?.(created.id)
    },
  })

  const errors = (ready && preview.data?.errors) || []
  const attachmentError = errors.find((e) => e.code === 'ATTACHMENT_REQUIRED')
  const otherErrors = errors.filter((e) => e.code !== 'ATTACHMENT_REQUIRED')
  const canSubmit =
    ready &&
    preview.data !== undefined &&
    !preview.isPlaceholderData &&
    errors.length === 0 &&
    reason.trim() !== '' &&
    !submit.isPending

  const onSubmit = (e: FormEvent) => {
    e.preventDefault()
    if (canSubmit) submit.mutate()
  }

  return (
    <section className="max-w-2xl space-y-4">
      <h1 className="text-2xl font-semibold">{t('leave.apply.title')}</h1>
      {submit.isSuccess && (
        <p role="status" className="rounded bg-green-50 p-3 text-green-800">
          {t('leave.apply.submitted')}
        </p>
      )}
      <form onSubmit={onSubmit} className="space-y-4">
        <div>
          <label htmlFor="leave-type" className="mb-1 block text-sm font-medium">
            {t('leave.apply.type')}
          </label>
          <select
            id="leave-type"
            className={inputClass}
            value={typeId}
            onChange={(e) => setTypeId(e.target.value)}
          >
            <option value="">{t('leave.apply.choose')}</option>
            {balances.data?.map((b) => (
              <option key={b.leave_type.id} value={b.leave_type.id}>
                {t('leave.apply.option', { name: b.leave_type.name, available: b.available })}
              </option>
            ))}
          </select>
        </div>

        <DateRangePicker value={range} onChange={setRange} />

        {ready && preview.isPending && <p className="text-sm">{t('leave.apply.checking')}</p>}
        {ready && preview.data && <PreviewPanel preview={preview.data} />}
        {ready && preview.error && (
          <p className="text-sm text-red-700">{errorMessage(preview.error, t)}</p>
        )}
        {otherErrors.length > 0 && (
          <ul className="list-disc pl-5 text-sm text-red-700">
            {otherErrors.map((e) => (
              <li key={e.code}>{t(`errors.${e.code}`, { defaultValue: e.message })}</li>
            ))}
          </ul>
        )}

        <div>
          <label htmlFor="leave-reason" className="mb-1 block text-sm font-medium">
            {t('leave.apply.reason')}
          </label>
          <textarea
            id="leave-reason"
            className={inputClass}
            rows={3}
            maxLength={500}
            value={reason}
            onChange={(e) => setReason(e.target.value)}
          />
          <p className="text-right text-xs text-gray-600">
            {t('leave.apply.reasonCount', { count: reason.length })}
          </p>
        </div>

        <NotifyPicker value={notify} onChange={setNotify} lastUsed={me.data?.last_notify ?? []} />

        <AttachmentUpload
          value={attachment}
          onChange={setAttachment}
          required={
            attachmentError
              ? t('errors.ATTACHMENT_REQUIRED', { defaultValue: attachmentError.message })
              : null
          }
        />

        {submit.error && (
          <p role="alert" className="rounded bg-red-50 p-3 text-red-800">
            {errorMessage(submit.error, t)}
          </p>
        )}
        <PrimaryButton type="submit" disabled={!canSubmit} busy={submit.isPending}>
          {submit.isPending ? t('leave.apply.submitting') : t('leave.apply.submit')}
        </PrimaryButton>
      </form>
    </section>
  )
}
