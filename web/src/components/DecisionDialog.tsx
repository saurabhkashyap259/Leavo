// Confirm a decision (T123, FR-023): rejection needs a comment; losing a race shows who decided.
import * as Dialog from '@radix-ui/react-dialog'
import { useMutation } from '@tanstack/react-query'
import { format, parseISO } from 'date-fns'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { decide, type ApprovalItem, type Decided, type Decision } from '../api/approvals'
import { ApiError, errorMessage, newIdempotencyKey } from '../api/client'
import { PrimaryButton } from '../routes/setup/shared'
import { inputClass } from '../routes/setup/wizard'

type AlreadyDecided = {
  status?: string
  decided_by?: { full_name?: string } | null
  decided_at?: string | null
}

export function DecisionDialog({
  item,
  decision,
  token,
  onClose,
  onDecided,
}: {
  item: ApprovalItem
  decision: Decision
  /** Token from an email link; sent so the link is retired with this decision. */
  token?: string
  onClose: () => void
  onDecided: (d: Decided) => void
}) {
  const { t } = useTranslation()
  const [comment, setComment] = useState('')
  // One key per dialog: retrying the same decision is deduplicated (FR-019).
  const [key] = useState(newIdempotencyKey)
  const reject = decision === 'reject'
  // Cancellation requests: approve accepts the cancellation, reject keeps the leave.
  const prefix = item.kind === 'cancel' ? 'approvals.cancel' : 'approvals'
  const name = item.requester.full_name
  const mutation = useMutation({
    mutationFn: () =>
      decide(
        item.kind === 'cancel' || item.kind === 'comp_off' ? item.kind : 'leave',
        item.id,
        {
          decision,
          version: item.version,
          ...(comment.trim() ? { comment: comment.trim() } : {}),
          ...(token ? { action_token: token } : {}),
        },
        key,
      ),
    onSuccess: onDecided,
  })

  const error = mutation.error
  let message: string | null = null
  if (error instanceof ApiError && error.code === 'ALREADY_DECIDED') {
    const d = (error.details ?? {}) as AlreadyDecided
    message = t('approvals.decidedBy', {
      status: t(`status.${d.status ?? 'pending'}`),
      name: d.decided_by?.full_name ?? '—',
      date: d.decided_at ? format(parseISO(d.decided_at), 'd MMM yyyy') : '',
    })
  } else if (error) {
    message = errorMessage(error, t)
  }

  return (
    <Dialog.Root open onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/30" />
        <Dialog.Content className="fixed top-1/2 left-1/2 w-[min(28rem,92vw)] -translate-x-1/2 -translate-y-1/2 space-y-4 rounded bg-white p-5 shadow-lg">
          <Dialog.Title className="text-lg font-semibold">
            {reject ? t(`${prefix}.rejectTitle`, { name }) : t(`${prefix}.approveTitle`, { name })}
          </Dialog.Title>
          <Dialog.Description className="text-sm text-gray-700">
            {item.leave_type.name} · {t('approvals.days', { count: item.days })}
          </Dialog.Description>
          <div>
            <label htmlFor="decision-comment" className="mb-1 block text-sm font-medium">
              {reject ? t('approvals.commentRequired') : t('approvals.comment')}
            </label>
            <textarea
              id="decision-comment"
              className={inputClass}
              rows={3}
              maxLength={500}
              value={comment}
              onChange={(e) => setComment(e.target.value)}
            />
          </div>
          {message && (
            <p role="alert" className="rounded bg-red-50 p-2 text-sm text-red-800">
              {message}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Dialog.Close className="rounded border border-line px-4 py-2">
              {t('common.cancel')}
            </Dialog.Close>
            <PrimaryButton
              type="button"
              busy={mutation.isPending}
              disabled={(reject && comment.trim() === '') || mutation.isSuccess}
              onClick={() => mutation.mutate()}
            >
              {reject ? t(`${prefix}.confirmReject`) : t(`${prefix}.confirmApprove`)}
            </PrimaryButton>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
