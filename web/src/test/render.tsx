import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from '@tanstack/react-router'
import { render } from '@testing-library/react'
import type { ReactElement } from 'react'
import { vi } from 'vitest'

/** Render a component inside a minimal router so <Link> works. */
export async function renderWithRouter(ui: ReactElement) {
  const root = createRootRoute()
  const page = createRoute({ getParentRoute: () => root, path: '/', component: () => ui })
  const other = createRoute({ getParentRoute: () => root, path: '$', component: () => null })
  const router = createRouter({
    routeTree: root.addChildren([page, other]),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  await router.load()
  return render(<RouterProvider router={router} />)
}

export const jsonResponse = (status: number, body?: unknown) =>
  new Response(body === undefined ? null : JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })

/** Same as `renderWithRouter`, with a fresh React Query client (no retries). */
export async function renderWithClient(ui: ReactElement) {
  const { QueryClient, QueryClientProvider } = await import('@tanstack/react-query')
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return renderWithRouter(<QueryClientProvider client={client}>{ui}</QueryClientProvider>)
}

type Call = { method: string; path: string; query: string; body: unknown; headers: Headers }
type Handler = (body: unknown, call: Call) => Response | Promise<Response>

/**
 * Mock `fetch` by `METHOD /api/v1/path` (query string ignored for matching); records every
 * call. JSON bodies are parsed; other bodies (FormData) are passed through.
 */
export function mockApi(routes: Record<string, Handler>) {
  const calls: Call[] = []
  const spy = vi.spyOn(globalThis, 'fetch').mockImplementation(async (input, init) => {
    const method = (init?.method ?? 'GET').toUpperCase()
    const [path, query = ''] = String(input)
      .replace(/^\/api\/v1/, '')
      .split('?')
    const raw = init?.body
    const body = typeof raw === 'string' ? JSON.parse(raw) : (raw ?? undefined)
    const call = { method, path, query, body, headers: new Headers(init?.headers) }
    calls.push(call)
    const handler = routes[`${method} ${path}`]
    if (!handler) return jsonResponse(404, { error: { code: 'NOT_FOUND', message: path } })
    return handler(body, call)
  })
  return {
    calls,
    spy,
    last: (key: string) => calls.filter((c) => `${c.method} ${c.path}` === key).at(-1),
  }
}
