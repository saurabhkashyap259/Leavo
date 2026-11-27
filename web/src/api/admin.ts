import { api } from './client'
import type { components } from './schema'

export type OrgSettings = components['schemas']['OrgSettings']
export type PatchOrg = components['schemas']['PatchOrg']
export type Domain = components['schemas']['Domain']
export type LeaveType = components['schemas']['LeaveType']
export type LeaveTypeInput = components['schemas']['LeaveTypeInput']
export type Grant = components['schemas']['Grant']

export const orgQuery = {
  queryKey: ['admin', 'org'] as const,
  queryFn: () => api.get<OrgSettings>('/admin/org'),
}
export const domainsQuery = {
  queryKey: ['admin', 'domains'] as const,
  queryFn: () => api.get<Domain[]>('/admin/org/domains'),
}
export const leaveTypesQuery = {
  queryKey: ['admin', 'leave-types'] as const,
  queryFn: () => api.get<LeaveType[]>('/admin/leave-types'),
}
export const grantsQuery = (typeId: string) => ({
  queryKey: ['admin', 'leave-types', typeId, 'grants'] as const,
  queryFn: () => api.get<Grant[]>(`/admin/leave-types/${typeId}/grants`),
})

export const patchOrg = (body: PatchOrg) => api.patch<OrgSettings>('/admin/org', body)
export const addDomain = (domain: string) => api.post<Domain>('/admin/org/domains', { domain })
export const removeDomain = (id: string) => api.delete(`/admin/org/domains/${id}`)
export const createLeaveType = (body: LeaveTypeInput) =>
  api.post<LeaveType>('/admin/leave-types', body)
export const patchLeaveType = (id: string, body: LeaveTypeInput) =>
  api.patch<LeaveType>(`/admin/leave-types/${id}`, body)
export const grantLeaveType = (id: string, email: string) =>
  api.post<Grant>(`/admin/leave-types/${id}/grants`, { email })
export const revokeGrant = (id: string, userId: string) =>
  api.delete(`/admin/leave-types/${id}/grants/${userId}`)

export type CarryForwardRow = components['schemas']['CarryForwardRow']

/** What year end does to carry-forward types when `year` closes (FR-039). */
export const carryForwardQuery = (year: number) => ({
  queryKey: ['admin', 'carry-forward', year] as const,
  queryFn: () => api.get<CarryForwardRow[]>(`/admin/carry-forward/preview?year=${year}`),
})

// --- People, groups, balances, dashboard, reports, audit (US10) ---

export type AdminUser = components['schemas']['AdminUser']
export type UserPage = components['schemas']['UserPage']
export type InviteInput = components['schemas']['InviteInput']
export type InviteImportResult = components['schemas']['InviteImportResult']
export type ApproverGroup = components['schemas']['ApproverGroup']
export type UserBalances = components['schemas']['UserBalances']
export type LedgerItem = components['schemas']['LedgerItem']
export type AdminDashboard = components['schemas']['AdminDashboard']
export type ReportRow = components['schemas']['ReportRow']
export type AuditItem = components['schemas']['AuditItem']
export type AuditPage = components['schemas']['AuditPage']

export type UserFilters = { q?: string; role?: string; status?: string; group?: string }

const qs = (params: Record<string, string | number | undefined>) => {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== '') p.set(k, String(v))
  const s = p.toString()
  return s ? `?${s}` : ''
}

export const usersQuery = (f: UserFilters) => ({
  queryKey: ['admin', 'users', f] as const,
  queryFn: () => api.get<UserPage>(`/admin/users${qs({ ...f, limit: 200 })}`),
})

export const userQuery = (id: string) => ({
  queryKey: ['admin', 'user', id] as const,
  queryFn: () => api.get<AdminUser>(`/admin/users/${id}`),
})

export const inviteUser = (body: InviteInput) => api.post<AdminUser>('/admin/users', body)

export function importUsers(file: File, dryRun: boolean) {
  const form = new FormData()
  form.append('file', file)
  return api.post<InviteImportResult>(`/admin/users/import?dry_run=${dryRun}`, form)
}

export const updateUser = (id: string, body: Record<string, unknown>) =>
  api.patch<AdminUser>(`/admin/users/${id}`, body)

export const anonymiseUser = (id: string) => api.post(`/admin/users/${id}/anonymise`, {})

export const exportUser = (id: string) => api.post<unknown>(`/admin/users/${id}/export`, {})

export const groupsQuery = {
  queryKey: ['admin', 'groups'] as const,
  queryFn: () => api.get<ApproverGroup[]>('/admin/approver-groups'),
}

export const createGroup = (body: { name: string; member_ids: string[] }) =>
  api.post<ApproverGroup>('/admin/approver-groups', body)

export const updateGroup = (id: string, body: { name?: string; member_ids?: string[] }) =>
  api.patch<ApproverGroup>(`/admin/approver-groups/${id}`, body)

export const deleteGroup = (id: string) => api.delete(`/admin/approver-groups/${id}`)

export const userBalancesQuery = (id: string, year: number) => ({
  queryKey: ['admin', 'user', id, 'balances', year] as const,
  queryFn: () => api.get<UserBalances>(`/admin/users/${id}/balances?year=${year}`),
})

export const adjustBalance = (
  id: string,
  body: { leave_type_id: string; leave_year: number; delta: number; note: string },
) => api.post<LedgerItem>(`/admin/users/${id}/adjustments`, body)

export const dashboardQuery = {
  queryKey: ['admin', 'dashboard'] as const,
  queryFn: () => api.get<AdminDashboard>('/admin/dashboard'),
}

export type GroupBy = 'type' | 'user' | 'month'

export const reportQuery = (groupBy: GroupBy, year: number) => ({
  queryKey: ['admin', 'reports', groupBy, year] as const,
  queryFn: () => api.get<ReportRow[]>(`/admin/reports/leaves?group_by=${groupBy}&year=${year}`),
})

export type AuditFilters = {
  action?: string
  actor?: string
  entity?: string
  from?: string
  to?: string
}

export const auditPage = (f: AuditFilters, cursor?: string) =>
  api.get<AuditPage>(`/admin/audit-log${qs({ ...f, cursor })}`)

// --- Import (US11) ---

export type ImportReport = components['schemas']['ImportReport']
export type ImportSource = 'keka' | 'csv'

export function runImport(
  source: ImportSource,
  files: { balances: File; history?: File | null },
  options: { dryRun: boolean; leaveTypeMap?: Record<string, string>; leaveYear?: number },
) {
  const form = new FormData()
  form.append('balances', files.balances)
  if (files.history) form.append('history', files.history)
  if (source === 'keka') {
    if (options.leaveTypeMap) form.append('leave_type_map', JSON.stringify(options.leaveTypeMap))
    if (options.leaveYear) form.append('leave_year', String(options.leaveYear))
  }
  return api.post<ImportReport>(`/admin/import/${source}?dry_run=${options.dryRun}`, form)
}

// --- Instance (US12) ---

export type InstanceStatus = components['schemas']['InstanceStatus']

export const instanceQuery = {
  queryKey: ['admin', 'instance'] as const,
  queryFn: () => api.get<InstanceStatus>('/admin/instance'),
}

export const sendInstanceTestEmail = () =>
  api.post<{ to: string }>('/admin/instance/test-email', {})

export const signEveryoneOut = () => api.post('/admin/org/sign-out-all', {})
