// Multi-org registration (T086, FR-008): verify the founder's email, then create the org.
import { Link } from '@tanstack/react-router'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errorMessage } from '../api/client'
import { AuthCard } from '../components/AuthCard'
import { CalendarFields } from './setup/org'
import { Field, PrimaryButton } from './setup/shared'
import { EMAIL_RE, browserTimeZone, inputClass, type WizardData } from './setup/wizard'

type Stage = 'email' | 'code' | 'org'

export function RegisterPage({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation()
  const [stage, setStage] = useState<Stage>('email')
  const [email, setEmail] = useState('')
  const [code, setCode] = useState('')
  const [ticket, setTicket] = useState('')
  const [org, setOrg] = useState<WizardData['org']>({
    name: '',
    timezone: browserTimeZone(),
    weekend: ['sat', 'sun'],
    leave_year_start_month: 1,
  })
  const [fullName, setFullName] = useState('')
  const [fieldError, setFieldError] = useState<Record<string, string | undefined>>({})
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function run(fn: () => Promise<void>) {
    setBusy(true)
    setError(null)
    try {
      await fn()
    } catch (err) {
      setError(errorMessage(err, t))
    } finally {
      setBusy(false)
    }
  }

  function sendCode(e: FormEvent) {
    e.preventDefault()
    const normalized = email.trim().toLowerCase()
    if (!EMAIL_RE.test(normalized)) return setFieldError({ email: t('auth.emailInvalid') })
    setFieldError({})
    void run(async () => {
      await api.post('/auth/code', { email: normalized, purpose: 'register_org' })
      setEmail(normalized)
      setStage('code')
    })
  }

  function verify(e: FormEvent) {
    e.preventDefault()
    if (!/^\d{6}$/.test(code)) return setFieldError({ code: t('auth.codeFormat') })
    setFieldError({})
    void run(async () => {
      const res = await api.post<{ ticket: string }>('/auth/verify', {
        email,
        code,
        purpose: 'register_org',
      })
      setTicket(res.ticket)
      setStage('org')
    })
  }

  function create(e: FormEvent) {
    e.preventDefault()
    const errs = {
      name: org.name.trim() ? undefined : t('setup.orgNameRequired'),
      fullName: fullName.trim() ? undefined : t('auth.nameRequired'),
      weekend: org.weekend.length < 7 ? undefined : t('setup.weekendAllDays'),
    }
    setFieldError(errs)
    if (errs.name || errs.fullName || errs.weekend) return
    void run(async () => {
      await api.post('/orgs', { ticket, ...org, name: org.name.trim(), full_name: fullName.trim() })
      onDone()
    })
  }

  const alert = error && (
    <p role="alert" className="text-sm text-red-700">
      {error}
    </p>
  )

  return (
    <AuthCard title={t('register.title')}>
      {stage === 'email' && (
        <form onSubmit={sendCode} className="space-y-4" noValidate>
          <p className="text-sm text-gray-700">{t('register.help')}</p>
          <Field label={t('auth.email')} error={fieldError.email}>
            {(id, hint) => (
              <input
                id={id}
                type="email"
                autoComplete="email"
                aria-describedby={hint}
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                className={inputClass}
              />
            )}
          </Field>
          {alert}
          <PrimaryButton type="submit" busy={busy}>
            {t('auth.sendCode')}
          </PrimaryButton>
          <p className="text-sm">
            {t('register.haveAccount')}{' '}
            <Link to="/login" className="underline">
              {t('auth.signIn')}
            </Link>
          </p>
        </form>
      )}
      {stage === 'code' && (
        <form onSubmit={verify} className="space-y-4" noValidate>
          <p className="text-sm text-gray-700">{t('setup.adminCodeHelp', { email })}</p>
          <Field label={t('auth.code')} error={fieldError.code}>
            {(id, hint) => (
              <input
                id={id}
                inputMode="numeric"
                autoComplete="one-time-code"
                maxLength={6}
                aria-describedby={hint}
                value={code}
                onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
                className={`${inputClass} tracking-widest`}
              />
            )}
          </Field>
          {alert}
          <PrimaryButton type="submit" busy={busy}>
            {t('common.continue')}
          </PrimaryButton>
        </form>
      )}
      {stage === 'org' && (
        <form onSubmit={create} className="space-y-4" noValidate>
          <Field label={t('setup.orgName')} error={fieldError.name}>
            {(id, hint) => (
              <input
                id={id}
                aria-describedby={hint}
                maxLength={120}
                value={org.name}
                onChange={(e) => setOrg({ ...org, name: e.target.value })}
                className={inputClass}
              />
            )}
          </Field>
          <Field label={t('setup.adminName')} error={fieldError.fullName}>
            {(id, hint) => (
              <input
                id={id}
                aria-describedby={hint}
                autoComplete="name"
                maxLength={120}
                value={fullName}
                onChange={(e) => setFullName(e.target.value)}
                className={inputClass}
              />
            )}
          </Field>
          <CalendarFields
            value={org}
            onChange={(v) => setOrg({ ...org, ...v })}
            weekendError={fieldError.weekend}
          />
          {alert}
          <PrimaryButton type="submit" busy={busy}>
            {t('register.create')}
          </PrimaryButton>
        </form>
      )}
    </AuthCard>
  )
}
