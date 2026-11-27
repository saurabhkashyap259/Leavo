import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { errorMessage } from '../../api/client'
import { verifySetupToken } from '../../api/setup'
import { Field, PrimaryButton } from './shared'
import { inputClass } from './wizard'

export function TokenStep({ token, onNext }: { token: string; onNext: (token: string) => void }) {
  const { t } = useTranslation()
  const [value, setValue] = useState(token)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await verifySetupToken(value.trim())
      onNext(value.trim())
    } catch (err) {
      setError(errorMessage(err, t))
    } finally {
      setBusy(false)
    }
  }

  return (
    <form onSubmit={submit} className="space-y-4" noValidate>
      <h2 className="text-lg font-medium">{t('setup.tokenTitle')}</h2>
      <Field label={t('setup.token')} help={t('setup.tokenHelp')}>
        {(id, hint) => (
          <input
            id={id}
            aria-describedby={hint}
            autoFocus
            autoComplete="off"
            spellCheck={false}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            className={`${inputClass} font-mono`}
          />
        )}
      </Field>
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
      <PrimaryButton type="submit" busy={busy}>
        {t('common.continue')}
      </PrimaryButton>
    </form>
  )
}
