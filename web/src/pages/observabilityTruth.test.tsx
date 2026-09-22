import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { errorKindKey, formatMicros } from '../observability'
import { ProjectProvider } from '../project'
import OverviewPage from './OverviewPage'
import OperationsPage, { RequestDetailPage } from './OperationsPage'

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const bootstrap = (available?: boolean) => ({ initialized: true, authenticated: true, user: { id: 'u1', email: 'owner@example.test', role: 'admin', language: 'en', theme: 'system:bronze', created_at: 1 }, capture_payloads: false, ...(available === undefined ? {} : { observability_available: available }), branding: { instance_name: 'Pangolin', branding_name: 'Pangolin', favicon_url: '/logo.webp', onboarding_complete: true } })
const policy = (default_level = 'metadata', enabled = true) => ({ enabled, default_level, key_override_enabled: false, key_disable_allowed: false })
const summary = (overrides: Record<string, unknown> = {}) => ({ requests: 0, errors: 0, error_rate: 0, p95_latency_ms: 0, input_tokens: 0, output_tokens: 0, cost_micros: 0, series: [], ...overrides })
const row = (overrides: Record<string, unknown> = {}) => ({ internal_id: `internal-${overrides.request_id ?? 'r1'}`, request_id: 'r1', started_at: 1, endpoint: '/v1/chat/completions', provider: 'openai', requested_model: 'demo', resolved_model: 'demo', status_code: 200, latency_ms: 12, input_tokens: 1, output_tokens: 1, cost_micros: 0, ...overrides })
const detail = (overrides: Record<string, unknown> = {}) => ({ ...row({ id: 'internal-r2', request_id: 'r2', status_code: 502, cost_micros: 1234 }), finished_at: 2, trace_id: 'trace-external', api_key_id: 'k1', error_kind: 'usage_unavailable', ttft_ms: null, cached_tokens: 3, payload_captured: false, request_json: null, response_json: null, ...overrides })

type MockOptions = { available?: boolean; policy?: unknown; summary?: unknown; rows?: unknown[]; detail?: unknown; traces?: unknown }
const mockApi = (options: MockOptions = {}) => vi.fn((input: RequestInfo | URL) => {
  const path = String(input)
  if (path.includes('/api/v1/bootstrap')) return json(bootstrap(options.available))
  if (path.includes('settings/request-logging')) return json(options.policy ?? policy())
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('/observability/summary')) return json(options.summary ?? summary())
  if (path.includes('/observability/requests/')) return json(options.detail ?? detail())
  if (path.includes('/observability/requests')) return json({ data: options.rows ?? [], total: (options.rows ?? []).length, offset: 0, limit: 25 })
  if (path.includes('/operations/usage')) return json({ data: [{ id: 'u1', execution_id: 'e1', input_tokens: 1, output_tokens: 1, cache_read_tokens: 0, cost_micros: 1234, settlement_kind: 'final' }], total: 1 })
  if (path.includes('/operations/cost-items')) return json({ data: [{ id: 'c1', usage_id: 'u1', component_id: 'input', quantity: 1, subtotal_micros: 1234 }], total: 1 })
  if (path.includes('/operations/traces')) return json(options.traces ?? { data: [], total: 0 })
  return json({ data: [], total: 0 })
})

const renderPage = (page: React.ReactElement, path = '/operations') => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[path]}><ProjectProvider>{page}</ProjectProvider></MemoryRouter></QueryClientProvider>)
}
/** The detail page reads its identifier from the route, so it needs one. */
const renderDetail = (path = '/operations/requests/r2', state?: unknown) => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[state === undefined ? path : { pathname: path, state }]}><ProjectProvider><Routes><Route path="/operations/requests/:id" element={<RequestDetailPage />} /></Routes></ProjectProvider></MemoryRouter></QueryClientProvider>)
}
const stat = (id: string) => document.querySelector(`[data-stat="${id}"]`)?.textContent

