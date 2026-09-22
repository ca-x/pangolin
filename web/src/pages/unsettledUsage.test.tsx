import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { usageMeasured } from '../observability'
import { ProjectProvider } from '../project'
import OperationsPage, { RequestDetailPage } from './OperationsPage'

/**
 * A 502 whose usage was never settled rendered "Input tokens 0 / Tokens: 0 /
 * Cost: $0.000000" — an invented measurement next to an alert saying the record
 * is the gateway's normalized rejection. The projection's own verdict is
 * `error_kind`; a failure that recorded no tokens and no cost is the rejection
 * record, and every one of those figures is `—`.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const bootstrap = { initialized: true, authenticated: true, user: { id: 'u1', email: 'owner@example.test', role: 'admin', language: 'en', theme: 'system:bronze', created_at: 1 }, capture_payloads: false, observability_available: true, branding: { instance_name: 'Pangolin', branding_name: 'Pangolin', favicon_url: '/logo.webp', onboarding_complete: true } }
const policy = { enabled: true, default_level: 'metadata', key_override_enabled: false, key_disable_allowed: false }
const row = (overrides: Record<string, unknown> = {}) => ({ internal_id: `internal-${overrides.request_id ?? 'r1'}`, request_id: 'r1', started_at: 1, endpoint: '/v1/chat/completions', provider: 'openai', requested_model: 'demo', resolved_model: 'demo', status_code: 200, error_kind: null, latency_ms: 12, input_tokens: 3, output_tokens: 4, cost_micros: 0, ...overrides })
const detail = (overrides: Record<string, unknown> = {}) => ({ ...row({ id: 'internal-r2', request_id: 'r2', status_code: 502, error_kind: 'failed', input_tokens: 0, output_tokens: 0, cost_micros: 0 }), finished_at: 2, trace_id: 'trace-external', api_key_id: 'k1', ttft_ms: null, cached_tokens: 0, payload_captured: false, request_json: null, response_json: null, ...overrides })

const mockApi = (options: { rows?: unknown[]; detail?: unknown } = {}) => vi.fn((input: RequestInfo | URL) => {
  const path = String(input)
  if (path.includes('/api/v1/bootstrap')) return json(bootstrap)
  if (path.includes('settings/request-logging')) return json(policy)
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('/observability/requests/')) return json(options.detail ?? detail())
  if (path.includes('/observability/requests')) return json({ data: options.rows ?? [], total: (options.rows ?? []).length, offset: 0, limit: 25 })
  return json({ data: [], total: 0 })
})

const renderPage = (page: React.ReactElement, path = '/operations') => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[path]}><ProjectProvider><Routes><Route path="/operations" element={page} /><Route path="/operations/requests/:id" element={<RequestDetailPage />} /></Routes></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

describe('an unsettled request reports unknown usage, not zero', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('prints — for every token figure and the cost of a failed request', async () => {
    vi.stubGlobal('fetch', mockApi())
    renderPage(<OperationsPage />, '/operations/requests/r2')
    expect(await screen.findByText('Request failed')).toBeInTheDocument()
    expect(screen.getByText('Tokens: —')).toBeInTheDocument()
    expect(screen.getByText('Cost: —')).toBeInTheDocument()
    expect(screen.queryByText('Cost: $0.000000')).not.toBeInTheDocument()
    expect(screen.queryByText('Tokens: 0')).not.toBeInTheDocument()
  })

  it('keeps the numbers a partial settlement really measured', async () => {
    vi.stubGlobal('fetch', mockApi({ detail: detail({ input_tokens: 7, output_tokens: 9, cost_micros: 1234 }) }))
    renderPage(<OperationsPage />, '/operations/requests/r2')
    expect(await screen.findByText('Cost: $0.001234')).toBeInTheDocument()
    expect(screen.getByText('Tokens: 16')).toBeInTheDocument()
  })

  it('keeps a measured zero on a request that succeeded', async () => {
    vi.stubGlobal('fetch', mockApi({ detail: detail({ status_code: 200, error_kind: null, input_tokens: 0, output_tokens: 0, cost_micros: 0 }) }))
    renderPage(<OperationsPage />, '/operations/requests/r2')
    expect(await screen.findByText('Cost: $0.000000')).toBeInTheDocument()
    expect(screen.getByText('Tokens: 0')).toBeInTheDocument()
  })

  it('prints — in the list cost column for a rejected row and a zero for a settled one', async () => {
    vi.stubGlobal('fetch', mockApi({ rows: [row({ request_id: 'ok', cost_micros: 0 }), row({ request_id: 'bad', status_code: 502, error_kind: 'failed', input_tokens: 0, output_tokens: 0, cost_micros: 0 })] }))
    renderPage(<OperationsPage />)
    await screen.findAllByText('502')
    const tableRow = (status: string) => screen.getAllByText(status).map((node) => node.closest('tr')).find((tr): tr is HTMLTableRowElement => tr !== null)!
    const bad = tableRow('502')
    const good = tableRow('200')
    expect(bad.textContent).toContain('—')
    expect(bad.textContent).not.toContain('$0.000000')
    expect(good.textContent).toContain('$0.000000')
  })

  it('answers the question from the record, for every kind the projection emits', () => {
    expect(usageMeasured(row({ error_kind: null }))).toBe(true)
    expect(usageMeasured(row({ error_kind: 'usage_unavailable' }))).toBe(false)
    expect(usageMeasured(row({ status_code: 502, error_kind: 'failed', input_tokens: 0, output_tokens: 0 }))).toBe(false)
    expect(usageMeasured(row({ status_code: 502, error_kind: 'failed', output_tokens: 2 }))).toBe(true)
    expect(usageMeasured(row({ status_code: 502, error_kind: 'failed', cost_micros: 5 }))).toBe(true)
    expect(usageMeasured(row({ error_kind: 'local_failure', input_tokens: 0, output_tokens: 0 }))).toBe(false)
  })
})
