import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { logout } from '../api/auth'
import { errorMessage } from '../api/client'

export function ProfilePage({ onSignedOut }: { onSignedOut: () => void }) {
  const { t } = useTranslation()
  const [error, setError] = useState<string | null>(null)

  async function signOut(all: boolean) {
    setError(null)
    try {
      await logout(all)
      onSignedOut()
    } catch (err) {
      setError(errorMessage(err, t))
    }
  }

  return (
    <section className="max-w-xl space-y-6">
      <h1 className="text-2xl font-semibold">{t('profile.title')}</h1>
      <div className="space-y-2">
        <button
          type="button"
          onClick={() => signOut(false)}
          className="rounded border border-line px-4 py-2"
        >
          {t('profile.signOut')}
        </button>
      </div>
      <div className="space-y-2">
        <button
          type="button"
          onClick={() => signOut(true)}
          className="rounded border border-red-300 px-4 py-2 text-red-700"
        >
          {t('profile.signOutEverywhere')}
        </button>
        <p className="text-sm text-gray-600">{t('profile.signOutEverywhereHelp')}</p>
      </div>
      <div className="space-y-2">
        <a href="/api/v1/me/export" className="inline-block rounded border border-line px-4 py-2">
          {t('profile.downloadData')}
        </a>
        <p className="text-sm text-gray-600">{t('profile.downloadDataHelp')}</p>
      </div>
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
    </section>
  )
}
