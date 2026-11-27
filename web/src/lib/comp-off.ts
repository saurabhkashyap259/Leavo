import { format, parseISO } from 'date-fns'

/** "Sat 21 Nov 2026, Sun 22 Nov 2026 (half day)" — `half` is the translated suffix. */
export const workedText = (
  dates: { date: string; portion: number }[],
  half: string,
  pattern = 'EEE d MMM yyyy',
) =>
  dates
    .map((d) => `${format(parseISO(d.date), pattern)}${d.portion < 1 ? ` (${half})` : ''}`)
    .join(', ')

/** Days in the "1 day" / "0.5 day" / "2 days" form used for comp-off. */
export const expiryKey = (days: number) =>
  days <= 1 ? 'compOffs.nextExpiryOne' : 'compOffs.nextExpiry'
