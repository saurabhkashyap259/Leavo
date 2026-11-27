import { api } from './client'
import type { components } from './schema'

export type SetupStatus = components['schemas']['SetupStatus']
export type PackInfo = components['schemas']['PackInfo']
export type CompleteSetup = components['schemas']['CompleteSetup']

export const setupStatusQuery = {
  queryKey: ['setup-status'] as const,
  queryFn: () => api.get<SetupStatus>('/setup/status'),
  staleTime: Infinity,
}

export const verifySetupToken = (setup_token: string) =>
  api.post('/setup/verify-token', { setup_token })

export const requestSetupCode = (email: string, setup_token: string) =>
  api.post('/auth/code', { email, purpose: 'setup', setup_token })

export const sendTestEmail = (setup_token: string, to: string) =>
  api.post<{ ok: boolean; error?: string }>('/setup/test-email', { setup_token, to })

export const completeSetup = (body: CompleteSetup) => api.post('/setup/complete', body)
