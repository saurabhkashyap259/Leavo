import type { Role } from '../api/me'

export type NavItem = { to: string; label: string }

const EVERYONE: NavItem[] = [
  { to: '/dashboard', label: 'nav.dashboard' },
  { to: '/leave/apply', label: 'nav.applyLeave' },
  { to: '/leave/history', label: 'nav.history' },
  { to: '/comp-offs', label: 'nav.compOffs' },
  { to: '/holidays', label: 'nav.holidays' },
  { to: '/team', label: 'nav.team' },
]

/** Navigation for a role (PRD §2): approvers add Approvals; admins add Approvals and Admin. */
export function navItemsFor(role: Role): NavItem[] {
  const items = [...EVERYONE]
  if (role === 'approver' || role === 'admin')
    items.push({ to: '/approvals', label: 'nav.approvals' })
  if (role === 'admin') items.push({ to: '/admin', label: 'nav.admin' })
  return items
}
