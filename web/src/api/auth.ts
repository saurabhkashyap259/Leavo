import { api } from './client'

export type Purpose = 'login' | 'signup'

export const requestCode = (body: {
  email: string
  purpose: Purpose
  full_name?: string
  date_of_joining?: string
}) => api.post('/auth/code', body)

export const verifyCode = (body: {
  email: string
  code: string
  purpose: Purpose
  keep_signed_in: boolean
}) => api.post('/auth/verify', body)

export const logout = (all = false) => api.post(all ? '/auth/logout?all=true' : '/auth/logout')
