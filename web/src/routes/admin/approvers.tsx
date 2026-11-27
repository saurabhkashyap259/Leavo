// Approver groups (T191, FR-021): each group's approvers decide the requests of the employees
// assigned to it (assignment is on the employee's page).
import * as Dialog from '@radix-ui/react-dialog'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import {
  createGroup,
  deleteGroup,
  groupsQuery,
  updateGroup,
  usersQuery,
  type ApproverGroup,
} from '../../api/admin'
import { errorMessage } from '../../api/client'
import { PrimaryButton } from '../setup/shared'
import { inputClass } from '../setup/wizard'
import { markChecklistSeen } from './checklist'

/** Approvers and admins who may be members. */
function useCandidates() {
  const approvers = useQuery(usersQuery({ role: 'approver' }))
  const admins = useQuery(usersQuery({ role: 'admin' }))
  return [...(admins.data?.items ?? []), ...(approvers.data?.items ?? [])].filter(
    (u) => u.status !== 'deactivated',
  )
}

function GroupForm({
  group,
  onDone,
  onCancel,
}: {
  group?: ApproverGroup
  onDone: () => void
  onCancel?: () => void
}) {
  const { t } = useTranslation()
  const candidates = useCandidates()
  const [name, setName] = useState(group?.name ?? '')
  const [members, setMembers] = useState<Set<string>>(new Set(group?.members.map((m) => m.id)))
  const mutation = useMutation({
    mutationFn: () => {
      const body = { name: name.trim(), member_ids: [...members] }
      return group ? updateGroup(group.id, body) : createGroup(body)
    },
    onSuccess: () => {
      markChecklistSeen('approvers')
      if (!group) {
        setName('')
        setMembers(new Set())
      }
      onDone()
    },
  })
  const title = group ? t('approvers.editTitle', { name: group.name }) : t('approvers.newTitle')
  const toggle = (id: string) =>
    setMembers((m) => {
      const next = new Set(m)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
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
      <label className="block text-sm">
        <span className="mb-1 block font-medium">{t('adminHolidays.name')}</span>
        <input
          className={inputClass}
          maxLength={80}
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
      </label>
      <fieldset className="space-y-1 text-sm">
        <legend className="mb-1 font-medium">{t('approvers.members')}</legend>
        {candidates.length === 0 && <p className="text-gray-600">{t('approvers.noCandidates')}</p>}
        {candidates.map((u) => (
          <div key={u.id} className="flex items-center gap-2">
            <label className="flex items-center gap-2">
              <input type="checkbox" checked={members.has(u.id)} onChange={() => toggle(u.id)} />
              {u.full_name}
            </label>
            <span className="text-xs text-gray-600">{t(`roles.${u.role}`)}</span>
          </div>
        ))}
      </fieldset>
      {mutation.error && (
        <p role="alert" className="text-sm text-red-800">
          {errorMessage(mutation.error, t)}
        </p>
      )}
      <div className="flex gap-2">
        <PrimaryButton type="submit" busy={mutation.isPending} disabled={!name.trim()}>
          {group ? t('adminHolidays.save') : t('approvers.create')}
        </PrimaryButton>
        {onCancel && (
          <button type="button" className="rounded border border-line px-4 py-2" onClick={onCancel}>
            {t('common.cancel')}
          </button>
        )}
      </div>
    </form>
  )
}

function DeleteDialog({
  group,
  onClose,
  onDone,
}: {
  group: ApproverGroup
  onClose: () => void
  onDone: () => void
}) {
  const { t } = useTranslation()
  const mutation = useMutation({ mutationFn: () => deleteGroup(group.id), onSuccess: onDone })
  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/30" />
        <Dialog.Content className="fixed top-1/2 left-1/2 w-[min(28rem,92vw)] -translate-x-1/2 -translate-y-1/2 space-y-4 rounded bg-white p-5 shadow-lg">
          <Dialog.Title className="text-lg font-semibold">
            {t('adminHolidays.deleteTitle', { name: group.name })}
          </Dialog.Title>
          <Dialog.Description className="text-sm text-gray-700">
            {t('approvers.deleteHelp', { count: group.employees })}
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

export function ApproversPage() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const groups = useQuery(groupsQuery)
  const [editing, setEditing] = useState<string | null>(null)
  const [deleting, setDeleting] = useState<ApproverGroup | null>(null)
  const refresh = () => {
    setEditing(null)
    setDeleting(null)
    void queryClient.invalidateQueries({ queryKey: ['admin'] })
  }
  return (
    <section className="max-w-4xl space-y-4">
      <h1 className="text-2xl font-semibold">{t('approvers.title')}</h1>
      <p className="text-sm text-gray-700">{t('approvers.help')}</p>
      {groups.error && <p className="text-red-700">{errorMessage(groups.error, t)}</p>}
      {groups.data?.length === 0 && <p className="text-gray-700">{t('approvers.none')}</p>}
      {groups.data?.map((g) =>
        editing === g.id ? (
          <GroupForm key={g.id} group={g} onDone={refresh} onCancel={() => setEditing(null)} />
        ) : (
          <article key={g.id} className="space-y-2 rounded border border-line bg-white p-4">
            <header className="flex flex-wrap items-baseline justify-between gap-2">
              <h2 className="text-lg font-semibold">{g.name}</h2>
              <span className="text-sm text-gray-600">
                {t('approvers.employees', { count: g.employees })}
              </span>
            </header>
            <p className="text-sm">
              {g.members.length === 0
                ? t('approvers.noMembers')
                : g.members.map((m) => m.full_name).join(', ')}
            </p>
            <div className="flex gap-3 text-sm">
              <button type="button" className="underline" onClick={() => setEditing(g.id)}>
                {t('adminHolidays.edit')}
              </button>
              <button
                type="button"
                className="text-red-800 underline"
                onClick={() => setDeleting(g)}
              >
                {t('adminHolidays.delete')}
              </button>
            </div>
          </article>
        ),
      )}
      <GroupForm onDone={refresh} />
      {deleting && (
        <DeleteDialog group={deleting} onClose={() => setDeleting(null)} onDone={refresh} />
      )}
    </section>
  )
}
