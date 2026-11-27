import { addMonths, format, parseISO } from 'date-fns'

/** "Mon 14 Dec" */
export const shortDay = (d: string) => format(parseISO(d), 'EEE d MMM')

/** "Mon 14 Dec – Tue 15 Dec", or one day. */
export const daySpan = (from: string, to: string) =>
  from === to ? shortDay(from) : `${shortDay(from)} – ${shortDay(to)}`

/** Today as `YYYY-MM-DD` in the browser's time zone. */
export const todayIso = () => format(new Date(), 'yyyy-MM-dd')

/** The leave year (the calendar year it starts in) containing `date`. */
export function leaveYearOf(date: Date, startMonth: number): number {
  return date.getMonth() + 1 >= startMonth ? date.getFullYear() : date.getFullYear() - 1
}

/** `YYYY-MM` of each month in a leave year. */
export function monthsOfLeaveYear(year: number, startMonth: number): string[] {
  const start = new Date(year, startMonth - 1, 1)
  return Array.from({ length: 12 }, (_, i) => format(addMonths(start, i), 'yyyy-MM'))
}
