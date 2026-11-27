// One employee (T191): profile, role, group and status; balances with the entries behind them;
// manual adjustment with a required reason; export and anonymise (FR-040–FR-042, FR-045).
import * as Dialog from '@radix-ui/react-dialog'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { format, parseISO } from 'date-fns'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import {
  adjustBalance,
  anonymiseUser,
  exportUser,
  groupsQuery,
  updateUser,
  userBalancesQuery,
  userQuery,
  type AdminUser,
} from '../../api/admin'
import { errorMessage } from '../../api/client'
import { BalanceCard } from '../../components/BalanceCard'
import { leaveYearOf } from '../../lib/dates'
import { PrimaryButton } from '../setup/shared'
import { inputClass } from '../setup/wizard'

const ROLES = ['employee', 'approver', 'admin'] as const

function ProfileForm({ user }: { user: AdminUser }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const groups = useQuery(groupsQuery)
  const [f, setF] = useState({
    full_name: user.full_name,
    employee_code: user.employee_code ?? '',
    department: user.department ?? '',
    date_of_joining: user.date_of_joining ?? '',
    role: user.role,
    approver_group_id: user.approver_group?.id ?? '',
  })
  const [saved, setSaved] = useState(false)
  const mutation = useMutation({
    mutationFn: () =>
      updateUser(user.id, {
        full_name: f.full_name.trim(),
        employee_code: f.employee_code.trim() || null,
        department: f.department.trim() || null,
        date_of_joining: f.date_of_joining || null,
        role: f.role,
        approver_group_id: f.approver_group_id || null,
      }),
    onSuccess: () => {
      setSaved(true)
      void queryClient.invalidateQueries({ queryKey: ['admin'] })
    },
  })
  const title = t('employee.profile')
  const submit = (e: FormEvent) => {
    e.preventDefault()
    setSaved(false)
    mutation.mutate()
  }
  const field = (k: 'full_name' | 'employee_code' | 'department', label: string) => (
    <label className="block text-sm">
      <span className="mb-1 block font-medium">{label}</span>
      <input
        className={inputClass}
        maxLength={120}
        value={f[k]}
        onChange={(e) => setF({ ...f, [k]: e.target.value })}
      />
    </label>
  )
  return (
    <form
      aria-label={title}
      onSubmit={submit}
      className="space-y-3 rounded border border-line bg-white p-4"
    >
      <h2 className="text-lg font-semibold">{title}</h2>
      <div className="grid gap-2 sm:grid-cols-2">
        {field('full_name', t('employees.fullName'))}
        {field('employee_code', t('employee.code'))}
        {field('department', t('employee.department'))}
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('employees.doj')}</span>
          <input
            type="date"
            className={inputClass}
            value={f.date_of_joining}
            onChange={(e) => setF({ ...f, date_of_joining: e.target.value })}
          />
        </label>
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('employees.role')}</span>
          <select
            className={inputClass}
            value={f.role}
            onChange={(e) => setF({ ...f, role: e.target.value })}
          >
            {ROLES.map((r) => (
              <option key={r} value={r}>
                {t(`roles.${r}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('employees.group')}</span>
          <select
            className={inputClass}
            value={f.approver_group_id}
            onChange={(e) => setF({ ...f, approver_group_id: e.target.value })}
          >
            <option value="">{t('employees.noGroup')}</option>
            {groups.data?.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name}
              </option>
            ))}
          </select>
        </label>
      </div>
      <p className="text-xs text-gray-600">{t('employee.dojHelp')}</p>
      {mutation.error && (
        <p role="alert" className="text-sm text-red-800">
          {errorMessage(mutation.error, t)}
        </p>
      )}
      {saved && <p className="text-sm text-green-800">{t('employee.saved')}</p>}
      <PrimaryButton type="submit" busy={mutation.isPending}>
        {t('adminHolidays.save')}
      </PrimaryButton>
    </form>
  )
}

function AdjustForm({
  id,
  year,
  types,
}: {
  id: string
  year: number
  types: { id: string; name: string }[]
}) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [type, setType] = useState('')
  const [delta, setDelta] = useState('')
  const [note, setNote] = useState('')
  const value = Number(delta)
  const valid =
    type && delta.trim() !== '' && value !== 0 && Number.isFinite(value) && note.trim() !== ''
  const mutation = useMutation({
    mutationFn: () =>
      adjustBalance(id, { leave_type_id: type, leave_year: year, delta: value, note: note.trim() }),
    onSuccess: () => {
      setDelta('')
      setNote('')
      void queryClient.invalidateQueries({ queryKey: ['admin', 'user', id] })
    },
  })
  const title = t('employee.adjustTitle')
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
      <div className="flex flex-wrap items-end gap-2">
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('carryForward.type')}</span>
          <select className={inputClass} value={type} onChange={(e) => setType(e.target.value)}>
            <option value="">—</option>
            {types.map((ty) => (
              <option key={ty.id} value={ty.id}>
                {ty.name}
              </option>
            ))}
          </select>
        </label>
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('employee.delta')}</span>
          <input
            type="number"
            step="0.5"
            className={inputClass}
            value={delta}
            onChange={(e) => setDelta(e.target.value)}
          />
        </label>
        <label className="block min-w-64 flex-1 text-sm">
          <span className="mb-1 block font-medium">{t('compOffs.form.reason')}</span>
          <input
            className={inputClass}
            maxLength={500}
            value={note}
            onChange={(e) => setNote(e.target.value)}
          />
        </label>
      </div>
      {mutation.error && (
        <p role="alert" className="text-sm text-red-800">
          {errorMessage(mutation.error, t)}
        </p>
      )}
      <PrimaryButton type="submit" busy={mutation.isPending} disabled={!valid}>
        {t('employee.record')}
      </PrimaryButton>
    </form>
  )
}

function AnonymiseDialog({ user, onClose }: { user: AdminUser; onClose: () => void }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const mutation = useMutation({
    mutationFn: () => anonymiseUser(user.id),
    onSuccess: () => {
      onClose()
      void queryClient.invalidateQueries({ queryKey: ['admin'] })
    },
  })
  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/30" />
        <Dialog.Content className="fixed top-1/2 left-1/2 w-[min(30rem,92vw)] -translate-x-1/2 -translate-y-1/2 space-y-4 rounded bg-white p-5 shadow-lg">
          <Dialog.Title className="text-lg font-semibold">
            {t('employee.anonymiseTitle', { name: user.full_name })}
          </Dialog.Title>
          <Dialog.Description className="text-sm text-gray-700">
            {t('employee.anonymiseHelp')}
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
              {t('employee.anonymise')}
            </PrimaryButton>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function download(name: string, data: unknown) {
  const url = URL.createObjectURL(
    new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' }),
  )
  const a = document.createElement('a')
  a.href = url
  a.download = name
  a.click()
  URL.revokeObjectURL(url)
}

export function EmployeePage({
  id,
  leaveYearStartMonth,
}: {
  id: string
  leaveYearStartMonth: number
}) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [current] = useState(() => leaveYearOf(new Date(), leaveYearStartMonth))
  const [year, setYear] = useState(current)
  const [anonymising, setAnonymising] = useState(false)
  const user = useQuery(userQuery(id))
  const balances = useQuery(userBalancesQuery(id, year))
  const status = useMutation({
    mutationFn: (s: 'active' | 'deactivated') => updateUser(id, { status: s }),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ['admin'] }),
  })
  const exporting = useMutation({
    mutationFn: () => exportUser(id),
    onSuccess: (data) => download(`leavo-${id}.json`, data),
  })
  const u = user.data
  if (user.error) return <p className="text-red-700">{errorMessage(user.error, t)}</p>
  if (!u) return <p>{t('common.loading')}</p>
  const types = (balances.data?.balances ?? []).map((b) => ({
    id: b.leave_type.id,
    name: b.leave_type.name,
  }))
  return (
    <section className="max-w-5xl space-y-6">
      <header className="flex flex-wrap items-baseline gap-3">
        <h1 className="text-2xl font-semibold">{u.full_name}</h1>
        <span className="text-gray-700">{u.email}</span>
        <span className="rounded bg-surface px-2 text-xs">{t(`userStatus.${u.status}`)}</span>
      </header>
      {(status.error ?? exporting.error) && (
        <p role="alert" className="text-sm text-red-800">
          {errorMessage(status.error ?? exporting.error, t)}
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        {u.status === 'deactivated' ? (
          <button
            type="button"
            className="rounded border border-line px-3 py-2"
            onClick={() => status.mutate('active')}
          >
            {t('employee.reactivate')}
          </button>
        ) : (
          <button
            type="button"
            className="rounded border border-red-300 px-3 py-2 text-red-700"
            onClick={() => status.mutate('deactivated')}
          >
            {t('employee.deactivate')}
          </button>
        )}
        <button
          type="button"
          className="rounded border border-line px-3 py-2"
          onClick={() => exporting.mutate()}
        >
          {t('employee.export')}
        </button>
        {u.status === 'deactivated' && !u.anonymised && (
          <button
            type="button"
            className="rounded border border-red-300 px-3 py-2 text-red-700"
            onClick={() => setAnonymising(true)}
          >
            {t('employee.anonymiseOpen')}
          </button>
        )}
      </div>
      <ProfileForm key={`${u.id}-${u.role}-${u.status}`} user={u} />
      <section className="space-y-3">
        <div className="flex items-center gap-3">
          <h2 className="text-lg font-semibold">{t('dashboard.balance.title')}</h2>
          <select
            aria-label={t('carryForward.year')}
            className="rounded border border-line px-2 py-1 text-sm"
            value={year}
            onChange={(e) => setYear(Number(e.target.value))}
          >
            {[current + 1, current, current - 1].map((y) => (
              <option key={y} value={y}>
                {y}
              </option>
            ))}
          </select>
        </div>
        <div className="grid gap-3 sm:grid-cols-3">
          {balances.data?.balances.map((b) => (
            <BalanceCard key={b.leave_type.id} balance={b} />
          ))}
        </div>
        {balances.data && (
          <div className="overflow-x-auto">
            <table
              aria-label={t('employee.ledger')}
              className="w-full rounded border border-line bg-white text-left text-sm"
            >
              <thead className="text-gray-600">
                <tr>
                  <th className="p-2">{t('employee.when')}</th>
                  <th className="p-2">{t('carryForward.type')}</th>
                  <th className="p-2">{t('employee.why')}</th>
                  <th className="p-2 text-right">{t('employee.days')}</th>
                  <th className="p-2">{t('employee.note')}</th>
                  <th className="p-2">{t('employee.by')}</th>
                </tr>
              </thead>
              <tbody>
                {balances.data.ledger.map((l) => (
                  <tr key={l.id} className="border-t border-line">
                    <td className="p-2">{format(parseISO(l.created_at), 'd MMM yyyy')}</td>
                    <td className="p-2">{l.leave_type.name}</td>
                    <td className="p-2">{t(`ledgerReason.${l.reason}`)}</td>
                    <td className="p-2 text-right">{l.delta > 0 ? `+${l.delta}` : l.delta}</td>
                    <td className="p-2 text-gray-700">{l.note}</td>
                    <td className="p-2 text-gray-700">
                      {l.created_by?.full_name ?? t('employee.system')}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
      <AdjustForm id={id} year={year} types={types} />
      {anonymising && <AnonymiseDialog user={u} onClose={() => setAnonymising(false)} />}
    </section>
  )
}
