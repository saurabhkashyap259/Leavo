import { api } from './client'
import type { components } from './schema'

export type Holiday = components['schemas']['HolidayItem']

/** Holidays of a leave year (employees may ask for the current and the next one). */
export const holidaysQuery = (year: number) => ({
  queryKey: ['holidays', year] as const,
  queryFn: () => api.get<Holiday[]>(`/holidays?year=${year}`),
  staleTime: 5 * 60_000,
})

export type HolidayInput = components['schemas']['HolidayInput']
export type ImportResult = components['schemas']['ImportResult']
export type PackInfo = components['schemas']['PackInfo']

export const packsQuery = {
  queryKey: ['holiday-packs'] as const,
  queryFn: () => api.get<PackInfo[]>('/holiday-packs'),
  staleTime: Infinity,
}

export const addHoliday = (body: HolidayInput) => api.post<Holiday>('/admin/holidays', body)

export const updateHoliday = (id: string, body: Partial<HolidayInput>) =>
  api.patch<Holiday>(`/admin/holidays/${id}`, body)

export const deleteHoliday = (id: string) => api.delete(`/admin/holidays/${id}`)

/** A pack, a copy of another year, or a CSV file. */
export type ImportFrom =
  { pack: string } | { copy_from_year: number; to_year: number } | { file: File }

export function importHolidays(from: ImportFrom, dryRun: boolean) {
  const path = `/admin/holidays/import?dry_run=${dryRun}`
  if ('file' in from) {
    const form = new FormData()
    form.append('file', from.file)
    return api.post<ImportResult>(path, form)
  }
  return api.post<ImportResult>(path, from)
}
