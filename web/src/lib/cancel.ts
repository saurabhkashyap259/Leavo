import type { HistoryItem } from '../api/leaves'

/** Withdraw a pending request, cancel approved leave before it starts, or ask to cancel started leave. */
export type CancelKind = 'withdraw' | 'cancel' | 'request'

/** What cancelling `r` means on `today` (FR-020), or null when it cannot be cancelled. */
export function cancelKind(r: HistoryItem, today: string): CancelKind | null {
  if (r.status === 'pending') return 'withdraw'
  if (r.status === 'approved') return r.from > today ? 'cancel' : 'request'
  return null
}
