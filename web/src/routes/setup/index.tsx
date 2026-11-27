// First-run setup wizard (T086, FR-007): token → organization → admin → test email → defaults.
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, errorMessage } from '../../api/client'
import { completeSetup, type PackInfo } from '../../api/setup'
import { AdminStep } from './admin'
import { DefaultsStep } from './defaults'
import { EmailTestStep } from './email-test'
import { OrgStep } from './org'
import { browserTimeZone, type WizardData } from './wizard'
import { TokenStep } from './token'

const TOTAL = 5

export function SetupWizard({ packs, onDone }: { packs: PackInfo[]; onDone: () => void }) {
  const { t } = useTranslation()
  const [step, setStep] = useState(1)
  const [data, setData] = useState<WizardData>({
    token: '',
    org: {
      name: '',
      timezone: browserTimeZone(),
      weekend: ['sat', 'sun'],
      leave_year_start_month: 1,
    },
    admin: { full_name: '', email: '', code: '' },
    extraDomains: '',
    holidayPack: '',
    seedLeaveTypes: true,
  })
  const [adminError, setAdminError] = useState<string | null>(null)
  const [finishError, setFinishError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function finish(d: Pick<WizardData, 'holidayPack' | 'seedLeaveTypes' | 'extraDomains'>) {
    const all = { ...data, ...d }
    setData(all)
    setBusy(true)
    setFinishError(null)
    try {
      await completeSetup({
        setup_token: all.token,
        org: all.org,
        admin: all.admin,
        extra_domains: all.extraDomains
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
        holiday_pack: all.holidayPack || null,
        seed_leave_types: all.seedLeaveTypes,
      })
      onDone()
    } catch (err) {
      const message = errorMessage(err, t)
      if (err instanceof ApiError && (err.code === 'CODE_INVALID' || err.code === 'LOCKED')) {
        setData({ ...all, admin: { ...all.admin, code: '' } })
        setAdminError(message)
        setStep(3)
      } else if (err instanceof ApiError && err.code === 'SETUP_TOKEN_INVALID') {
        setStep(1)
      } else {
        setFinishError(message)
      }
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex min-h-screen items-start justify-center bg-surface px-4 pt-12 sm:pt-20">
      <div className="w-full max-w-md rounded-lg bg-white p-6 shadow-sm">
        <h1 className="text-xl font-semibold">{t('setup.title')}</h1>
        <p className="mb-6 text-sm text-gray-600">{t('setup.step', { n: step, total: TOTAL })}</p>
        {step === 1 && (
          <TokenStep
            token={data.token}
            onNext={(token) => {
              setData({ ...data, token })
              setStep(2)
            }}
          />
        )}
        {step === 2 && (
          <OrgStep
            value={data.org}
            onNext={(org) => {
              setData({ ...data, org })
              setStep(3)
            }}
          />
        )}
        {step === 3 && (
          <AdminStep
            value={data.admin}
            token={data.token}
            error={adminError}
            onNext={(admin) => {
              setData({ ...data, admin })
              setAdminError(null)
              setStep(4)
            }}
          />
        )}
        {step === 4 && (
          <EmailTestStep token={data.token} email={data.admin.email} onNext={() => setStep(5)} />
        )}
        {step === 5 && (
          <DefaultsStep
            value={data}
            packs={packs}
            busy={busy}
            error={finishError}
            onFinish={finish}
          />
        )}
        {step > 1 && (
          <button
            type="button"
            onClick={() => setStep(step - 1)}
            className="mt-6 text-sm text-gray-700 underline"
          >
            {t('common.back')}
          </button>
        )}
      </div>
    </div>
  )
}
