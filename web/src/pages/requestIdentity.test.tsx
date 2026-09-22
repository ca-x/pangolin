import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import OperationsPage, { RequestDetailPage, TraceDetailPage } from './OperationsPage'
import OverviewPage from './OverviewPage'

/**
 * `request_id` is the caller's `x-request-id`: a client can repeat it, so it cannot
 * identify a row. The list used it as the React key and as the detail link, which
 * collapsed two rows into one key and made both links open whichever row the backend
 * resolved last. The projection's own UUID is `internal_id`; the external id stays on
 * the row for correlation.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const bootstrap = { initialized: true, authenticated: true, user: { id: 'u1', email: 'owner@example.test', role: 'admin', language: 'en', theme: 'system:bronze', created_at: 1 }, capture_payloads: false, observability_available: true, branding: { instance_name: 'Pangolin', branding_name: 'Pangolin', favicon_url: '/logo.webp', onboarding_complete: true } }
const summary = { requests: 2, errors: 0, error_rate: 0, p95_latency_ms: 12, input_tokens: 2, output_tokens: 2, cost_micros: 0, series: [] }
const row = (overrides: Record<string, unknown> = {}) => ({ internal_id: 'internal-a', request_id: 'r1', started_at: 1_700_000_000, endpoint: '/v1/chat/completions', provider: 'openai', requested_model: 'demo', resolved_model: 'demo', status_code: 200, error_kind: null, latency_ms: 12, input_tokens: 1, output_tokens: 1, cost_micros: 0, api_key_id: null, ...overrides })
/** The projection's detail document: the same row plus the record-system fields, keyed by `id`. */
const detail = (overrides: Record<string, unknown> = {}) => ({ ...row({ internal_id: undefined, id: 'internal-a', request_id: 'same-id' }), finished_at: 1_700_000_002, trace_id: 'trace-1', ttft_ms: null, cached_tokens: 0, payload_captured: false, request_json: null, response_json: null, ...overrides })

const mockApi = (options: { rows?: unknown[]; total?: number; detail?: unknown; trace?: unknown } = {}) => vi.fn((input: RequestInfo | URL) => {
  const path = String(input)
  if (path.includes('/api/v1/bootstrap')) return json(bootstrap)
  if (path.includes('settings/request-logging')) return json({ enabled: true, default_level: 'metadata', key_override_enabled: false, key_disable_allowed: false })
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('/observability/summary')) return json(summary)
  if (path.includes('/operations/trace-detail/')) return json(options.trace ?? traceBundle([]))
  if (path.includes('/observability/requests/')) return json(options.detail ?? detail())
  if (path.includes('/observability/requests')) return json({ data: options.rows ?? [], total: options.total ?? (options.rows ?? []).length, offset: 0, limit: 25 })
  if (path.includes('/analytics')) return json({ data: [], source: 'sqlite', derived_available: true })
  return json({ data: [], total: 0 })
})

/**
 * The trace bundle carries both identities per request: `id` is Pangolin's own UUID and
 * `public_id` is the client's `x-request-id`, which a caller can repeat.
 */
const traceBundle = (requests: unknown[]) => ({ trace: { id: 'trace-1', status: 'succeeded', started_at: 1_700_000_000, finished_at: 1_700_000_010, thread_id: null }, requests, executions: [] })

const harness = (page: React.ReactElement, entries: Parameters<typeof MemoryRouter>[0]['initialEntries']) => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={entries}><ProjectProvider><Routes><Route path="/" element={page} /><Route path="/operations" element={page} /><Route path="/operations/requests/:id" element={<RequestDetailPage />} /><Route path="/operations/traces/:id" element={<TraceDetailPage />} /></Routes></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

const detailLinks = () => screen.getAllByRole('link').map((link) => link.getAttribute('href')).filter((href): href is string => Boolean(href?.startsWith('/operations/requests/')))

