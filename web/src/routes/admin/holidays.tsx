// Admin holidays (T165, FR-034, FR-035): the list per leave year with add, edit and delete, and
// imports from a country pack, a CSV file or the previous year — always previewed first.
import * as Dialog from '@radix-ui/react-dialog'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { format, parseISO } from 'date-fns'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { errorMessage } from '../../api/client'
import {
  addHoliday,
  deleteHoliday,
  holidaysQuery,
  importHolidays,
  packsQuery,
  updateHoliday,
  type Holiday,
  type ImportFrom,
  type ImportResult,
} from '../../api/holidays'
import { leaveYearOf } from '../../lib/dates'
import { PrimaryButton } from '../setup/shared'
import { inputClass } from '../setup/wizard'

const long = (d: string) => format(parseISO(d), 'EEE d MMM yyyy')

type Fields = { date: string; name: string; kind: 'mandatory' | 'optional'; location: string }

function HolidayFields({ value, onChange }: { value: Fields; onChange: (f: Fields) => void }) {
  const { t } = useTranslation()
  return (
    <div className="flex flex-wrap items-end gap-2">
      <label className="block text-sm">
        <span className="mb-1 block font-medium">{t('adminHolidays.date')}</span>
        <input
          type="date"
          className={inputClass}
          value={value.date}
          onChange={(e) => onChange({ ...value, date: e.target.value })}
        />
      </label>
      <label className="block text-sm">
        <span className="mb-1 block font-medium">{t('adminHolidays.name')}</span>
        <input
          className={inputClass}
          maxLength={120}
          value={value.name}
          onChange={(e) => onChange({ ...value, name: e.target.value })}
        />
      </label>
      <label className="block text-sm">
        <span className="mb-1 block font-medium">{t('adminHolidays.kind')}</span>
        <select
          className={inputClass}
          value={value.kind}
          onChange={(e) =>
            onChange({ ...value, kind: e.target.value === 'optional' ? 'optional' : 'mandatory' })
          }
        >
          <option value="mandatory">{t('adminHolidays.mandatory')}</option>
          <option value="optional">{t('holidays.optional')}</option>
        </select>
      </label>
      <label className="block text-sm">
        <span className="mb-1 block font-medium">{t('adminHolidays.location')}</span>
        <input
          className={inputClass}
          maxLength={120}
          value={value.location}
          onChange={(e) => onChange({ ...value, location: e.target.value })}
        />
      </label>
    </div>
  )
}

const EMPTY: Fields = { date: '', name: '', kind: 'mandatory', location: '' }

function AddForm({ onDone }: { onDone: (msg: string) => void }) {
  const { t } = useTranslation()
  const [f, setF] = useState<Fields>(EMPTY)
  const mutation = useMutation({
    mutationFn: () =>
      addHoliday({
        date: f.date,
        name: f.name.trim(),
        kind: f.kind,
        ...(f.location.trim() ? { location: f.location.trim() } : {}),
      }),
    onSuccess: () => {
      setF(EMPTY)
      onDone(t('adminHolidays.added'))
    },
  })
  const title = t('adminHolidays.addTitle')
  const submit = (e: FormEvent) => {
    e.preventDefault()
    mutation.mutate()
  }
  return (
    <form
      aria-label={title}
      onSubmit={submit}
      className="space-y-3 rounded border border-line bg-white p-4"
    >
      <h2 className="text-lg font-semibold">{title}</h2>
      <HolidayFields value={f} onChange={setF} />
      {mutation.error && (
        <p role="alert" className="text-sm text-red-800">
          {errorMessage(mutation.error, t)}
        </p>
      )}
      <PrimaryButton type="submit" busy={mutation.isPending} disabled={!f.date || !f.name.trim()}>
        {t('adminHolidays.add')}
      </PrimaryButton>
    </form>
  )
}

