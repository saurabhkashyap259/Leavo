import { useEffect, useId, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { verifyCode, type Purpose } from '../api/auth'
import { errorMessage } from '../api/client'

const RESEND_AFTER = 30

type Props = {
  email: string
  purpose: Purpose
  onResend: () => Promise<void>
  onChangeEmail: () => void
  onSignedIn: () => void
}

/** Step 2 of sign-in/sign-up: the same screen whether or not the email has an account (FR-003). */
export function CodeEntry({ email, purpose, onResend, onChangeEmail, onSignedIn }: Props) {
  const { t } = useTranslation()
  const codeId = useId()
  const keepId = useId()
  const [code, setCode] = useState('')
  const [keep, setKeep] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  // Countdown from a deadline, so it stays correct even if timers are throttled.
  const [resendAt, setResendAt] = useState(() => Date.now() + RESEND_AFTER * 1000)
  const [now, setNow] = useState(() => Date.now())
  const wait = Math.max(0, Math.ceil((resendAt - now) / 1000))

  useEffect(() => {
    if (wait <= 0) return
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [wait])

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!/^\d{6}$/.test(code.trim())) {
      setError(t('auth.codeFormat'))
      return
    }
    setBusy(true)
    setError(null)
    try {
      await verifyCode({ email, code: code.trim(), purpose, keep_signed_in: keep })
      onSignedIn()
    } catch (err) {
      setError(errorMessage(err, t))
    } finally {
      setBusy(false)
    }
  }

  async function resend() {
    setError(null)
    try {
      await onResend()
      const t0 = Date.now()
      setNow(t0)
      setResendAt(t0 + RESEND_AFTER * 1000)
    } catch (err) {
      setError(errorMessage(err, t))
    }
  }

  return (
    <form onSubmit={submit} className="space-y-4" noValidate>
      <p role="status" className="text-sm text-gray-700">
        {t('auth.codeSent', { email })}
      </p>
      <div>
        <label htmlFor={codeId} className="block text-sm font-medium">
          {t('auth.code')}
        </label>
        <input
          id={codeId}
          value={code}
          onChange={(e) => setCode(e.target.value.replace(/\D/g, '').slice(0, 6))}
          inputMode="numeric"
          autoComplete="one-time-code"
          autoFocus
          maxLength={6}
          className="mt-1 w-40 rounded border border-line px-3 py-2 font-mono text-lg tracking-[0.4em]"
        />
      </div>
      <label htmlFor={keepId} className="flex items-center gap-2 text-sm">
        <input
          id={keepId}
          type="checkbox"
          checked={keep}
          onChange={(e) => setKeep(e.target.checked)}
        />
        {t('auth.keepSignedIn')}
      </label>
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
      <div className="flex flex-wrap items-center gap-3">
        <button
          type="submit"
          disabled={busy}
          className="rounded bg-accent px-4 py-2 text-white disabled:opacity-60"
        >
          {t('auth.signIn')}
        </button>
        <button
          type="button"
          onClick={resend}
          disabled={wait > 0}
          className="text-sm text-accent underline disabled:text-gray-400 disabled:no-underline"
        >
          {wait > 0 ? t('auth.resendIn', { seconds: wait }) : t('auth.resend')}
        </button>
        <button type="button" onClick={onChangeEmail} className="text-sm text-gray-600 underline">
          {t('auth.useDifferentEmail')}
        </button>
      </div>
    </form>
  )
}