describe('a page of requests is identified by the internal id', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('renders two rows that share an external id and links each to its own request', async () => {
    const fetchMock = mockApi({
      rows: [row({ internal_id: 'internal-a', request_id: 'same-id', started_at: 1_700_000_010 }), row({ internal_id: 'internal-b', request_id: 'same-id', started_at: 1_700_000_000 })],
      total: 2,
    })
    vi.stubGlobal('fetch', fetchMock)
    harness(<OperationsPage />, ['/operations'])

    expect(await screen.findByRole('columnheader', { name: 'External request ID' })).toBeInTheDocument()
    // Both rows are on the page: the duplicate external id is not a key collision.
    expect(screen.getAllByText('same-id')).toHaveLength(2)
    expect(detailLinks()).toEqual(['/operations/requests/internal-a', '/operations/requests/internal-b'])
    expect(new Set(detailLinks()).size).toBe(2)
  })

  it('carries the page of internal ids into the detail navigation', async () => {
    const fetchMock = mockApi({
      rows: [row({ internal_id: 'internal-a', request_id: 'same-id' }), row({ internal_id: 'internal-b', request_id: 'same-id' })],
      total: 2,
    })
    vi.stubGlobal('fetch', fetchMock)
    harness(<OperationsPage />, ['/operations'])
    await screen.findByRole('columnheader', { name: 'External request ID' })

    // Opening the second row hands the detail the page it came from, and stepping
    // back has to stay on internal ids: with external ids the step would ask the
    // backend for an id two rows share.
    const links = screen.getAllByRole('link').filter((link) => link.getAttribute('href')?.startsWith('/operations/requests/'))
    await userEvent.click(links[1])
    expect(await screen.findByText('2 of 2')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Previous request' }))
    expect(await screen.findByText('1 of 2')).toBeInTheDocument()
    const records = fetchMock.mock.calls.map(([input]) => String(input)).filter((path) => path.includes('/operations/requests/'))
    expect(records).toContain('/api/admin/v1/projects/p1/operations/requests/internal-a')
    expect(records).not.toContain('/api/admin/v1/projects/p1/operations/requests/same-id')
  })

  it('keeps the external id on the detail and loads the record by the internal id', async () => {
    const fetchMock = mockApi()
    vi.stubGlobal('fetch', fetchMock)
    harness(<RequestDetailPage />, ['/operations/requests/internal-a'])

    // Both identities are named: the page's own UUID and the client's correlation id.
    expect(await screen.findByText('same-id')).toBeInTheDocument()
    expect(screen.getByText('Internal request ID')).toBeInTheDocument()
    expect(screen.getAllByText('internal-a')).toHaveLength(2)
    // The record-system facts are read with the internal id the URL carries.
    expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain('/api/admin/v1/projects/p1/operations/requests/internal-a')
  })

  it('shows only the recorded client IP as localized request metadata', async () => {
    const fetchMock = mockApi({ detail: detail({
      source_ip: '203.0.113.7',
      user_agent: 'secret-user-agent',
      headers: { authorization: 'Bearer secret-token', cookie: 'secret-cookie' },
      metadata: { credential: 'secret-credential' },
    }) })
    vi.stubGlobal('fetch', fetchMock)
    harness(<RequestDetailPage />, ['/operations/requests/internal-a'])

    expect(await screen.findByText('Client IP')).toBeInTheDocument()
    expect(screen.getByText('203.0.113.7')).toBeInTheDocument()
    for (const leak of ['secret-user-agent', 'secret-token', 'secret-cookie', 'secret-credential']) {
      expect(screen.queryByText(leak)).not.toBeInTheDocument()
    }
    await i18n.changeLanguage('zh-CN')
    expect(await screen.findByText('客户端 IP')).toBeInTheDocument()
    expect(screen.getByText('203.0.113.7')).toBeInTheDocument()
  })

  it('states an unrecorded client IP as unmeasured rather than inventing one', async () => {
    vi.stubGlobal('fetch', mockApi({ detail: detail({ source_ip: null }) }))
    harness(<RequestDetailPage />, ['/operations/requests/internal-a'])

    const label = await screen.findByText('Client IP')
    expect(label.parentElement).toHaveTextContent('—')
    expect(label.parentElement).not.toHaveTextContent('0.0.0.0')
  })

  it('keys the overview recent requests by the internal id as well', async () => {
    const fetchMock = mockApi({ rows: [row({ internal_id: 'internal-a', request_id: 'same-id' }), row({ internal_id: 'internal-b', request_id: 'same-id' })] })
    vi.stubGlobal('fetch', fetchMock)
    harness(<OverviewPage />, ['/'])

    // The overview's own recent-request list is the same projection rows, so it has
    // the same identity rule.
    expect(await screen.findAllByRole('heading', { name: 'Recent requests' })).toHaveLength(1)
    expect(detailLinks()).toEqual(['/operations/requests/internal-a', '/operations/requests/internal-b'])
  })
})

/**
 * The trace bundle carries both identities for every request of the trace: `id` is
 * Pangolin's own UUID and `public_id` is the client's `x-request-id`, which a caller can
 * repeat. The timeline linked by `public_id`, so two requests of one trace that share an
 * external id both opened whichever row the backend's compatibility lookup resolved —
 * while the bundle already carried the unambiguous id.
 */
describe('a trace timeline links each request by its internal id', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('links two requests that share an external id to their own internal ids', async () => {
    const fetchMock = mockApi({ trace: traceBundle([
      { id: 'internal-a', public_id: 'same-id', status: 'succeeded', endpoint: '/v1/chat/completions', model: 'demo', started_at: 1_700_000_000, finished_at: 1_700_000_002 },
      { id: 'internal-b', public_id: 'same-id', status: 'succeeded', endpoint: '/v1/chat/completions', model: 'demo', started_at: 1_700_000_003, finished_at: 1_700_000_004 },
    ]) })
    vi.stubGlobal('fetch', fetchMock)
    harness(<TraceDetailPage />, ['/operations/traces/trace-1'])

    // The timeline is up, with one bar per request, before anything about identity is read.
    expect(await screen.findByRole('region', { name: 'Timeline' })).toBeInTheDocument()
    // Each request opens its own row: two requests, two distinct internal ids. The
    // external id is not a navigation target at all.
    expect([...new Set(detailLinks())].sort()).toEqual(['/operations/requests/internal-a', '/operations/requests/internal-b'])
    expect(detailLinks().some((href) => href.includes('same-id'))).toBe(false)
    // The external id stays on the row for correlation with the client.
    expect(within(screen.getByRole('region', { name: 'Timeline' })).getAllByText('same-id')).toHaveLength(2)
  })

  it('does not relabel the bundle’s internal-id fallback as an external id', async () => {
    // The bundle coalesces `public_id` to the internal id for a request with no
    // external id at all, so an equal pair is not an external id to display.
    const fetchMock = mockApi({ trace: traceBundle([
      { id: 'internal-a', public_id: 'internal-a', status: 'succeeded', endpoint: '/v1/chat/completions', model: 'demo', started_at: 1_700_000_000, finished_at: 1_700_000_002 },
    ]) })
    vi.stubGlobal('fetch', fetchMock)
    harness(<TraceDetailPage />, ['/operations/traces/trace-1'])

    const timeline = within(await screen.findByRole('region', { name: 'Timeline' }))
    expect(timeline.getByRole('link', { name: 'View details' })).toHaveAttribute('href', '/operations/requests/internal-a')
    expect(timeline.queryByText('internal-a')).not.toBeInTheDocument()
  })
})
