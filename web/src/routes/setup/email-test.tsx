import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { errorMessage } from '../../api/client'
import { sendTestEmail } from '../../api/setup'
import { PrimaryButton } from './shared'

export function EmailTestStep({
  token,
  email,
  onNext,
}: {
  token: string
  email: string
  onNext: () => void
}) {
  const { t } = useTranslation()
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null)
  const [busy, setBusy] = useState(false)

  async function send() {
    setBusy(true)
    setResult(null)
    try {
      const r = await sendTestEmail(token, email)
      setResult(
        r.ok
          ? { ok: true, text: t('setup.testSent', { email }) }
          : { ok: false, text: t('setup.testFailed', { error: r.error ?? '' }) },
      )
    } catch (err) {
      setResult({ ok: false, text: errorMessage(err, t) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="space-y-4">
      <h2 className="text-lg font-medium">{t('setup.emailTitle')}</h2>
      <p className="text-sm text-gray-700">{t('setup.emailHelp', { email })}</p>
      <button
        type="button"
        onClick={send}
        disabled={busy}
        className="rounded border border-line px-4 py-2"
      >
        {t('setup.sendTest')}
      </button>
      {result &&
        (result.ok ? (
          <p role="status" className="text-sm text-green-800">
            {result.text}
          </p>
        ) : (
          <p role="alert" className="text-sm break-words text-red-700">
            {result.text}
          </p>
        ))}
      <div>
        <PrimaryButton type="button" onClick={onNext}>
          {t('common.continue')}
        </PrimaryButton>
      </div>
    </div>
  )
}
