// Confirm cancelling a request (T143, FR-020). The wording says what will happen: a pending
// request is withdrawn, approved leave before its start is cancelled with every day returned,
// and leave already under way needs an approver, returning only days from today on.
import * as Dialog from '@radix-ui/react-dialog'
import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { errorMessage } from '../api/client'
import { cancelLeave, type CancelResult, type HistoryItem } from '../api/leaves'
import type { CancelKind } from '../lib/cancel'
import { shortDay } from '../lib/dates'
import { PrimaryButton } from '../routes/setup/shared'
import { inputClass } from '../routes/setup/wizard'

export function CancelDialog({
  item,
  kind,
  today,
  onClose,
  onDone,
}: {
  item: HistoryItem
  kind: CancelKind
  today: string
  onClose: () => void
  onDone: (r: CancelResult) => void
}) {
  const { t } = useTranslation()
  const [reason, setReason] = useState('')
  const mutation = useMutation({
    mutationFn: () => cancelLeave(item.id, reason.trim() ? { reason: reason.trim() } : {}),
    onSuccess: onDone,
  })
  return (
    <Dialog.Root open onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/30" />
        <Dialog.Content className="fixed top-1/2 left-1/2 w-[min(28rem,92vw)] -translate-x-1/2 -translate-y-1/2 space-y-4 rounded bg-white p-5 shadow-lg">
          <Dialog.Title className="text-lg font-semibold">{t(`cancel.${kind}.title`)}</Dialog.Title>
          <Dialog.Description className="text-sm text-gray-700">
            {t(`cancel.${kind}.body`, { count: item.days, date: shortDay(today) })}
          </Dialog.Description>
          {kind !== 'withdraw' && (
            <div>
              <label htmlFor="cancel-reason" className="mb-1 block text-sm font-medium">
                {t('cancel.reason')}
              </label>
              <textarea
                id="cancel-reason"
                className={inputClass}
                rows={2}
                maxLength={500}
                value={reason}
                onChange={(e) => setReason(e.target.value)}
              />
            </div>
          )}
          {mutation.error && (
            <p role="alert" className="rounded bg-red-50 p-2 text-sm text-red-800">
              {errorMessage(mutation.error, t)}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Dialog.Close className="rounded border border-line px-4 py-2">
              {t('cancel.keep')}
            </Dialog.Close>
            <PrimaryButton
              type="button"
              busy={mutation.isPending}
              disabled={mutation.isSuccess}
              onClick={() => mutation.mutate()}
            >
              {t(`cancel.${kind}.confirm`)}
            </PrimaryButton>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
