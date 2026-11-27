// T053 — navigation by role (PRD §2).
import { describe, expect, it } from 'vitest'
import { navItemsFor } from './nav'

const paths = (role: 'employee' | 'approver' | 'admin') => navItemsFor(role).map((i) => i.to)

describe('navItemsFor', () => {
  const everyone = [
    '/dashboard',
    '/leave/apply',
    '/leave/history',
    '/comp-offs',
    '/holidays',
    '/team',
  ]

  it('employees see the employee screens only', () => {
    expect(paths('employee')).toEqual(everyone)
  })

  it('approvers also see Approvals', () => {
    expect(paths('approver')).toEqual([...everyone, '/approvals'])
  })

  it('admins see Approvals and Admin', () => {
    expect(paths('admin')).toEqual([...everyone, '/approvals', '/admin'])
  })

  it('every item has a translation key', () => {
    for (const item of navItemsFor('admin')) expect(item.label).toMatch(/^nav\./)
  })
})
