import { api } from './client'
import type { components } from './schema'

export type CompOffSummary = components['schemas']['CompOffSummary']
export type CreditItem = components['schemas']['CreditItem']
export type CompOffRequestItem = components['schemas']['CompOffRequestItem']
export type CompOffInput = components['schemas']['CompOffInput']
export type CompOffCreated = components['schemas']['CompOffCreated']

export const compOffsQuery = {
  queryKey: ['comp-offs'] as const,
  queryFn: () => api.get<CompOffSummary>('/comp-offs'),
}

export const createCompOff = (body: CompOffInput, idempotencyKey: string) =>
  api.post<CompOffCreated>('/comp-offs', body, { idempotencyKey })

export const withdrawCompOff = (id: string) =>
  api.post<{ status: string }>(`/comp-offs/${id}/cancel`, {})
