// T052 — API client: base path, CSRF header, idempotency keys, error mapping.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, api, errorMessage, newIdempotencyKey } from './client'

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })

afterEach(() => vi.restoreAllMocks())

describe('api client', () => {
  it('prefixes /api/v1, sends the CSRF header and same-origin cookies', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(json(200, { id: 'u1' }))
    const me = await api.get<{ id: string }>('/me')
    expect(me.id).toBe('u1')
    const [url, init] = fetchMock.mock.calls[0]
    expect(url).toBe('/api/v1/me')
    expect(init?.credentials).toBe('same-origin')
    expect(new Headers(init?.headers).get('X-Requested-With')).toBe('leavo')
  })

  it('sends JSON bodies and an Idempotency-Key when given', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(json(201, { id: 'l1' }))
    await api.post('/leaves', { reason: 'Trip' }, { idempotencyKey: 'key-1' })
    const [, init] = fetchMock.mock.calls[0]
    const headers = new Headers(init?.headers)
    expect(init?.method).toBe('POST')
    expect(headers.get('Content-Type')).toBe('application/json')
    expect(headers.get('Idempotency-Key')).toBe('key-1')
    expect(init?.body).toBe(JSON.stringify({ reason: 'Trip' }))
  })

  it('returns undefined for 204', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(null, { status: 204 }))
    await expect(api.post('/auth/logout')).resolves.toBeUndefined()
  })

  it('throws ApiError with the catalogue code, message and details', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      json(409, {
        error: { code: 'ALREADY_DECIDED', message: 'Approved by Priya', details: { by: 'Priya' } },
      }),
    )
    const err = (await api
      .post('/approvals/leave/1/decide', {})
      .catch((e: unknown) => e)) as ApiError
    expect(err).toBeInstanceOf(ApiError)
    expect(err.status).toBe(409)
    expect(err.code).toBe('ALREADY_DECIDED')
    expect(err.message).toBe('Approved by Priya')
    expect(err.details).toEqual({ by: 'Priya' })
  })

  it('maps non-JSON failures and network errors to INTERNAL', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(new Response('oops', { status: 502 }))
    const err = (await api.get('/me').catch((e: unknown) => e)) as ApiError
    expect(err.code).toBe('INTERNAL')
    vi.spyOn(globalThis, 'fetch').mockRejectedValueOnce(new TypeError('offline'))
    const net = (await api.get('/me').catch((e: unknown) => e)) as ApiError
    expect(net).toBeInstanceOf(ApiError)
    expect(net.code).toBe('INTERNAL')
  })
})

describe('helpers', () => {
  it('creates unique idempotency keys', () => {
    expect(newIdempotencyKey()).not.toBe(newIdempotencyKey())
    expect(newIdempotencyKey()).toMatch(/^[0-9a-f-]{36}$/)
  })

  it('maps error codes to translation keys with a fallback', () => {
    const t = (k: string) => (k === 'errors.LOCKED' ? 'Locked!' : k)
    expect(errorMessage(new ApiError(429, 'LOCKED', 'x'), t)).toBe('Locked!')
    expect(errorMessage(new ApiError(500, 'WHO_KNOWS', 'Server says'), t)).toBe('Server says')
  })
})
