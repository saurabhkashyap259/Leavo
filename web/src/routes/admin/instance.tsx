// Instance (T213, FR-051): what the operator needs at a glance — version and updates, email
// delivery, storage, migrations and the job queue — plus a test email.
import * as Dialog from '@radix-ui/react-dialog'
import { useMutation, useQuery } from '@tanstack/react-query'
import { format, parseISO } from 'date-fns'
import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { instanceQuery, sendInstanceTestEmail, signEveryoneOut } from '../../api/admin'
import { errorMessage } from '../../api/client'
import { PrimaryButton } from '../setup/shared'

/** FR-063: end every session in the organization, including the admin's own. */
function SignOutEveryone({ onSignedOut }: { onSignedOut: () => void }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const mutation = useMutation({ mutationFn: signEveryoneOut, onSuccess: onSignedOut })
  return (
    <>
      <button
        type="button"
        className="rounded border border-red-300 px-3 py-1 text-sm text-red-700"
        onClick={() => setOpen(true)}
      >
        {t('instance.signOutAllOpen')}
      </button>
      <Dialog.Root open={open} onOpenChange={setOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 bg-black/30" />
          <Dialog.Content className="fixed top-1/2 left-1/2 w-[min(28rem,92vw)] -translate-x-1/2 -translate-y-1/2 space-y-4 rounded bg-white p-5 shadow-lg">
            <Dialog.Title className="text-lg font-semibold">
              {t('instance.signOutAllTitle')}
            </Dialog.Title>
            <Dialog.Description className="text-sm text-gray-700">
              {t('instance.signOutAllHelp')}
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
                {t('instance.signOutAll')}
              </PrimaryButton>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </>
  )
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid gap-1 border-t border-line py-3 first:border-t-0 sm:grid-cols-[12rem_1fr]">
      <dt className="font-medium">{label}</dt>
      <dd className="space-y-1 text-gray-800">{children}</dd>
    </div>
  )
}

export function InstancePage({ onSignedOut = () => {} }: { onSignedOut?: () => void }) {
  const { t } = useTranslation()
  const status = useQuery(instanceQuery)
  const test = useMutation({ mutationFn: sendInstanceTestEmail })
  const s = status.data
  return (
    <section className="max-w-3xl space-y-4">
      <h1 className="text-2xl font-semibold">{t('instance.title')}</h1>
      {status.error && <p className="text-red-700">{errorMessage(status.error, t)}</p>}
      {s && (
        <dl className="rounded border border-line bg-white px-4">
          <Row label={t('instance.version')}>
            <p>{t('instance.versionValue', { version: s.version })}</p>
            {!s.update_check.enabled ? (
              <p className="text-sm text-gray-600">{t('instance.updateOff')}</p>
            ) : s.update_check.update_available ? (
              <p className="text-sm text-amber-900">
                {t('instance.updateAvailable', { version: s.update_check.latest_version })}
              </p>
            ) : (
              <p className="text-sm text-gray-600">{t('instance.upToDate')}</p>
            )}
          </Row>
          <Row label={t('instance.email')}>
            <p>
              {t('instance.emailCounts', { sent: s.email.sent_24h, failed: s.email.failed_24h })}
            </p>
            {s.email.last_error && (
              <p className="text-sm text-red-800">
                {t('instance.lastError', {
                  error: s.email.last_error,
                  at: s.email.last_error_at
                    ? format(parseISO(s.email.last_error_at), 'd MMM HH:mm')
                    : '',
                })}
              </p>
            )}
            <button
              type="button"
              className="rounded border border-line px-3 py-1 text-sm"
              disabled={test.isPending}
              onClick={() => test.mutate()}
            >
              {t('instance.sendTest')}
            </button>
            {test.data && (
              <p role="status" className="text-sm text-green-800">
                {t('instance.testQueued', { to: test.data.to })}
              </p>
            )}
            {test.error && <p className="text-sm text-red-800">{errorMessage(test.error, t)}</p>}
          </Row>
          <Row label={t('instance.jobs')}>
            <p>
              {t('instance.jobCounts', {
                queued: s.jobs.queued,
                oldest: Math.round((s.jobs.oldest_queued_seconds ?? 0) / 60),
                dead: s.jobs.dead,
              })}
            </p>
          </Row>
          {s.storage && (
            <Row label={t('instance.storage')}>
              <p>{t(`instance.storageKinds.${s.storage}`)}</p>
            </Row>
          )}
          {s.migrations && (
            <Row label={t('instance.migrations')}>
              <p>{t('instance.migrationCounts', s.migrations)}</p>
            </Row>
          )}
          <Row label={t('instance.sessions')}>
            <p className="text-sm text-gray-700">{t('instance.sessionsHelp')}</p>
            <SignOutEveryone onSignedOut={onSignedOut} />
          </Row>
        </dl>
      )}
    </section>
  )
}
