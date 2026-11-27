// One month as a Monday-first table, marking holidays (mandatory solid, optional outlined).
import { eachDayOfInterval, endOfMonth, format, getISODay, parseISO } from 'date-fns'
import type { Holiday } from '../api/holidays'

export function MonthGrid({
  month,
  holidays,
  today,
}: {
  month: string
  holidays: Holiday[]
  today: string
}) {
  const first = parseISO(`${month}-01`)
  const days = eachDayOfInterval({ start: first, end: endOfMonth(first) })
  const cells: (Date | null)[] = [...Array(getISODay(first) - 1).fill(null), ...days]
  while (cells.length % 7) cells.push(null)
  const weeks = Array.from({ length: cells.length / 7 }, (_, i) => cells.slice(i * 7, i * 7 + 7))
  const byDate = new Map<string, Holiday[]>()
  for (const h of holidays) byDate.set(h.date, [...(byDate.get(h.date) ?? []), h])
  const title = format(first, 'MMMM yyyy')
  return (
    <table aria-label={title} className="w-full border-collapse text-center text-sm">
      <caption className="mb-1 text-left font-medium">{title}</caption>
      <thead className="text-xs text-gray-600">
        <tr>
          {/* Monday-first initials in the browser's language (5 Jan 2026 is a Monday). */}
          {Array.from({ length: 7 }, (_, i) => format(new Date(2026, 0, 5 + i), 'EEEEE')).map(
            (d, i) => (
              <th key={i} scope="col" className="font-normal">
                {d}
              </th>
            ),
          )}
        </tr>
      </thead>
      <tbody>
        {weeks.map((week, w) => (
          <tr key={w}>
            {week.map((day, i) => {
              if (!day) return <td key={i} />
              const iso = format(day, 'yyyy-MM-dd')
              const hs = byDate.get(iso) ?? []
              const label = `${format(day, 'd MMMM')}${hs.length ? `: ${hs.map((h) => h.name).join(', ')}` : ''}`
              const mandatory = hs.some((h) => h.kind === 'mandatory')
              return (
                <td
                  key={i}
                  aria-label={label}
                  title={hs.map((h) => h.name).join(', ') || undefined}
                  className={`p-1 ${iso < today ? 'opacity-50' : ''}`}
                >
                  <span
                    className={`inline-block w-7 rounded ${
                      mandatory ? 'bg-accent text-white' : hs.length ? 'border border-accent' : ''
                    } ${iso === today ? 'font-bold underline' : ''}`}
                  >
                    {format(day, 'd')}
                  </span>
                </td>
              )
            })}
          </tr>
        ))}
      </tbody>
    </table>
  )
}
