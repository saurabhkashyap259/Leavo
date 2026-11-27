// Shared, non-component pieces of the setup wizard and registration forms.
export const inputClass = 'mt-1 w-full rounded border border-line px-3 py-2'
export const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
export const WEEKDAYS = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'] as const

export type WizardData = {
  token: string
  org: { name: string; timezone: string; weekend: string[]; leave_year_start_month: number }
  admin: { full_name: string; email: string; code: string }
  extraDomains: string
  holidayPack: string
  seedLeaveTypes: boolean
}

// Legacy names some browsers still report → current IANA names.
const LEGACY_ZONES: Record<string, string> = {
  'Africa/Asmera': 'Africa/Asmara',
  'America/Buenos_Aires': 'America/Argentina/Buenos_Aires',
  'America/Catamarca': 'America/Argentina/Catamarca',
  'America/Cordoba': 'America/Argentina/Cordoba',
  'America/Godthab': 'America/Nuuk',
  'America/Indianapolis': 'America/Indiana/Indianapolis',
  'America/Jujuy': 'America/Argentina/Jujuy',
  'America/Louisville': 'America/Kentucky/Louisville',
  'America/Mendoza': 'America/Argentina/Mendoza',
  'Asia/Calcutta': 'Asia/Kolkata',
  'Asia/Katmandu': 'Asia/Kathmandu',
  'Asia/Rangoon': 'Asia/Yangon',
  'Asia/Saigon': 'Asia/Ho_Chi_Minh',
  'Atlantic/Faeroe': 'Atlantic/Faroe',
  'Europe/Kiev': 'Europe/Kyiv',
  'Pacific/Enderbury': 'Pacific/Kanton',
  'Pacific/Ponape': 'Pacific/Pohnpei',
  'Pacific/Truk': 'Pacific/Chuuk',
}

export const canonicalZone = (zone: string) => LEGACY_ZONES[zone] ?? zone

export function timeZones(): string[] {
  let zones: string[]
  try {
    zones = Intl.supportedValuesOf('timeZone')
  } catch {
    zones = ['Asia/Kolkata', 'UTC']
  }
  return [...new Set(zones.map(canonicalZone))].sort()
}

/** The browser's zone (current IANA name), defaulting to India. */
export function browserTimeZone(): string {
  try {
    return canonicalZone(Intl.DateTimeFormat().resolvedOptions().timeZone || 'Asia/Kolkata')
  } catch {
    return 'Asia/Kolkata'
  }
}
