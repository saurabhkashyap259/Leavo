import { Link } from '@tanstack/react-router'
import { useId, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { requestCode } from '../api/auth'
import { errorMessage } from '../api/client'
import { AuthCard } from '../components/AuthCard'
import { CodeEntry } from '../components/CodeEntry'

const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/

export function LoginPage({ onSignedIn }: { onSignedIn: () => void }) {
  const { t } = useTranslation()
  const emailId = useId()
  const [email, setEmail] = useState('')
  const [sentTo, setSentTo] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function submit(e: FormEvent) {
    e.preventDefault()
    const value = email.trim().toLowerCase()
    if (!EMAIL_RE.test(value)) {
      setError(t('auth.emailInvalid'))
      return
    }
    setBusy(true)
    setError(null)
    try {
      await requestCode({ email: value, purpose: 'login' })
      setSentTo(value)
    } catch (err) {
      setError(errorMessage(err, t))
    } finally {
      setBusy(false)
    }
  }

  return (
    <AuthCard title={t('auth.signInTitle')}>
      {sentTo ? (
        <CodeEntry
          email={sentTo}
          purpose="login"
          onResend={() => requestCode({ email: sentTo, purpose: 'login' })}
          onChangeEmail={() => setSentTo(null)}
          onSignedIn={onSignedIn}
        />
      ) : (
        <form onSubmit={submit} className="space-y-4" noValidate>
          <div>
            <label htmlFor={emailId} className="block text-sm font-medium">
              {t('auth.email')}
            </label>
            <input
              id={emailId}
              type="email"
              autoComplete="email"
              autoFocus
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              className="mt-1 w-full rounded border border-line px-3 py-2"
            />
          </div>
          {error && (
            <p role="alert" className="text-sm text-red-700">
              {error}
            </p>
          )}
          <button
            type="submit"
            disabled={busy}
            className="w-full rounded bg-accent px-4 py-2 text-white disabled:opacity-60"
          >
            {t('auth.sendCode')}
          </button>
          <p className="text-sm text-gray-600">
            {t('auth.noAccount')}{' '}
            <Link to="/signup" className="text-accent underline">
              {t('auth.createAccount')}
            </Link>
          </p>
        </form>
      )}
    </AuthCard>
  )
}
