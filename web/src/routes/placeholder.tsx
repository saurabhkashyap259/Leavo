import { useTranslation } from 'react-i18next'

/** Temporary page until the owning user story builds the real screen. */
export function Placeholder({ titleKey }: { titleKey: string }) {
  const { t } = useTranslation()
  return <h1 className="text-2xl font-semibold">{t(titleKey)}</h1>
}
