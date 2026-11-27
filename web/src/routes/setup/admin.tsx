import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { errorMessage } from '../../api/client'
import { requestSetupCode } from '../../api/setup'
import { Field, PrimaryButton } from './shared'
import { EMAIL_RE, inputClass, type WizardData } from './wizard'

type Admin = WizardData['admin']

export function AdminStep({
  value,
  token,
  error: initialError,
  onNext,
}: {
  value: Admin
  token: string
  error?: string | null
  onNext: (admin: Admin) => void
}) {
  const { t } = useTranslation()
  const [admin, setAdmin] = useState(value)
  const [sent, setSent] = useState(!!value.code)
  const [errors, setErrors] = useState<{ name?: string; email?: string; code?: string }>({})
  const [error, setError] = useState<string | null>(initialError ?? null)
  const [busy, setBusy] = useState(false)

  async function sendCode(e: FormEvent) {
    e.preventDefault()
    const email = admin.email.trim().toLowerCase()
    const errs = {
      name: admin.full_name.trim() ? undefined : t('auth.nameRequired'),
      email: EMAIL_RE.test(email) ? undefined : t('auth.emailInvalid'),
    }
    setErrors(errs)
    if (errs.name || errs.email) return
    setBusy(true)
    setError(null)
    try {
      await requestSetupCode(email, token)
      setAdmin({ ...admin, email, code: '' })
      setSent(true)
    } catch (err) {
      setError(errorMessage(err, t))
    } finally {
      setBusy(false)
    }
  }

  function next(e: FormEvent) {
    e.preventDefault()
    if (!/^\d{6}$/.test(admin.code)) {
      setErrors({ code: t('auth.codeFormat') })
      return
    }
    onNext({ ...admin, full_name: admin.full_name.trim() })
  }

  return (
    <div className="space-y-4">
      <h2 className="text-lg font-medium">{t('setup.adminTitle')}</h2>
      <form onSubmit={sendCode} className="space-y-4" noValidate>
        <Field label={t('setup.adminName')} error={errors.name}>
          {(id, hint) => (
            <input
              id={id}
              aria-describedby={hint}
              aria-invalid={!!errors.name}
              autoComplete="name"
              maxLength={120}
              value={admin.full_name}
              onChange={(e) => setAdmin({ ...admin, full_name: e.target.value })}
              className={inputClass}
            />
          )}
        </Field>
        <Field label={t('auth.email')} error={errors.email}>
          {(id, hint) => (
            <input
              id={id}
              type="email"
              aria-describedby={hint}
              aria-invalid={!!errors.email}
              autoComplete="email"
              value={admin.email}
              onChange={(e) => {
                setAdmin({ ...admin, email: e.target.value })
                setSent(false)
              }}
              className={inputClass}
            />
          )}
        </Field>
        <button type="submit" disabled={busy} className="rounded border border-line px-4 py-2">
          {sent ? t('auth.resend') : t('auth.sendCode')}
        </button>
      </form>
      {sent && (
        <form onSubmit={next} className="space-y-4" noValidate>
          <p className="text-sm text-gray-700">
            {t('setup.adminCodeHelp', { email: admin.email })}
          </p>
          <Field label={t('auth.code')} error={errors.code}>
            {(id, hint) => (
              <input
                id={id}
                aria-describedby={hint}
                inputMode="numeric"
                autoComplete="one-time-code"
                maxLength={6}
                value={admin.code}
                onChange={(e) => setAdmin({ ...admin, code: e.target.value.replace(/\D/g, '') })}
                className={`${inputClass} tracking-widest`}
              />
            )}
          </Field>
          <PrimaryButton type="submit">{t('common.continue')}</PrimaryButton>
        </form>
      )}
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
    </div>
  )
}
