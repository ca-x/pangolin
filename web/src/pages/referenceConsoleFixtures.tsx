import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { vi } from 'vitest'
import { ProjectProvider, useProject } from '../project'
import type { ReactNode } from 'react'

export const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
export const channels = [{ id: 'c1', name: 'Primary', kind: 'openai', enabled: true }, { id: 'c2', name: 'Gemini', kind: 'gemini', enabled: true }]
export const models = [{ id: 'm1', provider_id: 'c1', public_name: 'Fast', upstream_name: 'fast', capabilities: ['chat', 'responses'], enabled: true, lifecycle: 'active' }, { id: 'm2', provider_id: 'c2', public_name: 'Gemini model', upstream_name: 'gemini', capabilities: ['gemini'], enabled: true, lifecycle: 'active' }]
export const credentials = [{ id: 'k1', provider_id: 'c1', provider_name: 'Primary', suffix: '1111', enabled: true, state: 'ready', discovery_status: 'known', discovery_model_count: 0, discovery_last_success_at: 1700000000 }, { id: 'k2', provider_id: 'c1', provider_name: 'Primary', suffix: '2222', enabled: true, state: 'ready', discovery_status: 'stale', discovery_model_count: 3, discovery_last_success_at: 1700000000 }, { id: 'k3', provider_id: 'c2', provider_name: 'Gemini', suffix: '3333', enabled: true, discovery_status: 'unknown', discovery_model_count: null, discovery_last_success_at: null }, { id: 'disabled', provider_id: 'c1', suffix: '9999', enabled: false, discovery_status: 'unknown' }]
export function mockApi(overrides?: (path: string, init?: RequestInit) => Promise<Response> | undefined) {
  return vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input)
    const override = overrides?.(path, init)
    if (override) return override
    if (path.endsWith('/projects')) return json([{ id: 'p1', name: 'One', enabled: true, is_default: true }, { id: 'p2', name: 'Two', enabled: true }])
    if (path.includes('/permissions')) return json(['*'])
    if (init?.method === 'POST') return json({ job_id: 'job1' })
    if (path.includes('/operations/channels')) return json({ data: path.includes('/p2/') ? [channels[1]] : channels, total: 2 })
    if (path.includes('/operations/models')) return json({ data: path.includes('/p2/') ? [models[1]] : models, total: 2 })
    if (path.includes('/operations/credentials')) return json({ data: credentials, total: 4 })
    return json({ data: [], total: 0 })
  })
}
function SwitchProject() { const { setProjectId } = useProject(); return <button onClick={() => setProjectId('p2')}>Switch project</button> }
export function mount(page: ReactNode, path: string, fetchMock: ReturnType<typeof mockApi>) {
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[path]}><ProjectProvider><SwitchProject />{page}</ProjectProvider></MemoryRouter></QueryClientProvider>)
}
