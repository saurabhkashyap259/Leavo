// Import (T201, FR-043): a Keka export or the generic CSV template. Preview first: people to
// create, balances to set, history rows, errors and warnings. Keka leave type names are mapped
// to Leavo types here. Import stays disabled while any error exists; warnings do not block it.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { leaveTypesQuery, runImport, type ImportReport, type ImportSource } from '../../api/admin'
import { errorMessage } from '../../api/client'
import { leaveYearOf } from '../../lib/dates'
import { PrimaryButton } from '../setup/shared'
import { inputClass } from '../setup/wizard'

type Issue = ImportReport['errors'][number]

function IssueTable({ label, issues, tone }: { label: string; issues: Issue[]; tone: string }) {
  const { t } = useTranslation()
  if (issues.length === 0) return null
  return (
    <div className="overflow-x-auto">
      <table
        aria-label={label}
        className={`w-full rounded border border-line bg-white text-left text-sm ${tone}`}
      >
        <caption className="p-2 text-left font-medium">{label}</caption>
        <thead className="text-gray-600">
          <tr>
            <th className="p-2">{t('import.file')}</th>
            <th className="p-2">{t('import.row')}</th>
            <th className="p-2">{t('import.code')}</th>
            <th className="p-2">{t('import.message')}</th>
          </tr>
        </thead>
        <tbody>
          {issues.map((i) => (
            <tr key={`${i.file}-${i.row}-${i.code}`} className="border-t border-line">
              <td className="p-2">{t(`import.files.${i.file}`)}</td>
              <td className="p-2">{i.row}</td>
              <td className="p-2 font-mono">{i.code}</td>
              <td className="p-2">{i.message}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function ImportPage({ leaveYearStartMonth }: { leaveYearStartMonth: number }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const types = useQuery(leaveTypesQuery)
  const [current] = useState(() => leaveYearOf(new Date(), leaveYearStartMonth))
  const [source, setSource] = useState<ImportSource>('keka')
  const [balances, setBalances] = useState<File | null>(null)
  const [history, setHistory] = useState<File | null>(null)
  const [year, setYear] = useState(current)
  const [map, setMap] = useState<Record<string, string>>({})
  const [report, setReport] = useState<ImportReport | null>(null)
  const [done, setDone] = useState<string | null>(null)
  const options = (dryRun: boolean) => ({
    dryRun,
    leaveYear: year,
    leaveTypeMap: source === 'keka' && Object.keys(map).length > 0 ? map : undefined,
  })
  const preview = useMutation({
    mutationFn: () => runImport(source, { balances: balances!, history }, options(true)),
    onSuccess: (r) => {
      setDone(null)
      setReport(r)
      // Keep the suggested or chosen code for every Keka type found.
      if (r.keka_leave_types) {
        const next: Record<string, string> = {}
        for (const k of r.keka_leave_types) if (k.code) next[k.name] = k.code
        setMap((m) => ({ ...next, ...m }))
      }
    },
  })
  const apply = useMutation({
    mutationFn: () => runImport(source, { balances: balances!, history }, options(false)),
    onSuccess: (r) => {
      setReport(null)
      setDone(
        t('import.done', {
          people: t('import.people', { count: r.users_to_create.length }),
          balances: t('import.balances', { count: r.balances_to_set.length }),
        }),
      )
      void queryClient.invalidateQueries({ queryKey: ['admin'] })
    },
  })
  const switchTo = (s: ImportSource) => {
    setSource(s)
    setReport(null)
    setMap({})
  }
  const error = preview.error ?? apply.error
  const blocked = !report || report.errors.length > 0
  return (
    <section className="max-w-5xl space-y-4">
      <h1 className="text-2xl font-semibold">{t('import.title')}</h1>
      <p className="text-sm text-gray-700">{t('import.help')}</p>
      <div role="tablist" className="flex gap-1">
        {(['keka', 'csv'] as const).map((s) => (
          <button
            key={s}
            role="tab"
            type="button"
            aria-selected={s === source}
            onClick={() => switchTo(s)}
            className={`rounded px-3 py-1 ${s === source ? 'bg-accent text-white' : 'border border-line'}`}
          >
            {t(`import.sources.${s}`)}
          </button>
        ))}
      </div>
      <p className="text-xs text-gray-600">{t(`import.formats.${source}`)}</p>
      <div className="flex flex-wrap items-end gap-4 rounded border border-line bg-white p-4">
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('import.balancesFile')}</span>
          <input
            type="file"
            accept=".csv,text/csv"
            onChange={(e) => {
              setBalances(e.target.files?.[0] ?? null)
              setReport(null)
            }}
          />
        </label>
        <label className="block text-sm">
          <span className="mb-1 block font-medium">{t('import.historyFile')}</span>
          <input
            type="file"
            accept=".csv,text/csv"
            onChange={(e) => {
              setHistory(e.target.files?.[0] ?? null)
              setReport(null)
            }}
          />
        </label>
        {source === 'keka' && (
          <label className="block text-sm">
            <span className="mb-1 block font-medium">{t('import.year')}</span>
            <select
              className={inputClass}
              value={year}
              onChange={(e) => setYear(Number(e.target.value))}
            >
              {[current, current + 1, current - 1].map((y) => (
                <option key={y} value={y}>
                  {y}
                </option>
              ))}
            </select>
          </label>
        )}
        <button
          type="button"
          className="rounded border border-line px-3 py-2"
          disabled={!balances || preview.isPending}
          onClick={() => preview.mutate()}
        >
          {t('import.preview')}
        </button>
      </div>
      {source === 'keka' && report?.keka_leave_types && report.keka_leave_types.length > 0 && (
        <fieldset className="space-y-2 rounded border border-line bg-white p-4">
          <legend className="px-1 font-medium">{t('import.mapping')}</legend>
          {report.keka_leave_types.map((k) => (
            <label key={k.name} className="flex flex-wrap items-center gap-2 text-sm">
              <span className="w-48">{k.name}</span>
              <select
                aria-label={k.name}
                className={inputClass}
                value={map[k.name] ?? ''}
                onChange={(e) => setMap({ ...map, [k.name]: e.target.value })}
              >
                <option value="">{t('import.notMapped')}</option>
                {types.data?.map((ty) => (
                  <option key={ty.id} value={ty.code}>
                    {ty.name}
                  </option>
                ))}
              </select>
            </label>
          ))}
          <p className="text-xs text-gray-600">{t('import.mappingHelp')}</p>
        </fieldset>
      )}
      {error && (
        <p role="alert" className="rounded bg-red-50 p-2 text-sm text-red-800">
          {errorMessage(error, t)}
        </p>
      )}
      {done && (
        <p role="status" className="rounded bg-green-50 p-2 text-sm text-green-800">
          {done}
        </p>
      )}
      {report && (
        <div className="space-y-3">
          <p className="font-medium">
            {t('import.summary', {
              people: t('import.toCreate', { count: report.users_to_create.length }),
              balances: t('import.balances', { count: report.balances_to_set.length }),
              history: t('import.historyRows', { count: report.history_rows.length }),
            })}
          </p>
          {report.skipped_history ? (
            <p className="text-sm text-gray-700">
              {t('import.skipped', { count: report.skipped_history })}
            </p>
          ) : null}
          <IssueTable label={t('import.errors')} issues={report.errors} tone="" />
          <IssueTable label={t('import.warnings')} issues={report.warnings} tone="" />
          {report.balances_to_set.length > 0 && (
            <details className="rounded border border-line bg-white p-2 text-sm">
              <summary>{t('import.balancesList')}</summary>
              <ul className="mt-2">
                {report.balances_to_set.map((b) => (
                  <li key={`${b.row}`}>
                    {b.email} — {b.leave_type_code} {b.leave_year}: {b.available_days}
                  </li>
                ))}
              </ul>
            </details>
          )}
        </div>
      )}
      <div className="flex items-center gap-3">
        <PrimaryButton
          type="button"
          busy={apply.isPending}
          disabled={blocked}
          onClick={() => apply.mutate()}
        >
          {t('import.apply')}
        </PrimaryButton>
        {report && report.errors.length > 0 && (
          <span className="text-sm text-red-800">{t('import.fixFirst')}</span>
        )}
      </div>
    </section>
  )
}
