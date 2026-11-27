// Employees (T191, FR-042): search and filter, invite one, invite many from a CSV (previewed).
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import {
  groupsQuery,
  importUsers,
  inviteUser,
  usersQuery,
  type InviteImportResult,
  type UserFilters,
} from '../../api/admin'
import { errorMessage } from '../../api/client'
import { PrimaryButton } from '../setup/shared'
import { inputClass } from '../setup/wizard'
import { markChecklistSeen } from './checklist'

const ROLES = ['employee', 'approver', 'admin'] as const

function InviteForm({ onDone }: { onDone: (email: string) => void }) {
  const { t } = useTranslation()
  const groups = useQuery(groupsQuery)
  const [email, setEmail] = useState('')
  const [name, setName] = useState('')
  const [role, setRole] = useState<string>('employee')
  const [group, setGroup] = useState('')
  const [doj, setDoj] = useState('')
  const mutation = useMutation({
    mutationFn: () =>
      inviteUser({
        email: email.trim(),
        full_name: name.trim(),
        role,
        ...(group ? { approver_group_id: group } : {}),
        ...(doj ? { date_of_joining: doj } : {}),
      }),
    onSuccess: (u) => {
      markChecklistSeen('invite')
      setEmail('')
      setName('')
      setDoj('')
      onDone(u.email)
    },
  })
  const title = t('employees.inviteTitle')
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
          <span className="mb-1 block font-medium">{t('employees.email')}</span>
          <input
            type="email"
            className={inputClass}
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
        </label>
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('employees.fullName')}</span>
          <input
            className={inputClass}
            maxLength={120}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('employees.inviteAs')}</span>
          <select className={inputClass} value={role} onChange={(e) => setRole(e.target.value)}>
            {ROLES.map((r) => (
              <option key={r} value={r}>
                {t(`roles.${r}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('employees.group')}</span>
          <select className={inputClass} value={group} onChange={(e) => setGroup(e.target.value)}>
            <option value="">{t('employees.noGroup')}</option>
            {groups.data?.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name}
              </option>
            ))}
          </select>
        </label>
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('employees.doj')}</span>
          <input
            type="date"
            className={inputClass}
            value={doj}
            onChange={(e) => setDoj(e.target.value)}
          />
        </label>
      </div>
      {mutation.error && (
        <p role="alert" className="text-sm text-red-800">
          {errorMessage(mutation.error, t)}
        </p>
      )}
      <PrimaryButton
        type="submit"
        busy={mutation.isPending}
        disabled={!email.trim() || !name.trim()}
      >
        {t('employees.send')}
      </PrimaryButton>
    </form>
  )
}

function CsvImport({ onDone }: { onDone: (count: number) => void }) {
  const { t } = useTranslation()
  const [file, setFile] = useState<File | null>(null)
  const [preview, setPreview] = useState<InviteImportResult | null>(null)
  const dry = useMutation({ mutationFn: (f: File) => importUsers(f, true), onSuccess: setPreview })
  const apply = useMutation({
    mutationFn: (f: File) => importUsers(f, false),
    onSuccess: (r) => {
      markChecklistSeen('invite')
      setPreview(null)
      onDone(r.to_create.length)
    },
  })
  const error = dry.error ?? apply.error
  return (
    <section
      aria-labelledby="csv-invite"
      className="space-y-3 rounded border border-line bg-white p-4"
    >
      <h2 id="csv-invite" className="text-lg font-semibold">
        {t('employees.csvTitle')}
      </h2>
      <p className="text-xs text-gray-600">{t('employees.csvHelp')}</p>
      <div className="flex flex-wrap items-end gap-2">
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('employees.csvFile')}</span>
          <input
            type="file"
            accept=".csv,text/csv"
            onChange={(e) => {
              setFile(e.target.files?.[0] ?? null)
              setPreview(null)
            }}
          />
        </label>
        <button
          type="button"
          className="rounded border border-line px-3 py-2"
          disabled={!file}
          onClick={() => file && dry.mutate(file)}
        >
          {t('employees.preview')}
        </button>
      </div>
      {error && (
        <p role="alert" className="text-sm text-red-800">
          {errorMessage(error, t)}
        </p>
      )}
      {preview && (
        <div className="space-y-2 rounded bg-surface p-3 text-sm">
          <p className="font-medium">
            {t('employees.summary', {
              create: preview.to_create.length,
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
          {preview.warnings.length > 0 && (
            <ul className="text-amber-900">
              {preview.warnings.map((e) => (
                <li key={`${e.row}-${e.code}`}>
                  {t('adminHolidays.rowError', { row: e.row, message: e.message })}
                </li>
              ))}
            </ul>
          )}
          <ul>
            {preview.to_create.map((u) => (
              <li key={u.email}>
                {u.full_name} — {u.email} ({t(`roles.${u.role}`)}
                {u.approver_group ? `, ${u.approver_group}` : ''})
              </li>
            ))}
          </ul>
          {preview.errors.length === 0 && preview.to_create.length > 0 && file && (
            <PrimaryButton type="button" busy={apply.isPending} onClick={() => apply.mutate(file)}>
              {t('employees.inviteMany', { count: preview.to_create.length })}
            </PrimaryButton>
          )}
        </div>
      )}
    </section>
  )
}

export function EmployeesPage() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [filters, setFilters] = useState<UserFilters>({})
  const users = useQuery({ ...usersQuery(filters), placeholderData: keepPreviousData })
  const [done, setDone] = useState<string | null>(null)
  const set = (k: keyof UserFilters, v: string) =>
    setFilters((f) => ({ ...f, [k]: v || undefined }))
  const refresh = (msg: string) => {
    setDone(msg)
    void queryClient.invalidateQueries({ queryKey: ['admin'] })
  }
  return (
    <section className="max-w-5xl space-y-4">
      <h1 className="text-2xl font-semibold">{t('employees.title')}</h1>
      {done && (
        <p role="status" className="rounded bg-green-50 p-2 text-sm text-green-800">
          {done}
        </p>
      )}
      <div className="flex flex-wrap items-end gap-2">
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('employees.search')}</span>
          <input
            className={inputClass}
            value={filters.q ?? ''}
            onChange={(e) => set('q', e.target.value)}
          />
        </label>
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('employees.role')}</span>
          <select
            className={inputClass}
            value={filters.role ?? ''}
            onChange={(e) => set('role', e.target.value)}
          >
            <option value="">{t('employees.any')}</option>
            {ROLES.map((r) => (
              <option key={r} value={r}>
                {t(`roles.${r}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('employees.status')}</span>
          <select
            className={inputClass}
            value={filters.status ?? ''}
            onChange={(e) => set('status', e.target.value)}
          >
            <option value="">{t('employees.any')}</option>
            {(['active', 'invited', 'deactivated'] as const).map((s) => (
              <option key={s} value={s}>
                {t(`userStatus.${s}`)}
              </option>
            ))}
          </select>
        </label>
      </div>
      {users.error && <p className="text-red-700">{errorMessage(users.error, t)}</p>}
      {users.data && (
        <div className="overflow-x-auto">
          <table className="w-full rounded border border-line bg-white text-left text-sm">
            <thead className="text-gray-600">
              <tr>
                <th className="p-2">{t('employees.fullName')}</th>
                <th className="p-2">{t('employees.email')}</th>
                <th className="p-2">{t('employees.role')}</th>
                <th className="p-2">{t('employees.group')}</th>
                <th className="p-2">{t('employees.status')}</th>
              </tr>
            </thead>
            <tbody>
              {users.data.items.map((u) => (
                <tr key={u.id} className="border-t border-line">
                  <td className="p-2">
                    <Link
                      to="/admin/employees/$id"
                      params={{ id: u.id }}
                      className="text-accent underline"
                    >
                      {u.full_name}
                    </Link>
                  </td>
                  <td className="p-2 text-gray-700">{u.email}</td>
                  <td className="p-2">{t(`roles.${u.role}`)}</td>
                  <td className="p-2">{u.approver_group?.name ?? ''}</td>
                  <td className="p-2">{t(`userStatus.${u.status}`)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <InviteForm onDone={(email) => refresh(t('employees.invited', { email }))} />
      <CsvImport onDone={(count) => refresh(t('employees.importedMany', { count }))} />
    </section>
  )
}