describe('the console tells "no traffic" apart from "not recorded"', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('renders an unmeasured summary as unknown, never as zero', async () => {
    vi.stubGlobal('fetch', mockApi({ available: false, summary: summary() }))
    renderPage(<OverviewPage />)
    expect(await screen.findByRole('alert')).toHaveTextContent('Analytics projection unavailable')
    expect(stat('requests')).toBe('—')
    expect(stat('errors')).toBe('—')
    expect(stat('latency')).toBe('—')
    expect(stat('cost')).toBe('—')
    // The old page turned an unreadable projection into a plausible-looking zero.
    expect(stat('requests')).not.toBe('0')
  })

  it('does not tell an operator with a channel to configure one when nothing is recorded', async () => {
    vi.stubGlobal('fetch', mockApi({ available: false }))
    renderPage(<OverviewPage />)
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(screen.queryByText('Last 24 hours: no requests recorded')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Configure a channel' })).not.toBeInTheDocument()
  })

  it('names the logging policy as the reason when the projection is healthy but silent', async () => {
    vi.stubGlobal('fetch', mockApi({ available: true, policy: policy('off') }))
    renderPage(<OverviewPage />)
    expect(await screen.findByRole('alert')).toHaveTextContent('Requests are not being recorded')
    expect(stat('requests')).toBe('—')
    expect(stat('cost')).toBe('—')
    expect(screen.queryByText('Last 24 hours: no requests recorded')).not.toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Request logging' })).toHaveAttribute('href', '/system')
  })

  it('still reports a measured zero as zero, with the onboarding empty state', async () => {
    vi.stubGlobal('fetch', mockApi({ available: true }))
    renderPage(<OverviewPage />)
    expect(await screen.findByText('Last 24 hours: no requests recorded')).toBeInTheDocument()
    expect(stat('requests')).toBe('0')
    expect(stat('cost')).toBe('$0.000000')
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('plots the error counts the backend reported, rather than a reconstructed rate', async () => {
    vi.stubGlobal('fetch', mockApi({ available: true, summary: summary({ requests: 5, errors: 2, error_rate: 0.4, p95_latency_ms: 12, cost_micros: 10, input_tokens: 5, output_tokens: 0, series: [{ bucket: 1700000000, requests: 5, errors: 2, latency_ms: 10 }] }) }))
    renderPage(<OverviewPage />)
    expect(await screen.findByText('2 failed')).toBeInTheDocument()
    // The chart carries an error series of its own; the request line is solid.
    expect(document.querySelector('svg.trend-chart path[stroke-dasharray]')).not.toBeNull()
    expect(document.querySelector('.chart-table')?.textContent).toContain('2')
  })

  it('keeps the banner while the projection is degraded, with a retry', async () => {
    vi.stubGlobal('fetch', mockApi({ available: false }))
    renderPage(<OperationsPage />)
    const banner = await screen.findByRole('alert')
    expect(banner).toHaveTextContent('Analytics projection unavailable')
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument()
  })
})

describe('money has one representation', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('prints the same micro-USD value identically on the list and the overview', async () => {
    vi.stubGlobal('fetch', mockApi({ available: true, rows: [row({ cost_micros: 1234 })], summary: summary({ requests: 1, cost_micros: 1234, error_rate: 0 }) }))
    const { unmount } = renderPage(<OperationsPage />)
    expect(await screen.findByText('$0.001234')).toBeInTheDocument()
    // The neighbouring surfaces must not round to a different precision.
    expect(screen.queryByText('$0.0012')).not.toBeInTheDocument()
    expect(screen.queryByText('$0.001234000')).not.toBeInTheDocument()
    unmount()
    renderPage(<OverviewPage />)
    expect(await screen.findByText('$0.001234')).toBeInTheDocument()
    expect(stat('cost')).toBe('$0.001234')
  })

  it('renders the usage and cost-item columns as money, not as raw micro-USD', async () => {
    vi.stubGlobal('fetch', mockApi({ available: true }))
    renderPage(<OperationsPage />)
    await userEvent.click(await screen.findByRole('tab', { name: 'Usage' }))
    expect(await screen.findByText('$0.001234')).toBeInTheDocument()
    expect(screen.queryByText('1234')).not.toBeInTheDocument()
    await userEvent.click(screen.getByRole('tab', { name: 'Cost items' }))
    expect(await screen.findByText('$0.001234')).toBeInTheDocument()
    expect(screen.queryByText('1234')).not.toBeInTheDocument()
  })

  it('renders an unmeasured amount as unknown rather than as a number', () => {
    expect(formatMicros(null)).toBe('—')
    expect(formatMicros(undefined)).toBe('—')
    expect(formatMicros(-1)).toBe('—')
    expect(formatMicros(0)).toBe('$0.000000')
  })
})

describe('a failure carries a reason', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('shows error_kind and the fields the detail response already carries', async () => {
    vi.stubGlobal('fetch', mockApi({ available: true }))
    renderDetail()
    expect(await screen.findByText('Upstream returned no usage')).toBeInTheDocument()
    expect(screen.getByText('Request failed')).toBeInTheDocument()
    // The card used to say the upstream status "is not persisted", which stopped
    // being true when the projection started recording it. What this view can
    // honestly say is that it does not carry that code; when it does, the real
    // status is printed instead (see `requestAttempts.test.tsx`).
    expect(screen.getByText(/does not carry the upstream status code/i)).toBeInTheDocument()
    expect(screen.queryByText(/is not persisted/i)).not.toBeInTheDocument()
    expect(screen.getByText('k1')).toBeInTheDocument()
    expect(screen.getByText('/v1/chat/completions')).toBeInTheDocument()
    expect(screen.getByText('Cache tokens')).toBeInTheDocument()
    // `usage_unavailable` is the projection saying it never settled this request,
    // so the cost is unknown — the money format for a settled one is asserted in
    // `unsettledUsage.test.tsx`.
    expect(screen.getByText('Cost: —')).toBeInTheDocument()
    expect(screen.queryByText('Cost: $0.001234')).not.toBeInTheDocument()
    // The record system does carry an attempt list for this request; this mock
    // answers that read with an empty document, so the page reports the count as
    // unmeasured. What it must never do again is claim the response has no such
    // list — that sentence was false, and it sent operators to the trace view for
    // data this page already held.
    expect(screen.getByText('Attempts: —')).toBeInTheDocument()
    expect(screen.queryByText(/carries no per-attempt list/i)).not.toBeInTheDocument()
  })

  it('links the trace id to the trace view through the identifier that view resolves', async () => {
    vi.stubGlobal('fetch', mockApi({ available: true, traces: { data: [{ id: 'internal-trace', external_id: 'trace-external' }], total: 1 } }))
    renderDetail()
    expect(await screen.findByRole('link', { name: 'trace-external' })).toHaveAttribute('href', '/operations/traces/internal-trace')
  })

  it('offers a copy action when the trace cannot be resolved, instead of a dead link', async () => {
    vi.stubGlobal('fetch', mockApi({ available: true, traces: { data: [], total: 0 } }))
    renderDetail()
    expect(await screen.findByRole('button', { name: 'Copy trace ID' })).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'trace-external' })).not.toBeInTheDocument()
  })

  it('steps through the page of requests the detail was opened from', async () => {
    vi.stubGlobal('fetch', mockApi({ available: true }))
    renderDetail('/operations/requests/r2', { from: '/operations', tab: 'requests', ids: ['r1', 'r2', 'r3'], index: 1 })
    expect(await screen.findByText('2 of 3')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Next request' }))
    expect(await screen.findByText('3 of 3')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Next request' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Previous request' })).toBeEnabled()
  })

  it('translates every error kind it can name, in both languages', () => {
    for (const kind of ['failed', 'local_failure', 'usage_unavailable', 'cancelled', 'interrupted']) {
      const key = errorKindKey(kind)
      expect(key).toBeTruthy()
      for (const language of ['en', 'zh-CN']) expect(i18n.exists(key!, { lng: language })).toBe(true)
    }
    expect(errorKindKey('provider_specific_thing')).toBeNull()
  })
})

