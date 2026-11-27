// Leave policy (T087, FR-011, FR-012, FR-056): types table, edit drawer, quota-change choice,
// grants for restricted types.
import * as Dialog from '@radix-ui/react-dialog'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import {
  createLeaveType,
  grantLeaveType,
  grantsQuery,
  leaveTypesQuery,
  patchLeaveType,
  revokeGrant,
  type LeaveType,
  type LeaveTypeInput,
} from '../../api/admin'
import { ApiError, errorMessage } from '../../api/client'
import { Field } from '../setup/shared'
import { inputClass } from '../setup/wizard'

export function PolicyPage() {
  const { t } = useTranslation()
  const types = useQuery(leaveTypesQuery)
  // A type being edited, 'new' while adding one, or null.
  const [editing, setEditing] = useState<LeaveType | 'new' | null>(null)

  return (
    <section className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h1 className="text-2xl font-semibold">{t('admin.policy.title')}</h1>
        <button
          type="button"
          onClick={() => setEditing('new')}
          className="rounded bg-accent px-4 py-2 font-medium text-white"
        >
          {t('admin.policy.add')}
        </button>
      </div>
      {types.isPending && <p>{t('common.loading')}</p>}
      {types.error && (
        <p role="alert" className="text-red-700">
          {errorMessage(types.error, t)}
        </p>
      )}
      {types.data?.length === 0 && <p className="text-gray-700">{t('admin.policy.empty')}</p>}
      {types.data && types.data.length > 0 && (
        <div className="relative overflow-x-auto rounded border border-line bg-white">
          <table className="w-full text-left text-sm">
            <thead className="border-b border-line bg-surface">
              <tr>
                <th className="px-3 py-2">{t('admin.policy.code')}</th>
                <th className="px-3 py-2">{t('admin.policy.name')}</th>
                <th className="px-3 py-2">{t('admin.policy.quota')}</th>
                <th className="px-3 py-2">{t('admin.policy.availability')}</th>
                <th className="px-3 py-2">{t('admin.policy.status')}</th>
                <th className="px-3 py-2">
                  <span className="sr-only">{t('common.edit')}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {types.data.map((lt) => (
                <tr key={lt.id} className="border-b border-line last:border-0">
                  <td className="px-3 py-2">
                    <span
                      className="mr-2 inline-block h-3 w-3 rounded-full align-middle"
                      style={{ background: lt.color }}
                    />
                    {lt.code}
                  </td>
                  <td className="px-3 py-2">{lt.name}</td>
                  <td className="px-3 py-2">
                    {lt.is_comp_off ? t('admin.policy.earned') : lt.yearly_quota}
                    {lt.pending_yearly_quota != null && (
                      <span className="block text-xs text-gray-600">
                        {t('admin.policy.nextYear', { quota: lt.pending_yearly_quota })}
                      </span>
                    )}
                  </td>
                  <td className="px-3 py-2">
                    {lt.availability === 'restricted'
                      ? t('admin.policy.restricted', { count: lt.granted_count })
                      : t('admin.policy.all')}
                  </td>
                  <td className="px-3 py-2">
                    {lt.active ? t('admin.policy.active') : t('admin.policy.inactive')}
                  </td>
                  <td className="px-3 py-2 text-right">
                    <button
                      type="button"
                      onClick={() => setEditing(lt)}
                      className="rounded border border-line px-3 py-1"
                    >
                      {t('common.edit')}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {editing && (
        <EditDrawer
          key={editing === 'new' ? 'new' : editing.id}
          type={editing === 'new' ? null : editing}
          onClose={() => setEditing(null)}
        />
      )}
    </section>
  )
}

type Form = {
  name: string
  code: string
  color: string
  quota: string
  allow_half_day: boolean
  allow_negative: boolean
  negative_limit: string
  max_days: string
  notice: string
  backdate: string
  attachment: string
  availability: string
  receives_carry_forward: boolean
  active: boolean
}

const str = (n: number | null | undefined) => (n == null ? '' : String(n))
const numOrNull = (s: string) => (s.trim() === '' ? null : Number(s))

function toForm(lt: LeaveType): Form {
  return {
    name: lt.name,
    code: lt.code,
    color: lt.color,
    quota: str(lt.yearly_quota),
    allow_half_day: lt.allow_half_day,
    allow_negative: lt.allow_negative,
    negative_limit: str(lt.negative_limit),
    max_days: str(lt.max_days_per_request),
    notice: str(lt.min_notice_days),
    backdate: str(lt.max_backdate_days),
    attachment: str(lt.attachment_required_above_days),
    availability: lt.availability,
    receives_carry_forward: lt.receives_carry_forward,
    active: lt.active,
  }
}

const NEW_FORM: Form = {
  name: '',
  code: '',
  color: '#3B82F6',
  quota: '',
  allow_half_day: true,
  allow_negative: false,
  negative_limit: '0',
  max_days: '',
  notice: '0',
  backdate: '0',
  attachment: '',
  availability: 'all',
  receives_carry_forward: false,
  active: true,
}

/** Every field, for creating a type. */
function full(f: Form): LeaveTypeInput {
  return {
    name: f.name,
    code: f.code,
    color: f.color,
    yearly_quota: numOrNull(f.quota) ?? undefined,
    allow_half_day: f.allow_half_day,
    allow_negative: f.allow_negative,
    negative_limit: Number(f.negative_limit),
    max_days_per_request: numOrNull(f.max_days),
    min_notice_days: Number(f.notice),
    max_backdate_days: Number(f.backdate),
    attachment_required_above_days: numOrNull(f.attachment),
    availability: f.availability,
    receives_carry_forward: f.receives_carry_forward,
    active: f.active,
  }
}

/** Only the fields that changed (the quota is handled separately). */
function diff(lt: LeaveType, f: Form): LeaveTypeInput {
  const out: LeaveTypeInput = {}
  if (f.name !== lt.name) out.name = f.name
  if (f.code !== lt.code) out.code = f.code
  if (f.color.toUpperCase() !== lt.color.toUpperCase()) out.color = f.color
  if (f.allow_half_day !== lt.allow_half_day) out.allow_half_day = f.allow_half_day
  if (f.allow_negative !== lt.allow_negative) out.allow_negative = f.allow_negative
  if (Number(f.negative_limit) !== lt.negative_limit) out.negative_limit = Number(f.negative_limit)
  if (numOrNull(f.max_days) !== (lt.max_days_per_request ?? null))
    out.max_days_per_request = numOrNull(f.max_days)
  if (Number(f.notice) !== lt.min_notice_days) out.min_notice_days = Number(f.notice)
  if (Number(f.backdate) !== lt.max_backdate_days) out.max_backdate_days = Number(f.backdate)
  if (numOrNull(f.attachment) !== (lt.attachment_required_above_days ?? null))
    out.attachment_required_above_days = numOrNull(f.attachment)
  if (f.availability !== lt.availability) out.availability = f.availability
  if (f.receives_carry_forward !== lt.receives_carry_forward)
    out.receives_carry_forward = f.receives_carry_forward
  if (f.active !== lt.active) out.active = f.active
  return out
}

/** Edits `type`, or adds a new type when it is null. */
function EditDrawer({ type: lt, onClose }: { type: LeaveType | null; onClose: () => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [form, setForm] = useState(() => (lt ? toForm(lt) : NEW_FORM))
  const [choosing, setChoosing] = useState(false)
  const [applyFrom, setApplyFrom] = useState<'now' | 'next_year' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [fieldError, setFieldError] = useState<string | null>(null)
  const save = useMutation({
    mutationFn: (body: LeaveTypeInput) =>
      lt ? patchLeaveType(lt.id, body) : createLeaveType(body),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: leaveTypesQuery.queryKey })
      onClose()
    },
    onError: (err) => {
      setChoosing(false)
      setError(errorMessage(err, t))
      const details =
        err instanceof ApiError ? (err.details as { field?: string } | undefined) : undefined
      setFieldError(details?.field ?? null)
    },
  })
  const set = (patch: Partial<Form>) => setForm({ ...form, ...patch })
  const quotaChanged =
    lt != null && !lt.is_comp_off && numOrNull(form.quota) !== (lt.yearly_quota ?? null)

  function submit(e: FormEvent) {
    e.preventDefault()
    setError(null)
    if (!lt) {
      save.mutate(full(form))
    } else if (quotaChanged) {
      setApplyFrom(null)
      setChoosing(true)
    } else {
      save.mutate(diff(lt, form))
    }
  }

  const text = (
    label: string,
    key: keyof Form,
    field: string,
    inputMode?: 'decimal' | 'numeric',
  ) => (
    <Field label={label} error={fieldError === field ? (error ?? undefined) : undefined}>
      {(id, hint) => (
        <input
          id={id}
          aria-describedby={hint}
          inputMode={inputMode}
          value={form[key] as string}
          onChange={(e) => set({ [key]: e.target.value } as Partial<Form>)}
          className={inputClass}
        />
      )}
    </Field>
  )
  const check = (label: string, key: keyof Form) => (
    <label className="flex items-center gap-2 text-sm">
      <input
        type="checkbox"
        checked={form[key] as boolean}
        onChange={(e) => set({ [key]: e.target.checked } as Partial<Form>)}
      />
      {label}
    </label>
  )

  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/30" />
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed inset-y-0 right-0 w-full max-w-md overflow-y-auto bg-white p-6 shadow-lg"
        >
          <Dialog.Title className="mb-4 text-lg font-semibold">
            {lt ? t('admin.policy.editTitle', { name: lt.name }) : t('admin.policy.add')}
          </Dialog.Title>
          <form onSubmit={submit} className="space-y-4" noValidate>
            {text(t('admin.policy.fields.name'), 'name', 'name')}
            {text(t('admin.policy.fields.code'), 'code', 'code')}
            <Field
              label={t('admin.policy.fields.color')}
              error={fieldError === 'color' ? (error ?? undefined) : undefined}
            >
              {(id) => (
                <input
                  id={id}
                  type="color"
                  value={form.color}
                  onChange={(e) => set({ color: e.target.value })}
                  className="mt-1 h-9 w-16"
                />
              )}
            </Field>
            {!lt?.is_comp_off &&
              text(t('admin.policy.fields.quota'), 'quota', 'yearly_quota', 'decimal')}
            {check(t('admin.policy.fields.halfDay'), 'allow_half_day')}
            {check(t('admin.policy.fields.negative'), 'allow_negative')}
            {form.allow_negative &&
              text(
                t('admin.policy.fields.negativeLimit'),
                'negative_limit',
                'negative_limit',
                'decimal',
              )}
            {text(t('admin.policy.fields.maxDays'), 'max_days', 'max_days_per_request', 'decimal')}
            {text(t('admin.policy.fields.notice'), 'notice', 'min_notice_days', 'numeric')}
            {text(t('admin.policy.fields.backdate'), 'backdate', 'max_backdate_days', 'numeric')}
            {text(
              t('admin.policy.fields.attachment'),
              'attachment',
              'attachment_required_above_days',
              'decimal',
            )}
            <Field label={t('admin.policy.fields.availability')}>
              {(id) => (
                <select
                  id={id}
                  value={form.availability}
                  onChange={(e) => set({ availability: e.target.value })}
                  className={inputClass}
                >
                  <option value="all">{t('admin.policy.all')}</option>
                  <option value="restricted">
                    {t('admin.policy.restricted', { count: lt?.granted_count ?? 0 })}
                  </option>
                </select>
              )}
            </Field>
            {check(t('admin.policy.fields.carry'), 'receives_carry_forward')}
            {check(t('admin.policy.fields.active'), 'active')}
            {error && !fieldError && (
              <p role="alert" className="text-sm text-red-700">
                {error}
              </p>
            )}
            <div className="flex gap-2">
              <button
                type="submit"
                disabled={save.isPending}
                className="rounded bg-accent px-4 py-2 font-medium text-white"
              >
                {t('common.save')}
              </button>
              <Dialog.Close className="rounded border border-line px-4 py-2">
                {t('common.cancel')}
              </Dialog.Close>
            </div>
          </form>
          {lt?.availability === 'restricted' && <Grants typeId={lt.id} />}

          <Dialog.Root open={choosing} onOpenChange={setChoosing}>
            <Dialog.Portal>
              <Dialog.Overlay className="fixed inset-0 bg-black/30" />
              <Dialog.Content
                role="alertdialog"
                aria-describedby={undefined}
                className="fixed top-1/2 left-1/2 w-[min(28rem,92vw)] -translate-x-1/2 -translate-y-1/2 rounded-lg bg-white p-6 shadow-lg"
              >
                <Dialog.Title className="text-lg font-semibold">
                  {t('admin.policy.quotaChoiceTitle')}
                </Dialog.Title>
                <p className="mt-2 text-sm text-gray-700">
                  {t('admin.policy.quotaChoiceHelp', { old: lt?.yearly_quota, new: form.quota })}
                </p>
                <fieldset className="mt-4 space-y-2">
                  {(['now', 'next_year'] as const).map((v) => (
                    <label key={v} className="flex items-start gap-2 text-sm">
                      <input
                        type="radio"
                        name="apply_from"
                        className="mt-1"
                        checked={applyFrom === v}
                        onChange={() => setApplyFrom(v)}
                      />
                      {v === 'now' ? t('admin.policy.applyNow') : t('admin.policy.applyNextYear')}
                    </label>
                  ))}
                </fieldset>
                <div className="mt-4 flex gap-2">
                  <button
                    type="button"
                    disabled={!applyFrom || save.isPending}
                    onClick={() =>
                      save.mutate({
                        ...(lt ? diff(lt, form) : full(form)),
                        yearly_quota: Number(form.quota),
                        apply_from: applyFrom,
                      })
                    }
                    className="rounded bg-accent px-4 py-2 font-medium text-white disabled:opacity-60"
                  >
                    {t('admin.policy.applyChange')}
                  </button>
                  <Dialog.Close className="rounded border border-line px-4 py-2">
                    {t('common.cancel')}
                  </Dialog.Close>
                </div>
              </Dialog.Content>
            </Dialog.Portal>
          </Dialog.Root>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function Grants({ typeId }: { typeId: string }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const grants = useQuery(grantsQuery(typeId))
  const [email, setEmail] = useState('')
  const [error, setError] = useState<string | null>(null)
  const refresh = () =>
    Promise.all([
      qc.invalidateQueries({ queryKey: grantsQuery(typeId).queryKey }),
      qc.invalidateQueries({ queryKey: leaveTypesQuery.queryKey }),
    ])
  const add = useMutation({
    mutationFn: () => grantLeaveType(typeId, email.trim()),
    onSuccess: async () => {
      setEmail('')
      setError(null)
      await refresh()
    },
    onError: (e) => setError(errorMessage(e, t)),
  })
  const revoke = useMutation({
    mutationFn: (userId: string) => revokeGrant(typeId, userId),
    onSuccess: refresh,
  })

  return (
    <section className="mt-8 space-y-3 border-t border-line pt-4">
      <h3 className="font-medium">{t('admin.policy.grants')}</h3>
      {grants.data?.length === 0 && (
        <p className="text-sm text-gray-600">{t('admin.policy.noGrants')}</p>
      )}
      <ul className="space-y-1">
        {grants.data?.map((g) => (
          <li key={g.id} className="flex items-center justify-between text-sm">
            <span>
              {g.full_name} <span className="text-gray-600">({g.email})</span>
            </span>
            <button
              type="button"
              aria-label={t('admin.policy.revokeLabel', { name: g.full_name })}
              onClick={() => revoke.mutate(g.user_id)}
              className="text-red-700 underline"
            >
              {t('admin.policy.revoke')}
            </button>
          </li>
        ))}
      </ul>
      <form
        onSubmit={(e) => {
          e.preventDefault()
          if (email.trim()) add.mutate()
        }}
        className="flex items-end gap-2"
      >
        <div className="flex-1">
          <Field label={t('admin.policy.grantEmail')} error={error ?? undefined}>
            {(id, hint) => (
              <input
                id={id}
                type="email"
                aria-describedby={hint}
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                className={inputClass}
              />
            )}
          </Field>
        </div>
        <button
          type="submit"
          disabled={add.isPending}
          className="mb-px rounded border border-line px-3 py-2"
        >
          {t('admin.policy.grant')}
        </button>
      </form>
    </section>
  )
}
