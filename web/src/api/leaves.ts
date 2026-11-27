import { api, ApiError } from './client'
import type { components } from './schema'

export type Balance = components['schemas']['Balance']
export type LeaveInput = components['schemas']['LeaveInput']
export type Preview = components['schemas']['Preview']
export type Created = components['schemas']['Created']
export type UserHit = components['schemas']['UserHit']
export type Uploaded = components['schemas']['Uploaded']
export type NotifyMember = components['schemas']['NotifyMember']
export type DayHalf = components['schemas']['DayHalf']

export const balancesQuery = {
  queryKey: ['me', 'balances'] as const,
  queryFn: () => api.get<Balance[]>('/me/balances'),
}

export const previewLeave = (body: LeaveInput, signal?: AbortSignal) =>
  api.post<Preview>('/leaves/preview', body, { signal })

export const createLeave = (body: LeaveInput, idempotencyKey: string) =>
  api.post<Created>('/leaves', body, { idempotencyKey })

export const searchUsers = (q: string, signal?: AbortSignal) =>
  api.get<UserHit[]>(`/users/search?q=${encodeURIComponent(q)}`, { signal })

/** Multipart upload; the JSON client does not handle FormData. */
export async function uploadAttachment(file: File): Promise<Uploaded> {
  const form = new FormData()
  form.append('file', file)
  const res = await fetch('/api/v1/attachments', {
    method: 'POST',
    headers: { 'X-Requested-With': 'leavo', Accept: 'application/json' },
    credentials: 'same-origin',
    body: form,
  })
  const data = await res.json().catch(() => undefined)
  if (!res.ok) {
    const err = (data as { error?: { code?: string; message?: string } } | undefined)?.error
    throw new ApiError(res.status, err?.code ?? 'INTERNAL', err?.message ?? res.statusText)
  }
  return data as Uploaded
}

export type HistoryItem = components['schemas']['HistoryItem']
export type HistoryPage = components['schemas']['HistoryPage']
export type RequestStatus = 'pending' | 'approved' | 'rejected' | 'cancelled' | 'cancel_requested'
export type HistoryFilters = { year?: string; type?: string; status?: string }

/** Query string of `/me/leaves` and `/me/leaves.csv`; empty filters are left out. */
export function historyParams(f: HistoryFilters, extra: Record<string, string> = {}): string {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries({ ...f, ...extra })) if (v) p.set(k, v)
  const s = p.toString()
  return s ? `?${s}` : ''
}

export const historyPage = (f: HistoryFilters, cursor?: string, limit = 25) =>
  api.get<HistoryPage>(
    `/me/leaves${historyParams(f, { limit: String(limit), ...(cursor ? { cursor } : {}) })}`,
  )

/** The newest requests, for the dashboard. */
export const recentRequestsQuery = {
  queryKey: ['me', 'leaves', 'recent'] as const,
  queryFn: () => historyPage({}, undefined, 50),
}

export type CancelInput = components['schemas']['CancelInput']
export type CancelResult = components['schemas']['CancelResult']

/** Withdraw, cancel, or ask an approver to accept cancelling started leave (FR-020). */
export const cancelLeave = (id: string, body: CancelInput) =>
  api.post<CancelResult>(`/leaves/${id}/cancel`, body)