function EditDialog({
  h,
  onClose,
  onDone,
}: {
  h: Holiday
  onClose: () => void
  onDone: () => void
}) {
  const { t } = useTranslation()
  const [f, setF] = useState<Fields>({
    date: h.date,
    name: h.name,
    kind: h.kind === 'optional' ? 'optional' : 'mandatory',
    location: h.location ?? '',
  })
  const mutation = useMutation({
    mutationFn: () =>
      updateHoliday(h.id, {
        date: f.date,
        name: f.name.trim(),
        kind: f.kind,
        location: f.location.trim() || null,
      }),
    onSuccess: onDone,
  })
  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/30" />
        <Dialog.Content className="fixed top-1/2 left-1/2 w-[min(40rem,94vw)] -translate-x-1/2 -translate-y-1/2 space-y-4 rounded bg-white p-5 shadow-lg">
          <Dialog.Title className="text-lg font-semibold">
            {t('adminHolidays.editTitle', { name: h.name })}
          </Dialog.Title>
          <Dialog.Description className="text-sm text-gray-700">
            {t('adminHolidays.sideEffects')}
          </Dialog.Description>
          <HolidayFields value={f} onChange={setF} />
          {mutation.error && (
            <p role="alert" className="text-sm text-red-800">
              {errorMessage(mutation.error, t)}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Dialog.Close className="rounded border border-line px-4 py-2">
              {t('common.cancel')}
            </Dialog.Close>
            <PrimaryButton
              type="button"
              busy={mutation.isPending}
              onClick={() => mutation.mutate()}
            >
              {t('adminHolidays.save')}
            </PrimaryButton>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function DeleteDialog({
  h,
  onClose,
  onDone,
}: {
  h: Holiday
  onClose: () => void
  onDone: () => void
}) {
  const { t } = useTranslation()
  const mutation = useMutation({ mutationFn: () => deleteHoliday(h.id), onSuccess: onDone })
  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/30" />
        <Dialog.Content className="fixed top-1/2 left-1/2 w-[min(28rem,92vw)] -translate-x-1/2 -translate-y-1/2 space-y-4 rounded bg-white p-5 shadow-lg">
          <Dialog.Title className="text-lg font-semibold">
            {t('adminHolidays.deleteTitle', { name: h.name })}
          </Dialog.Title>
          <Dialog.Description className="text-sm text-gray-700">
            {h.kind === 'mandatory'
              ? t('adminHolidays.deleteMandatory')
              : t('adminHolidays.deleteOptional')}
          </Dialog.Description>
          {mutation.error && (
            <p role="alert" className="text-sm text-red-800">
              {errorMessage(mutation.error, t)}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Dialog.Close className="rounded border border-line px-4 py-2">
              {t('common.cancel')}
            </Dialog.Close>
            <PrimaryButton
              type="button"
              busy={mutation.isPending}
              onClick={() => mutation.mutate()}
            >
              {t('adminHolidays.delete')}
            </PrimaryButton>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function ImportBox({ year, onDone }: { year: number; onDone: (msg: string) => void }) {
  const { t } = useTranslation()
  const packs = useQuery(packsQuery)
  const [pack, setPack] = useState('')
  const [file, setFile] = useState<File | null>(null)
  const [source, setSource] = useState<ImportFrom | null>(null)
  const [preview, setPreview] = useState<ImportResult | null>(null)
  const dry = useMutation({
    mutationFn: (from: ImportFrom) => importHolidays(from, true),
    onSuccess: setPreview,
  })
  const apply = useMutation({
    mutationFn: () => importHolidays(source!, false),
    onSuccess: (r) => {
      setPreview(null)
      setSource(null)
      onDone(t('adminHolidays.imported', { count: r.added.length }))
    },
  })
  const run = (from: ImportFrom) => {
    setSource(from)
    setPreview(null)
    dry.mutate(from)
  }
  const title = t('adminHolidays.importTitle')
  const error = dry.error ?? apply.error
  return (
    <section
      aria-labelledby="import-holidays"
      className="space-y-3 rounded border border-line bg-white p-4"
    >
      <h2 id="import-holidays" className="text-lg font-semibold">
        {title}
      </h2>
      <div className="flex flex-wrap items-end gap-2">
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('adminHolidays.pack')}</span>
          <select className={inputClass} value={pack} onChange={(e) => setPack(e.target.value)}>
            <option value="">—</option>
            {packs.data?.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          className="rounded border border-line px-3 py-2"
          disabled={!pack}
          onClick={() => run({ pack })}
        >
          {t('adminHolidays.previewPack')}
        </button>
      </div>
      <div className="flex flex-wrap items-end gap-2">
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('adminHolidays.csv')}</span>
          <input
            type="file"
            accept=".csv,text/csv"
            onChange={(e) => setFile(e.target.files?.[0] ?? null)}
          />
        </label>
        <button
          type="button"
          className="rounded border border-line px-3 py-2"
          disabled={!file}
          onClick={() => file && run({ file })}
        >
          {t('adminHolidays.previewFile')}
        </button>
      </div>
      <p className="text-xs text-gray-600">{t('adminHolidays.csvHelp')}</p>
      <button
        type="button"
        className="rounded border border-line px-3 py-2"
        onClick={() => run({ copy_from_year: year - 1, to_year: year })}
      >
        {t('adminHolidays.copy', { from: year - 1, to: year })}
      </button>
      {error && (
        <p role="alert" className="text-sm text-red-800">
          {errorMessage(error, t)}
        </p>
      )}
      {preview && (
        <div className="space-y-2 rounded bg-surface p-3 text-sm">
          <p className="font-medium">
            {t('adminHolidays.summary', {
              added: preview.added.length,
              skipped: preview.skipped.length,
            })}
          </p>
          {preview.errors.length > 0 && (
            <ul className="text-red-800">
              {preview.errors.map((e) => (
                <li key={`${e.row}-${e.code}`}>
                  {t('adminHolidays.rowError', { row: e.row, message: e.message })}
                </li>
              ))}
            </ul>
          )}
          {preview.added.length > 0 && (
            <ul>
              {preview.added.map((a) => (
                <li key={`${a.date}-${a.name}`}>
                  {long(a.date)} — {a.name}
                  {a.kind === 'optional' && ` (${t('holidays.optional')})`}
                </li>
              ))}
            </ul>
          )}
          {preview.errors.length === 0 && preview.added.length > 0 && (
            <PrimaryButton type="button" busy={apply.isPending} onClick={() => apply.mutate()}>
              {t('adminHolidays.import', { count: preview.added.length })}
            </PrimaryButton>
          )}
        </div>
      )}
    </section>
  )
}

export function AdminHolidaysPage({ leaveYearStartMonth }: { leaveYearStartMonth: number }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [current] = useState(() => leaveYearOf(new Date(), leaveYearStartMonth))
  const [year, setYear] = useState(current)
  const holidays = useQuery(holidaysQuery(year))
  const [done, setDone] = useState<string | null>(null)
  const [editing, setEditing] = useState<Holiday | null>(null)
  const [deleting, setDeleting] = useState<Holiday | null>(null)
  const refresh = (msg: string) => {
    setDone(msg)
    setEditing(null)
    setDeleting(null)
    void queryClient.invalidateQueries({ queryKey: ['holidays'] })
  }
  return (
    <section className="max-w-4xl space-y-4">
      <h1 className="text-2xl font-semibold">{t('adminHolidays.title')}</h1>
      <div role="tablist" className="flex gap-1">
        {[current - 1, current, current + 1].map((y) => (
          <button
            key={y}
            role="tab"
            type="button"
            aria-selected={y === year}
            onClick={() => setYear(y)}
            className={`rounded px-3 py-1 ${y === year ? 'bg-accent text-white' : 'border border-line'}`}
          >
            {y}
          </button>
        ))}
      </div>
      {done && (
        <p role="status" className="rounded bg-green-50 p-2 text-sm text-green-800">
          {done}
        </p>
      )}
      {holidays.data?.length === 0 && <p className="text-gray-700">{t('holidays.none')}</p>}
      {holidays.data && holidays.data.length > 0 && (
        <div className="overflow-x-auto">
          <table className="w-full rounded border border-line bg-white text-left text-sm">
            <tbody>
              {holidays.data.map((h) => (
                <tr key={h.id} className="border-t border-line">
                  <td className="p-2">{long(h.date)}</td>
                  <td className="p-2">{h.name}</td>
                  <td className="p-2">
                    {h.kind === 'optional' ? t('holidays.optional') : t('adminHolidays.mandatory')}
                  </td>
                  <td className="p-2 text-gray-600">{h.location}</td>
                  <td className="p-2 text-right">
                    <button type="button" className="mr-3 underline" onClick={() => setEditing(h)}>
                      {t('adminHolidays.edit')}
                    </button>
                    <button
                      type="button"
                      className="text-red-800 underline"
                      onClick={() => setDeleting(h)}
                    >
                      {t('adminHolidays.delete')}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <AddForm onDone={refresh} />
      <ImportBox year={year} onDone={refresh} />
      {editing && (
        <EditDialog
          h={editing}
          onClose={() => setEditing(null)}
          onDone={() => refresh(t('adminHolidays.saved'))}
        />
      )}
      {deleting && (
        <DeleteDialog
          h={deleting}
          onClose={() => setDeleting(null)}
          onDone={() => refresh(t('adminHolidays.deleted'))}
        />
      )}
    </section>
  )
}
