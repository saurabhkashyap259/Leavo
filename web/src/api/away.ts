import { api } from './client'
import type { components } from './schema'

export type AwayList = components['schemas']['AwayList']
export type AwayItem = components['schemas']['AwayItem']
export type TeamCalendar = components['schemas']['TeamCalendar']
export type CalendarPerson = components['schemas']['CalendarPerson']
export type AwayRange = 'today' | 'week' | 'month'
/** `team` = the viewer's approver group, the groups they approve, or their department. */
export type Scope = 'org' | 'team'

export const awayQuery = (range: AwayRange, scope: Scope) => ({
  queryKey: ['away', range, scope] as const,
  queryFn: () => api.get<AwayList>(`/away?range=${range}&scope=${scope}`),
})

/** `month` is `YYYY-MM`. */
export const teamCalendarQuery = (month: string, scope: Scope) => ({
  queryKey: ['team-calendar', month, scope] as const,
  queryFn: () => api.get<TeamCalendar>(`/team/calendar?month=${month}&scope=${scope}`),
})
