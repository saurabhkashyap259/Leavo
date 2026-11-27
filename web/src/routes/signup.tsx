import { Link } from '@tanstack/react-router'
import { useId, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { requestCode } from '../api/auth'
import { errorMessage } from '../api/client'
import { AuthCard } from '../components/AuthCard'
import { CodeEntry } from '../components/CodeEntry'

const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/

export function SignupPage({ onSignedIn }: { onSignedIn: () => void }) {
  const { t } = useTranslation()
  const ids = { name: useId(), email: useId(), doj: useId() }
  const [form, setForm] = useState({ name: '', email: '', doj: '' })
  const [sentTo, setSentTo] = useState<string | null>(null)
  const [fieldError, setFieldError] = useState<{ name?: string; email?: string }>({})
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const body = (email: string) => ({
    email,
    purpose: 'signup' as const,
    full_name: form.name.trim(),
    ...(form.doj ? { date_of_joining: form.doj } : {}),
  })

  async function submit(e: FormEvent) {
    e.preventDefault()
    const email = form.email.trim().toLowerCase()
    const errs = {
      name: form.name.trim() ? undefined : t('auth.nameRequired'),
      email: EMAIL_RE.test(email) ? undefined : t('auth.emailInvalid'),
    }
    setFieldError(errs)
    if (errs.name || errs.email) return
    setBusy(true)
    setError(null)
    try {
      await requestCode(body(email))
      setSentTo(email)
    } catch (err) {
      setError(errorMessage(err, t))
    } finally {
      setBusy(false)
    }
  }

  const input = 'mt-1 w-full rounded border border-line px-3 py-2'

  return (
    <AuthCard title={t('auth.signUpTitle')}>
      {sentTo ? (
        <CodeEntry
          email={sentTo}
          purpose="signup"
          onResend={() => requestCode(body(sentTo))}
          onChangeEmail={() => setSentTo(null)}
          onSignedIn={onSignedIn}
        />
      ) : (
        <form onSubmit={submit} className="space-y-4" noValidate>
          <div>
            <label htmlFor={ids.name} className="block text-sm font-medium">
              {t('auth.fullName')}
            </label>
            <input
              id={ids.name}
              autoComplete="name"
              autoFocus
              maxLength={120}
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              aria-invalid={!!fieldError.name}
              className={input}
            />
            {fieldError.name && <p className="mt-1 text-sm text-red-700">{fieldError.name}</p>}
          </div>
          <div>
            <label htmlFor={ids.email} className="block text-sm font-medium">
              {t('auth.email')}
            </label>
            <input
              id={ids.email}
              type="email"
              autoComplete="email"
              value={form.email}
              onChange={(e) => setForm({ ...form, email: e.target.value })}
              aria-invalid={!!fieldError.email}
              className={input}
            />
            {fieldError.email && <p className="mt-1 text-sm text-red-700">{fieldError.email}</p>}
          </div>
          <div>
            <label htmlFor={ids.doj} className="block text-sm font-medium">
              {t('auth.dateOfJoining')}
            </label>
            <input
              id={ids.doj}
              type="date"
              value={form.doj}
              onChange={(e) => setForm({ ...form, doj: e.target.value })}
              className={input}
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
            {t('auth.haveAccount')}{' '}
            <Link to="/login" className="text-accent underline">
              {t('auth.signIn')}
            </Link>
          </p>
        </form>
      )}
    </AuthCard>
  )
}