describe('filters only offer what the projection emitted', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('cannot offer a status code that never occurred', async () => {
    vi.stubGlobal('fetch', mockApi({ available: true, rows: [row({ status_code: 200 }), row({ request_id: 'r2', status_code: 429 })] }))
    renderPage(<OperationsPage />)
    expect(await screen.findByRole('button', { name: 'Pause' })).toBeInTheDocument()
    await userEvent.click(screen.getByRole('combobox', { name: 'Status code' }))
    expect(await screen.findByRole('option', { name: '200' })).toBeInTheDocument()
    expect(screen.getByRole('option', { name: '429' })).toBeInTheDocument()
    expect(screen.queryByRole('option', { name: '599' })).not.toBeInTheDocument()
  })

  it('builds the provider and model facets from observed values', async () => {
    vi.stubGlobal('fetch', mockApi({ available: true, rows: [row({ provider: 'openai', requested_model: 'demo' })] }))
    renderPage(<OperationsPage />)
    await userEvent.click(await screen.findByRole('combobox', { name: 'Provider' }))
    expect(await screen.findByRole('option', { name: 'openai' })).toBeInTheDocument()
    await userEvent.keyboard('{Escape}')
    await userEvent.click(screen.getByRole('combobox', { name: 'Model' }))
    expect(await screen.findByRole('option', { name: 'demo' })).toBeInTheDocument()
    expect(screen.queryByRole('option', { name: 'gpt-5' })).not.toBeInTheDocument()
  })
})
