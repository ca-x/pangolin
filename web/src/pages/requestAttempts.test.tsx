import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { RequestDetailPage } from './OperationsPage'

/**
 * The detail card told the operator that the request detail "does not return
 * per-component costs" and "carries no per-attempt list". Both statements were
 * false: `/operations/requests/{id}` has returned `executions`, `usage` and
 * `cost_items` all along (`src/api/operations_api.rs`), and the page simply never
 * read it. A console that invents zeros is bad; one that denies data it holds is
 * worse, because the operator stops looking for it.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const bootstrap = { initialized: true, authenticated: true, user: { id: 'u1', email: 'owner@example.test', role: 'admin', language: 'en', theme: 'system:bronze', created_at: 1 }, capture_payloads: false, observability_available: true, branding: { instance_name: 'Pangolin', branding_name: 'Pangolin', favicon_url: '/logo.webp', onboarding_complete: true } }
const policy = { enabled: true, default_level: 'metadata', key_override_enabled: false, key_disable_allowed: false }
const detail = { id: 'internal-r2', request_id: 'r2', started_at: 1, finished_at: 2, endpoint: '/v1/chat/completions', provider: 'openai', requested_model: 'demo', resolved_model: 'demo', status_code: 200, error_kind: null, latency_ms: 12, input_tokens: 3, output_tokens: 4, cost_micros: 9, trace_id: 'trace-external', api_key_id: 'k1', ttft_ms: null, cached_tokens: 0, payload_captured: false, request_json: null, response_json: null }

/** What the record system returns for the same request: two attempts, priced. */
const record = {
  executions: [
    { id: 'e1', request_id: 'r2', provider_id: 'openai', provider_name: 'OpenAI main', model: 'demo', attempt: 1, status: 'failed', latency_ms: 5, retry_reason: 'upstream_5xx' },
    { id: 'e2', request_id: 'r2', provider_id: 'backup', provider_name: 'Backup provider', model: 'demo', attempt: 2, status: 'succeeded', latency_ms: 7, retry_reason: null },
  ],
  usage: [{ execution_id: 'e2', model_id: 'm1', input_tokens: 3, output_tokens: 4, cache_read_tokens: 0, cache_write_tokens: 0, reasoning_tokens: 0, total_cost_micros: 9, created_at: 2 }],
  cost_items: [{ execution_id: 'e2', kind: 'input', quantity: 3, unit_price_micros: 1, subtotal_micros: 3 }, { execution_id: 'e2', kind: 'output', quantity: 4, unit_price_micros: 1, subtotal_micros: 6 }],
}

const mockApi = vi.fn((input: RequestInfo | URL) => {
  const path = String(input)
  if (path.includes('/api/v1/bootstrap')) return json(bootstrap)
  if (path.includes('settings/request-logging')) return json(policy)
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('/observability/requests/')) return json(detail)
  if (path.includes('/operations/requests/')) return json(record)
  return json({ data: [], total: 0 })
})

const renderPage = () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/operations/requests/r2']}><ProjectProvider><Routes><Route path="/operations/requests/:id" element={<RequestDetailPage />} /></Routes></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

describe('the request detail reports the attempts and costs it is given', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('stops claiming the request detail returns no per-attempt list or per-component costs', async () => {
    vi.stubGlobal('fetch', mockApi)
    renderPage()
    expect(await screen.findByText('Usage breakdown')).toBeInTheDocument()
    expect(screen.queryByText(/carries no per-attempt list/i)).not.toBeInTheDocument()
    expect(screen.queryByText(/does not return per-component costs/i)).not.toBeInTheDocument()
  })

  it('shows both attempts and the priced components', async () => {
    vi.stubGlobal('fetch', mockApi)
    renderPage()
    expect(await screen.findByText('Usage breakdown')).toBeInTheDocument()
    expect(await screen.findByText('Backup provider')).toBeInTheDocument()
    expect(await screen.findByText('Input')).toBeInTheDocument()
    expect(await screen.findByText('Output')).toBeInTheDocument()
  })
})

/**
 * The card told the operator "the upstream status code itself is not persisted"
 * directly above a 429. That sentence was written when it was true; the record
 * system has stored `executions.http_status` since (`src/operations/lifecycle.rs`
 * sets it on every attempt), so the console was denying data it held — and in the
 * one place an operator goes to find out why a request failed.
 */
describe('the request detail reports the upstream status it recorded', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('prints the upstream status instead of claiming it was not persisted', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.includes('/api/v1/bootstrap')) return json(bootstrap)
      if (path.includes('settings/request-logging')) return json(policy)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/observability/requests/')) return json({ ...detail, status_code: 429, error_kind: 'upstream_error' })
      if (path.includes('/operations/requests/')) return json({ executions: [{ id: 'e1', attempt: 1, provider_id: 'openai', model: 'demo', status: 'failed', latency_ms: 5, http_status: 429 }], usage: [], cost_items: [] })
      return json({ data: [], total: 0 })
    }))
    renderPage()
    expect(await screen.findByText('Usage breakdown')).toBeInTheDocument()
    expect(await screen.findByText('Status code: 429')).toBeInTheDocument()
    expect(screen.queryByText(/is not persisted/i)).not.toBeInTheDocument()
  })
})
