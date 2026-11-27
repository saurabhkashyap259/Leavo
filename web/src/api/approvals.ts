import { api } from './client'
import type { components } from './schema'

export type ApprovalItem = components['schemas']['ApprovalItem']
export type DecideInput = components['schemas']['DecideInput']
export type Decided = components['schemas']['Decided']
export type Decision = components['schemas']['Decision']
/** `leave` requests, or `cancel`: requests to cancel leave that has started (FR-020). */
export type ApprovalKind = 'leave' | 'cancel' | 'comp_off'

export const approvalsQuery = {
  queryKey: ['approvals', 'pending'] as const,
  queryFn: () => api.get<ApprovalItem[]>('/approvals?status=pending&kind=leave'),
}

export const cancelApprovalsQuery = {
  queryKey: ['approvals', 'cancel'] as const,
  queryFn: () => api.get<ApprovalItem[]>('/approvals?status=pending&kind=cancel'),
}

export const compOffApprovalsQuery = {
  queryKey: ['approvals', 'comp_off'] as const,
  queryFn: () => api.get<ApprovalItem[]>('/approvals?status=pending&kind=comp_off'),
}

export const decide = (kind: ApprovalKind, id: string, body: DecideInput, idempotencyKey: string) =>
  api.post<Decided>(`/approvals/${kind}/${id}/decide`, body, { idempotencyKey })
