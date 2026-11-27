import { api } from './client'
import type { components } from './schema'

export type Me = components['schemas']['Me']
export type Role = 'employee' | 'approver' | 'admin'

export const meQuery = {
  queryKey: ['me'] as const,
  queryFn: () => api.get<Me>('/me'),
  staleTime: 60_000,
}
