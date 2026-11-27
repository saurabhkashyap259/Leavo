// Thin fetch wrapper for /api/v1 (T052). Every request sends the CSRF header; errors become
// ApiError carrying a code from the catalogue (data-model.md) for translated messages.

export class ApiError extends Error {
  readonly status: number
  readonly code: string
  readonly details?: unknown

  constructor(status: number, code: string, message: string, details?: unknown) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
    this.details = details
  }
}

type Options = { idempotencyKey?: string; signal?: AbortSignal }

const BASE = '/api/v1'

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  opts: Options = {},
): Promise<T> {
  const headers = new Headers({ 'X-Requested-With': 'leavo', Accept: 'application/json' })
  // FormData sets its own multipart Content-Type with the boundary.
  const form = body instanceof FormData
  if (body !== undefined && !form) headers.set('Content-Type', 'application/json')
  if (opts.idempotencyKey) headers.set('Idempotency-Key', opts.idempotencyKey)

  let res: Response
  try {
    res = await fetch(`${BASE}${path}`, {
      method,
      headers,
      credentials: 'same-origin',
      body: body === undefined ? undefined : form ? body : JSON.stringify(body),
      signal: opts.signal,
    })
  } catch (e) {
    throw new ApiError(0, 'INTERNAL', e instanceof Error ? e.message : 'Network error')
  }

  if (res.status === 204) return undefined as T
  const text = await res.text()
  let data: unknown
  try {
    data = text ? JSON.parse(text) : undefined
  } catch {
    data = undefined
  }
  if (!res.ok) {
    const err = (data as { error?: { code?: string; message?: string; details?: unknown } })?.error
    throw new ApiError(
      res.status,
      err?.code ?? 'INTERNAL',
      err?.message ?? res.statusText ?? 'Request failed',
      err?.details,
    )
  }
  return data as T
}

export const api = {
  get: <T>(path: string, opts?: Options) => request<T>('GET', path, undefined, opts),
  post: <T = void>(path: string, body?: unknown, opts?: Options) =>
    request<T>('POST', path, body, opts),
  patch: <T = void>(path: string, body?: unknown, opts?: Options) =>
    request<T>('PATCH', path, body, opts),
  delete: <T = void>(path: string, opts?: Options) => request<T>('DELETE', path, undefined, opts),
}

/** A fresh key per form submission, so retries of the same submission are deduplicated. */
export function newIdempotencyKey(): string {
  return crypto.randomUUID()
}

/** Translated message for an error: `errors.<CODE>` if defined, else the server message. */
export function errorMessage(err: unknown, t: (key: string) => string): string {
  if (err instanceof ApiError) {
    const key = `errors.${err.code}`
    const translated = t(key)
    return translated !== key ? translated : err.message
  }
  return t('errors.unknown')
}
