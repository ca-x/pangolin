import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { createMemoryRouter, MemoryRouter, Route, RouterProvider, Routes } from 'react-router'
import { toast } from 'sonner'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfirmHost } from '../components'
import i18n from '../i18n'
import { ProjectProvider, useProject } from '../project'
import OperationsPage, { TraceDetailPage } from './OperationsPage'
import OverviewPage from './OverviewPage'

/**
 * The trace lifecycle's own mutations are asserted through the toast the
 * console shows, so the real host would put a second copy of the same sentence
 * in the document and make every "was this reported" question ambiguous.
 */
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

/**
 * The request log, the trace detail and the overview all read payloads the
 * gateway already returns and used to drop on the floor: the failure reason on
 * every row, the window the log is asked for, the per-attempt usage and price
 * components of a trace, and the per-dimension breakdowns. These tests pin the
 * observable behaviour of that wiring, including the rule that an unmeasured
 * value reads `—` and never an invented zero.
 */
/**
 * The request log's column order. A cell assertion names the column it means, so
 * it cannot silently pass while checking a neighbouring column.
 */
const COLUMN = { status: 0, reason: 1, externalId: 2, model: 3, provider: 4, endpoint: 5, tokens: 6, ttft: 7, stream: 8, latency: 9, cost: 10, startedAt: 11 } as const
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const bootstrap = { initialized: true, authenticated: true, user: { id: 'u1', email: 'owner@example.test', role: 'admin', language: 'en', theme: 'system:bronze', created_at: 1 }, capture_payloads: false, branding: { instance_name: 'Pangolin', branding_name: 'Pangolin', favicon_url: '/logo.webp', onboarding_complete: true } }
const policy = { enabled: true, default_level: 'metadata', key_override_enabled: false, key_disable_allowed: false }
const summary = (overrides: Record<string, unknown> = {}) => ({ requests: 3, errors: 1, error_rate: 0.33, p95_latency_ms: 20, input_tokens: 10, output_tokens: 5, cost_micros: 1234, series: [{ bucket: 1_700_000_000, requests: 3, errors: 1, latency_ms: 12 }], ...overrides })
const row = (overrides: Record<string, unknown> = {}) => ({ internal_id: `internal-${overrides.request_id ?? 'r1'}`, request_id: 'r1', started_at: 1_700_000_000, endpoint: '/v1/chat/completions', provider: 'openai', requested_model: 'demo', resolved_model: 'demo', status_code: 200, error_kind: null, latency_ms: 12, input_tokens: 1, output_tokens: 1, cost_micros: 0, ...overrides })
const dimensionRow = (overrides: Record<string, unknown> = {}) => ({ dimension: 'prov-1', requests: 5, attempts: 6, errors: 1, input_tokens: 10, output_tokens: 4, cache_hit_tokens: 2, cache_savings_micros: 0, cost_micros: 1234, latency_ms: 12.5, ttft_ms: null, ...overrides })
const bundle = (overrides: Record<string, unknown> = {}) => ({
  trace: { id: 't1', status: 'failed', started_at: 1_700_000_000, finished_at: 1_700_000_009, thread_id: null },
  requests: [{ id: 'r1', public_id: 'pub-1', status: 'failed', endpoint: '/v1/chat/completions', model: 'demo', started_at: 1_700_000_000, finished_at: 1_700_000_009 }],
  executions: [
    { id: 'e1', request_id: 'r1', provider_id: 'openai', provider_name: 'Channel A', attempt: 1, model: 'gpt-4o', status: 'failed', retry_reason: 'circuit_open', latency_ms: 120, started_at: 1_700_000_000, finished_at: 1_700_000_001 },
    { id: 'e2', request_id: 'r1', provider_id: 'anthropic', provider_name: 'Channel B', attempt: 2, model: 'claude', status: 'succeeded', retry_reason: null, latency_ms: 80, started_at: 1_700_000_001, finished_at: 1_700_000_002 },
    { id: 'e3', request_id: 'r1', provider_id: 'google', provider_name: 'Channel C', attempt: 3, model: 'gemini', status: 'succeeded', retry_reason: null, latency_ms: 40, started_at: 1_700_000_002, finished_at: 1_700_000_003 },
  ],
  usage: [
    { execution_id: 'e1', model_id: 'm1', input_tokens: 10, output_tokens: 4, cache_read_tokens: 2, cache_write_tokens: 1, reasoning_tokens: 3, total_cost_micros: 1234, created_at: 1_700_000_001 },
    { execution_id: 'e2', model_id: 'm2', input_tokens: 5, output_tokens: 1, cache_read_tokens: 0, cache_write_tokens: 0, reasoning_tokens: 0, total_cost_micros: 100, created_at: 1_700_000_002 },
  ],
  cost_items: [
    { execution_id: 'e1', kind: 'input', quantity: 10, unit_price_micros: 100, subtotal_micros: 1000 },
    { execution_id: 'e1', kind: 'output', quantity: 4, unit_price_micros: 58, subtotal_micros: 232 },
    { execution_id: 'e2', kind: null, quantity: 1, unit_price_micros: 2, subtotal_micros: 2 },
  ],
  ...overrides,
})

type MockOptions = { measured?: boolean; rows?: unknown[]; total?: number; windowedTotal?: number; analytics?: unknown; analyticsFails?: boolean; channels?: unknown; channelsFail?: boolean; trace?: unknown; traces?: unknown; summary?: Record<string, unknown>; summaryFails?: boolean }
const mockApi = (options: MockOptions = {}) => vi.fn((input: RequestInfo | URL) => {
  const path = String(input)
  if (path.includes('/api/v1/bootstrap')) return json({ ...bootstrap, observability_available: options.measured !== false })
  if (path.includes('settings/request-logging')) return json(policy)
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('/analytics')) return options.analyticsFails ? Promise.reject(new Error('analytics unavailable')) : json(options.analytics ?? { data: [dimensionRow()], source: 'sqlite', derived_available: true })
  if (path.includes('/observability/summary')) return options.summaryFails ? Promise.reject(new Error('summary unavailable')) : json(summary(options.summary))
  if (path.includes('/observability/requests/')) return json(row({ id: 'internal-r2', request_id: 'r2' }))
  // The log and its total come from one response, and the windowed request is
  // the one that carries a lower bound.
  if (path.includes('/observability/requests')) return json({ data: options.rows ?? [], total: path.includes('from=') ? options.windowedTotal ?? options.total ?? 0 : options.total ?? 0, offset: 0, limit: 25 })
  if (path.includes('/operations/trace-detail/')) return json(options.trace ?? bundle())
  if (path.includes('/operations/traces')) return json({ data: options.traces ?? [], total: (options.traces as unknown[] | undefined)?.length ?? 0 })
  if (path.includes('/operations/channels')) return options.channelsFail ? Promise.reject(new Error('channels unavailable')) : json({ data: options.channels ?? [{ id: 'prov-1', name: 'OpenAI main' }, { id: 'prov-2', name: 'Backup provider' }], total: 2 })
  if (path.includes('/operations/models')) return json({ data: [{ id: 'mod-1', public_name: 'gpt-4o' }], total: 1 })
  return json({ data: [], total: 0 })
})

const renderPage = (page: React.ReactElement, path = '/operations') => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[path]}><ProjectProvider>{page}</ProjectProvider></MemoryRouter></QueryClientProvider>)
}
const renderRoute = (page: React.ReactElement, path: string, pattern: string) => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[path]}><ProjectProvider><Routes><Route path={pattern} element={page} /></Routes></ProjectProvider></MemoryRouter></QueryClientProvider>)
}
const renderOperationsRouter = (path: string) => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const router = createMemoryRouter([{
    path: '/operations',
    element: <ProjectProvider><OperationsPage /></ProjectProvider>,
  }], { initialEntries: [path] })
  render(<QueryClientProvider client={client}><RouterProvider router={router} /></QueryClientProvider>)
  return router
}
const urls = (fetchMock: ReturnType<typeof vi.fn>, match: string) => fetchMock.mock.calls.map(([input]) => String(input)).filter((path) => path.includes(match))
const paramsOf = (url: string) => new URL(url, 'http://pangolin.test').searchParams

describe('the request log shows why a request failed', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('filters the log by the key that made the request', async () => {
    // `total` has to be non-zero: the filter bar is hidden on an empty log, which
    // is why an earlier version of this test could not find the control.
    const fetchMock = mockApi({ rows: [row({ request_id: 'r-key', api_key_id: 'key-abc' })], total: 1 })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<OperationsPage />)

    const facet = await screen.findByRole('combobox', { name: 'Key ID' })
    await userEvent.click(facet)
    await userEvent.click(await screen.findByRole('option', { name: 'key-abc' }))

    await waitFor(() => {
      const asked = fetchMock.mock.calls.map(([path]) => String(path)).filter((path) => path.includes('/observability/requests?'))
      expect(asked.some((path) => path.includes('api_key_id=key-abc'))).toBe(true)
    })
  })
  it('names the reason on the row and keeps — when the row carries none', async () => {
    vi.stubGlobal('fetch', mockApi({ rows: [row({ request_id: 'r-ok', requested_model: 'demo', resolved_model: 'demo' }), row({ request_id: 'r-bad', requested_model: 'broken', resolved_model: 'broken', status_code: 502, error_kind: 'usage_unavailable' })] }))
    renderPage(<OperationsPage />)

    expect(await screen.findByRole('columnheader', { name: 'Failure reason' })).toBeInTheDocument()
    expect(within(screen.getByRole('row', { name: /broken/ })).getByText('Upstream returned no usage')).toBeInTheDocument()
    // A request that did not fail has no reason, which is unmeasured, not empty.
    // The reason is the second cell; the row's other unmeasured facts read `—`
    // too, so the cell is what proves the reason itself is unmeasured.
    expect(within(screen.getByRole('row', { name: /demo/ })).getAllByRole('cell')[1]).toHaveTextContent('—')
  })

  it('shows a kind it cannot translate verbatim rather than hiding it', async () => {
    vi.stubGlobal('fetch', mockApi({ rows: [row({ status_code: 502, error_kind: 'provider_specific_thing' })] }))
    renderPage(<OperationsPage />)
    expect(await screen.findByText('provider_specific_thing')).toBeInTheDocument()
  })
})

describe('the request log keeps shareable multi-value filters in navigation history', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('restores every facet from the URL and sends bounded multi-value filters to the API', async () => {
    const fetchMock = mockApi({
      rows: [
        row({ request_id: 'r-openai', status_code: 200, provider: 'openai,primary', requested_model: 'fast', resolved_model: 'fast', api_key_id: 'key-a' }),
        row({ request_id: 'r-anthropic', status_code: 429, provider: 'anthropic', requested_model: 'careful', resolved_model: 'careful', api_key_id: 'key-b' }),
      ],
      total: 2,
    })
    vi.stubGlobal('fetch', fetchMock)
    renderOperationsRouter('/operations?status=200&status=429&provider=openai%2Cprimary&provider=anthropic&model=fast&model=careful&key=key-a&key=key-b')

    await waitFor(() => {
      const params = paramsOf(urls(fetchMock, '/observability/requests?').at(-1)!)
      expect(JSON.parse(params.get('status_codes')!)).toEqual(['200', '429'])
      expect(JSON.parse(params.get('providers')!)).toEqual(['openai,primary', 'anthropic'])
      expect(JSON.parse(params.get('models')!)).toEqual(['fast', 'careful'])
      expect(JSON.parse(params.get('api_key_ids')!)).toEqual(['key-a', 'key-b'])
    })

    expect((await screen.findAllByText('200')).length).toBeGreaterThan(0)
    expect(screen.getAllByText('429').length).toBeGreaterThan(0)
    expect(screen.getAllByText('openai,primary').length).toBeGreaterThan(0)
    expect(screen.getAllByText('anthropic').length).toBeGreaterThan(0)
  })

  it('pushes facet changes and restores the prior list on back and forward', async () => {
    const fetchMock = mockApi({
      rows: [
        row({ request_id: 'r-fast', requested_model: 'fast', resolved_model: 'fast' }),
        row({ request_id: 'r-careful', requested_model: 'careful', resolved_model: 'careful' }),
      ],
      total: 2,
    })
    vi.stubGlobal('fetch', fetchMock)
    const router = renderOperationsRouter('/operations')
    const model = await screen.findByRole('combobox', { name: 'Model' })

    await userEvent.click(model)
    await userEvent.click(await screen.findByRole('option', { name: 'fast' }))
    await userEvent.click(model)
    await userEvent.click(await screen.findByRole('option', { name: 'careful' }))
    await waitFor(() => expect(JSON.parse(paramsOf(urls(fetchMock, '/observability/requests?').at(-1)!).get('models')!)).toEqual(['fast', 'careful']))

    await router.navigate(-1)
    await waitFor(() => expect(paramsOf(urls(fetchMock, '/observability/requests?').at(-1)!).get('model')).toBe('fast'))
    expect(paramsOf(urls(fetchMock, '/observability/requests?').at(-1)!).has('models')).toBe(false)

    await router.navigate(1)
    await waitFor(() => expect(JSON.parse(paramsOf(urls(fetchMock, '/observability/requests?').at(-1)!).get('models')!)).toEqual(['fast', 'careful']))
  })
})

describe('the request log honours a time window', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('sends the chosen window and reports the count that window produced', async () => {
    const fetchMock = mockApi({ rows: [row()], total: 3, windowedTotal: 40 })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<OperationsPage />)
    // Unwindowed, the three recorded requests fit on one page and the count is
    // not shown at all.
    await screen.findByRole('columnheader', { name: 'Failure reason' })
    expect(screen.queryByText(/\/ 3$/)).not.toBeInTheDocument()

    await userEvent.click(await screen.findByRole('combobox', { name: 'Time window' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Last 24 hours' }))

    await waitFor(() => expect(urls(fetchMock, '/observability/requests?').some((url) => url.includes('from='))).toBe(true))
    const latest = urls(fetchMock, '/observability/requests?').at(-1)!
    const params = paramsOf(latest)
    expect(Number(params.get('until')) - Number(params.get('from'))).toBe(86400)
    // The row count and the page total are read from the same windowed response.
    expect(await screen.findByText(/1.25 \/ 40/)).toBeInTheDocument()
  })

  it('sends the exact range the operator typed', async () => {
    const fetchMock = mockApi({ rows: [row()], total: 1 })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<OperationsPage />)
    await userEvent.click(await screen.findByRole('combobox', { name: 'Time window' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Custom range' }))

    fireEvent.change(await screen.findByLabelText('From'), { target: { value: '2024-01-01T00:00' } })
    fireEvent.change(screen.getByLabelText('Until'), { target: { value: '2024-01-02T06:30' } })

    await waitFor(() => {
      const params = paramsOf(urls(fetchMock, '/observability/requests?').at(-1)!)
      expect(params.get('from')).toBe(String(Math.floor(new Date('2024-01-01T00:00').getTime() / 1000)))
      expect(params.get('until')).toBe(String(Math.floor(new Date('2024-01-02T06:30').getTime() / 1000)))
    })
  })
})

describe('the request log shows the token facts, TTFT and whether it streamed', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('renders requested → resolved, the five token facts, TTFT and a streamed request', async () => {
    vi.stubGlobal('fetch', mockApi({
      rows: [row({ request_id: 'r-populated', requested_model: 'fast', resolved_model: 'gpt-4o', input_tokens: 100, output_tokens: 20, cached_tokens: 30, cache_write_tokens: 5, reasoning_tokens: 7, ttft_ms: 250, stream: true })],
      total: 1,
    }))
    renderPage(<OperationsPage />)

    const populated = await screen.findByRole('row', { name: /r-populated/ })
    // The requested model is what the client asked for; the resolved one is what
    // the route chose. Both are facts and neither replaces the other.
    expect(within(populated).getByText('fast')).toBeInTheDocument()
    expect(within(populated).getByText('gpt-4o')).toBeInTheDocument()
    for (const value of ['100', '20', '30', '5', '7']) {
      expect(within(populated).getByText(new RegExp(`(^|[^0-9])${value}([^0-9]|$)`))).toBeInTheDocument()
    }
    expect(within(populated).getByText('250 ms')).toBeInTheDocument()
    expect(within(populated).getByText('Yes')).toBeInTheDocument()
  })

  it('keeps a legacy row unmeasured instead of inventing zeros or a false', async () => {
    vi.stubGlobal('fetch', mockApi({
      rows: [row({ request_id: 'r-legacy', requested_model: 'legacy', resolved_model: null, error_kind: 'usage_unavailable', input_tokens: 0, output_tokens: 0, cost_micros: 0, cached_tokens: null, cache_write_tokens: null, reasoning_tokens: null, ttft_ms: null, stream: null })],
      total: 1,
    }))
    renderPage(<OperationsPage />)

    const legacy = await screen.findByRole('row', { name: /r-legacy/ })
    const cells = within(legacy).getAllByRole('cell')
    // The token cell, the TTFT cell and the stream cell are all unmeasured. A
    // zero in the token cell would be the gateway's normalized rejection read as
    // a measurement, and `No` would claim a stream decision nobody recorded.
    expect(cells[COLUMN.tokens]).toHaveTextContent('—')
    expect(cells[COLUMN.tokens]).not.toHaveTextContent('0')
    expect(cells[COLUMN.ttft]).toHaveTextContent('—')
    expect(cells[COLUMN.stream]).toHaveTextContent('—')
    expect(within(legacy).queryByText('No')).not.toBeInTheDocument()
    // The requested model stays a fact even when the route resolved nothing, and
    // each side of the pair keeps its own `—`.
    const model = within(cells[COLUMN.model])
    expect(model.getByText('legacy')).toBeInTheDocument()
    expect(model.getByText('—')).toBeInTheDocument()
  })

  it('keeps a measured zero as zero and a non-streamed request as No', async () => {
    vi.stubGlobal('fetch', mockApi({
      rows: [row({ request_id: 'r-zero', input_tokens: 0, output_tokens: 0, cached_tokens: 0, cache_write_tokens: 0, reasoning_tokens: 0, ttft_ms: null, stream: false })],
      total: 1,
    }))
    renderPage(<OperationsPage />)

    const zero = await screen.findByRole('row', { name: /r-zero/ })
    const cells = within(zero).getAllByRole('cell')
    // A measured zero is a real zero: the cell prints it, and the second line
    // names each of the three facts that only some providers report.
    expect(cells[COLUMN.tokens]).toHaveTextContent('0 / 0')
    expect(cells[COLUMN.tokens]).toHaveTextContent('cache read 0 · cache write 0 · reasoning 0')
    expect(within(zero).getByText('No')).toBeInTheDocument()
    // A non-stream response may legitimately have no TTFT.
    expect(cells[COLUMN.ttft]).toHaveTextContent('—')
  })
})

describe('the trace detail shows every attempt with its tokens and cost', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('lists each attempt, its usage and the price components behind the charge', async () => {
    vi.stubGlobal('fetch', mockApi())
    renderRoute(<TraceDetailPage />, '/operations/traces/t1', '/operations/traces/:id')

    const attempts = within(await screen.findByRole('region', { name: 'Attempts' }))
    // The row is named by the channel that ran it — the name frozen on the
    // execution, not the provider id the projection also carries.
    const first = attempts.getByRole('row', { name: /Channel A/ })
    expect(within(first).getByText('#1')).toBeInTheDocument()
    expect(within(first).getByText('gpt-4o')).toBeInTheDocument()
    // The retry decision is localized, not the raw enum.
    expect(within(first).getByText('Circuit breaker is open')).toBeInTheDocument()
    expect(within(first).getByText('10')).toBeInTheDocument()
    expect(within(first).getByText('$0.001234')).toBeInTheDocument()
    expect(within(first).queryByText('openai')).not.toBeInTheDocument()
    expect(within(attempts.getByRole('row', { name: /Channel B/ })).getByText('No retry')).toBeInTheDocument()
    // An attempt the projection recorded no usage for is unmeasured, never zero.
    const unmeasured = attempts.getByRole('row', { name: /Channel C/ })
    expect(within(unmeasured).getAllByText('—').length).toBeGreaterThanOrEqual(6)

    // Totals are the sum of the recorded usage rows, in micro-USD.
    const totals = within(screen.getByRole('region', { name: 'Trace usage' }))
    expect(totals.getByText('15')).toBeInTheDocument()
    expect(totals.getByText('5')).toBeInTheDocument()
    expect(totals.getByText('$0.001334')).toBeInTheDocument()

    const components = within(screen.getByRole('region', { name: 'Cost components' }))
    const input = components.getByRole('row', { name: /Input/ })
    expect(within(input).getByText('10')).toBeInTheDocument()
    expect(within(input).getByText('$0.000100')).toBeInTheDocument()
    expect(within(input).getByText('$0.001000')).toBeInTheDocument()
    expect(within(components.getByRole('row', { name: /Output/ })).getByText('$0.000232')).toBeInTheDocument()
    // A charge whose price component was deleted keeps its amount and loses its kind.
    expect(within(components.getByRole('row', { name: /#2/ })).getByText('—')).toBeInTheDocument()
  })

  it('states that usage is unmeasured when the trace carries no usage rows at all', async () => {
    vi.stubGlobal('fetch', mockApi({ trace: bundle({ usage: [], cost_items: [] }) }))
    renderRoute(<TraceDetailPage />, '/operations/traces/t1', '/operations/traces/:id')
    const totals = within(await screen.findByRole('region', { name: 'Trace usage' }))
    expect(totals.getAllByText('—').length).toBeGreaterThanOrEqual(5)
    expect(screen.getByText('No cost components were recorded for this trace.')).toBeInTheDocument()
  })

  it('falls back to the provider id only for an attempt that never recorded a name', async () => {
    // `provider_name` is frozen on the execution when it is created, so a row from
    // before that snapshot has none. The id is the only honest thing left to show —
    // and the name is still never live-joined from the current channel.
    vi.stubGlobal('fetch', mockApi({ trace: bundle({
      executions: [{ id: 'e9', request_id: 'r1', provider_id: 'legacy-provider', provider_name: null, attempt: 1, model: 'demo', status: 'failed', retry_reason: null, latency_ms: 12, started_at: 1_700_000_000, finished_at: 1_700_000_001 }],
      usage: [], cost_items: [],
    }) }))
    renderRoute(<TraceDetailPage />, '/operations/traces/t1', '/operations/traces/:id')

    const attempts = within(await screen.findByRole('region', { name: 'Attempts' }))
    expect(within(attempts.getByRole('row', { name: /legacy-provider/ })).getByText('legacy-provider')).toBeInTheDocument()
  })
})

/**
 * The trace list is what an operator triages from. It carried the internal UUID, so
 * a row could not be matched against the trace id the client logs, said nothing
 * about how many requests the trace held and never showed the question that
 * started it — all three of them facts the record system already had.
 */
describe('the trace list names the client trace id, its requests and the first user query', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  /** The trace list's column order, so a cell assertion names the column it means. */
  const TRACE_COLUMN = { status: 0, lifecycle: 1, view: 2, externalId: 3, requestCount: 4, firstUserQuery: 5 } as const
  const traceRow = (overrides: Record<string, unknown> = {}) => ({ id: 'internal-trace-1', external_id: 'trace-external-1', status: 'succeeded', lifecycle: 'active', request_count: 3, first_user_query: 'summarize the invoice', thread_id: null, started_at: 1_700_000_000, finished_at: 1_700_000_009, ...overrides })

  const openTraces = async () => {
    await userEvent.click(await screen.findByRole('tab', { name: 'Traces' }))
    return within(await screen.findByRole('region', { name: 'Traces' }))
  }

  it('shows the client trace id, the request count and the first user query', async () => {
    vi.stubGlobal('fetch', mockApi({ traces: [traceRow()] }))
    renderPage(<OperationsPage />)
    const list = await openTraces()

    expect(list.getByRole('columnheader', { name: 'External trace ID' })).toBeInTheDocument()
    expect(list.getByRole('columnheader', { name: 'Request count' })).toBeInTheDocument()
    expect(list.getByRole('columnheader', { name: 'First user query' })).toBeInTheDocument()

    const row = list.getByRole('row', { name: /trace-external-1/ })
    const cells = within(row).getAllByRole('cell')
    // An identifier stays an identifier; the preview is prose. The outcome is the
    // console's own word for the stored value, not the value itself.
    expect(cells[TRACE_COLUMN.status]).toHaveTextContent('Succeeded')
    expect(cells[TRACE_COLUMN.externalId].firstElementChild).toHaveClass('mono-cell')
    expect(cells[TRACE_COLUMN.requestCount]).toHaveTextContent('3')
    expect(cells[TRACE_COLUMN.firstUserQuery].firstElementChild).not.toHaveClass('mono-cell')
    const preview = within(cells[TRACE_COLUMN.firstUserQuery]).getByText('summarize the invoice')
    // The full stored value is available even where the cell is read out of context.
    expect(preview).toHaveAttribute('title', 'summarize the invoice')
    // The row still opens the trace by its internal id.
    expect(within(row).getByRole('link')).toHaveAttribute('href', '/operations/traces/internal-trace-1')
  })

  it('reads — for a client id that was never sent and a payload that was never captured', async () => {
    vi.stubGlobal('fetch', mockApi({ traces: [traceRow({ id: 'internal-trace-2', external_id: null, status: 'failed', request_count: 1, first_user_query: null, thread_id: 'thread-1' })] }))
    renderPage(<OperationsPage />)
    const list = await openTraces()

    // Nothing identifies this row by text but its own facts: the internal UUID is
    // not an external id, so it is not rendered as one anywhere in the table.
    const row = list.getAllByRole('row')[1]
    const cells = within(row).getAllByRole('cell')
    expect(cells[TRACE_COLUMN.externalId]).toHaveTextContent('—')
    expect(within(row).queryByText('internal-trace-2')).not.toBeInTheDocument()
    expect(cells[TRACE_COLUMN.requestCount]).toHaveTextContent('1')
    // And no body was stored, so there is no question to preview.
    expect(cells[TRACE_COLUMN.firstUserQuery]).toHaveTextContent('—')
  })
})

/**
 * The overview's own window drives both of its reads. Every selection resolves one
 * `{from, until}` pair that the summary and the breakdown share, and every surface
 * that names a window names the selected one — including the retry, which re-reads
 * the same bounds rather than a window that slid forward while the read was failing.
 */
describe('the overview reads one window everywhere', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  const choose = async (name: string) => {
    await userEvent.click(await screen.findByRole('combobox', { name: 'Time window' }))
    await userEvent.click(await screen.findByRole('option', { name }))
  }
  // The description is the header's own paragraph: the select renders its options
  // inside the same header element, so a text query there is ambiguous.
  const headerDescription = () => document.querySelector('.page-header p')?.textContent
  const lastSummary = (fetchMock: ReturnType<typeof vi.fn>) => paramsOf(urls(fetchMock, '/observability/summary?').at(-1)!)

  it('sends the selected window to the summary and the same bounds to the breakdown', async () => {
    const fetchMock = mockApi()
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<OverviewPage />, '/')
    await screen.findByRole('region', { name: 'Breakdowns' })

    // The default window is still the last 24 hours, and it is asked for by its
    // own bounds rather than left to the endpoint's default.
    const initial = lastSummary(fetchMock)
    expect(Number(initial.get('until')) - Number(initial.get('from'))).toBe(86400)

    await choose('Last 7 days')

    await waitFor(() => expect(Number(lastSummary(fetchMock).get('until')) - Number(lastSummary(fetchMock).get('from'))).toBe(604800))
    const selected = lastSummary(fetchMock)
    await waitFor(() => expect(urls(fetchMock, '/analytics?').at(-1)).toContain(`from=${selected.get('from')}`))
    const analytics = paramsOf(urls(fetchMock, '/analytics?').at(-1)!)
    expect(analytics.get('from')).toBe(selected.get('from'))
    expect(analytics.get('until')).toBe(selected.get('until'))
  })

  it('reads all retained history from the beginning of the retained window', async () => {
    const fetchMock = mockApi()
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<OverviewPage />, '/')
    await screen.findByRole('region', { name: 'Breakdowns' })

    await choose('All retained history')

    // The projection's own retention is the ultimate bound, so the pair starts at
    // the beginning of time and the aggregate is still bounded by what it holds.
    await waitFor(() => expect(lastSummary(fetchMock).get('from')).toBe('0'))
    expect(lastSummary(fetchMock).get('until')).not.toBe('0')
    await waitFor(() => expect(paramsOf(urls(fetchMock, '/analytics?').at(-1)!).get('from')).toBe('0'))
  })

  it('names the selected window in the header, the stat group and the breakdown hint', async () => {
    const fetchMock = mockApi()
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<OverviewPage />, '/')
    await screen.findByRole('region', { name: 'Breakdowns' })

    expect(headerDescription()).toBe('Last 24 hours')
    expect(screen.getByRole('group', { name: 'Last 24 hours' })).toBeInTheDocument()
    expect(screen.getByText('The same window as the cards above (Last 24 hours), grouped by the dimension you pick.')).toBeInTheDocument()

    await choose('Last 30 days')

    await waitFor(() => expect(headerDescription()).toBe('Last 30 days'))
    expect(await screen.findByRole('group', { name: 'Last 30 days' })).toBeInTheDocument()
    expect(screen.queryByRole('group', { name: 'Last 24 hours' })).not.toBeInTheDocument()
    expect(await screen.findByText('The same window as the cards above (Last 30 days), grouped by the dimension you pick.')).toBeInTheDocument()
  })

  it('names the window that recorded nothing instead of always the last day', async () => {
    vi.stubGlobal('fetch', mockApi({ summary: { requests: 0, errors: 0, error_rate: 0, p95_latency_ms: null, input_tokens: null, output_tokens: null, cost_micros: null, series: [] } }))
    renderPage(<OverviewPage />, '/')

    await choose('Last 30 days')
    expect(await screen.findByText('Last 30 days: no requests recorded')).toBeInTheDocument()
    // A window that measured no usage has no cost, so the card says so.
    expect(document.querySelector('[data-stat="cost"]')?.textContent).toBe('—')
  })

  it('re-reads the same bounds when the summary fails, and the retry keeps them', async () => {
    const fetchMock = mockApi({ summaryFails: true })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<OverviewPage />, '/')
    expect(await screen.findByRole('alert')).toHaveTextContent('The request failed. Try again shortly.')
    const failed = urls(fetchMock, '/observability/summary?').at(-1)!

    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))

    await waitFor(() => expect(urls(fetchMock, '/observability/summary?').length).toBeGreaterThan(1))
    expect(urls(fetchMock, '/observability/summary?').at(-1)).toBe(failed)
  })
})

describe('the overview breaks the window down by dimension', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('renders the breakdown with resolved names and — for an unmeasured average', async () => {
    const fetchMock = mockApi({ analytics: { data: [dimensionRow(), dimensionRow({ dimension: 'prov-2', requests: 0, errors: 0, latency_ms: null, cost_micros: 0 })] } })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<OverviewPage />, '/')

    const table = within(await screen.findByRole('region', { name: 'Breakdowns' }))
    expect(within(table.getByRole('row', { name: /OpenAI main/ })).getByText('5')).toBeInTheDocument()
    expect(within(table.getByRole('row', { name: /OpenAI main/ })).getByText('$0.001234')).toBeInTheDocument()
    // A dimension whose average latency the projection could not compute is
    // unmeasured; a measured zero request count is a real zero.
    const second = table.getByRole('row', { name: /Backup provider/ })
    expect(within(second).getByText('—')).toBeInTheDocument()
    // A measured zero is a real zero, so this row prints both of its zeros.
    expect(within(second).getAllByText('0')).toHaveLength(2)
    expect(urls(fetchMock, '/analytics?')[0]).toContain('dimension=provider')
  })

  it('re-reads the breakdown when the operator picks another dimension', async () => {
    const fetchMock = mockApi()
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<OverviewPage />, '/')
    await screen.findByRole('region', { name: 'Breakdowns' })

    await userEvent.click(screen.getByRole('combobox', { name: 'Dimension' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Model' }))

    await waitFor(() => expect(urls(fetchMock, '/analytics?').some((url) => url.includes('dimension=model'))).toBe(true))
  })

  it('offers a retry when the breakdown fails, and an empty state when nothing was recorded', async () => {
    const failing = mockApi({ analyticsFails: true })
    vi.stubGlobal('fetch', failing)
    const { unmount } = renderPage(<OverviewPage />, '/')
    expect(await screen.findByRole('alert')).toHaveTextContent('The request failed. Try again shortly.')
    const before = urls(failing, '/analytics?').length
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await waitFor(() => expect(urls(failing, '/analytics?').length).toBeGreaterThan(before))
    unmount()

    vi.stubGlobal('fetch', mockApi({ analytics: { data: [], source: 'sqlite', derived_available: true } }))
    renderPage(<OverviewPage />, '/')
    expect(await screen.findByText('No requests were recorded in this window.')).toBeInTheDocument()
  })

  it('reads nothing while the projection is down, rather than a table of zeros', async () => {
    const fetchMock = mockApi({ measured: false })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<OverviewPage />, '/')
    expect(await screen.findByRole('alert')).toHaveTextContent('Analytics projection unavailable')
    expect(urls(fetchMock, '/analytics?')).toHaveLength(0)
    expect(screen.queryByRole('region', { name: 'Breakdowns' })).not.toBeInTheDocument()
  })

  it('falls back to the opaque dimension id when the names cannot be read, and retries', async () => {
    const fetchMock = mockApi({ channelsFail: true })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<OverviewPage />, '/')
    const table = within(await screen.findByRole('region', { name: 'Breakdowns' }))
    expect(table.getByText('prov-1')).toBeInTheDocument()
    const before = urls(fetchMock, '/operations/channels').length
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await waitFor(() => expect(urls(fetchMock, '/operations/channels').length).toBeGreaterThan(before))
  })
})

/**
 * The timeline labels each bar with `finished_at - started_at` in seconds, but
 * those timestamps only have second resolution: an attempt that took 120 ms spans
 * no whole second and was printed as `0 ms`, and this fixture's one-second spans
 * printed `1000 ms` next to the same page's recorded `120 ms`. Both are numbers
 * the record never measured. The recorded latency is right there on the row.
 */
describe('the trace timeline labels an attempt with the latency that was measured', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('prints the recorded latency instead of a span the timestamps cannot resolve', async () => {
    vi.stubGlobal('fetch', mockApi())
    renderRoute(<TraceDetailPage />, '/operations/traces/t1', '/operations/traces/:id')

    const timeline = within(await screen.findByRole('region', { name: 'Timeline' }))
    expect(timeline.getByText('120 ms')).toBeInTheDocument()
    expect(timeline.getByText('80 ms')).toBeInTheDocument()
    expect(timeline.queryByText('1000 ms')).not.toBeInTheDocument()
  })

  it('links a request by its internal id and keeps the external id as a fact', async () => {
    vi.stubGlobal('fetch', mockApi())
    renderRoute(<TraceDetailPage />, '/operations/traces/t1', '/operations/traces/:id')

    const timeline = within(await screen.findByRole('region', { name: 'Timeline' }))
    // `pub-1` is the caller's repeatable `x-request-id`; the bundle's `id` is the
    // record's own identity, which is what the detail page opens.
    expect(timeline.getByRole('link', { name: 'View details' })).toHaveAttribute('href', '/operations/requests/r1')
    expect(timeline.getByText('pub-1')).toBeInTheDocument()
    expect(timeline.queryByRole('link', { name: 'pub-1' })).not.toBeInTheDocument()
  })
})

/**
 * The trace list's archive and pin lifecycle. Four explicit actions, each offered
 * only in the state that can take it, one project-scoped endpoint addressed by the
 * trace's internal id, a confirmation before the one action that hides a row, and
 * a refusal that is reported as a refusal — never as a success.
 */
describe('the trace list owns the archive and pin lifecycle', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.mocked(toast.success).mockClear(); vi.mocked(toast.error).mockClear() })

  /** The trace list's column order, so a cell assertion names the column it means. */
  const COLUMN = { status: 0, lifecycle: 1, view: 2, externalId: 3 } as const
  const traceRow = (overrides: Record<string, unknown> = {}) => ({ id: 'internal-trace-1', external_id: 'trace-external-1', status: 'succeeded', lifecycle: 'active', request_count: 3, first_user_query: 'summarize the invoice', thread_id: null, started_at: 1_700_000_000, finished_at: 1_700_000_009, ...overrides })

  const renderTraces = () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/operations']}><ProjectProvider><OperationsPage /><ConfirmHost /></ProjectProvider></MemoryRouter></QueryClientProvider>)
  }
  const openTraces = async () => {
    await userEvent.click(await screen.findByRole('tab', { name: 'Traces' }))
    return within(await screen.findByRole('region', { name: 'Traces' }))
  }
  const listUrls = (fetchMock: ReturnType<typeof vi.fn>) => urls(fetchMock, '/operations/traces')
  const listUrlsFor = (fetchMock: ReturnType<typeof vi.fn>, id: string) => listUrls(fetchMock).filter((url) => url.includes(`/projects/${id}/`))
  const lifecycleCalls = (fetchMock: ReturnType<typeof vi.fn>) => fetchMock.mock.calls.filter(([input]) => String(input).includes('/lifecycle'))

  it('shows the state of every row and keeps the archived ones out of the default view', async () => {
    const fetchMock = mockApi({ traces: [traceRow(), traceRow({ id: 'internal-trace-2', external_id: 'trace-external-2', status: 'failed', lifecycle: 'archived' }), traceRow({ id: 'internal-trace-3', external_id: 'trace-external-3', lifecycle: 'retained' })] })
    vi.stubGlobal('fetch', fetchMock)
    renderTraces()
    const list = await openTraces()

    expect(list.getByRole('columnheader', { name: 'Lifecycle' })).toBeInTheDocument()
    const archived = list.getByRole('row', { name: /trace-external-2/ })
    const cells = within(archived).getAllByRole('cell')
    // The lifecycle is its own column: the outcome the run produced is untouched.
    expect(cells[COLUMN.status]).toHaveTextContent('Failed')
    expect(cells[COLUMN.lifecycle]).toHaveTextContent('Archived')
    expect(within(list.getByRole('row', { name: /trace-external-3/ })).getAllByRole('cell')[COLUMN.lifecycle]).toHaveTextContent('Retained')
    expect(within(list.getByRole('row', { name: /trace-external-1/ })).getAllByRole('cell')[COLUMN.lifecycle]).toHaveTextContent('Active')
    // The View action and the row's identity survive the extra column.
    expect(within(archived).getByRole('link')).toHaveAttribute('href', '/operations/traces/internal-trace-2')

    // The default request is the non-archived view; nothing was asked for yet.
    expect(listUrls(fetchMock).some((url) => url.includes('lifecycle=archived'))).toBe(false)
  })

  it('localizes the outcome it renders, in both languages, instead of printing the stored enum', async () => {
    vi.stubGlobal('fetch', mockApi({ traces: [traceRow({ status: 'succeeded' }), traceRow({ id: 'internal-trace-2', external_id: 'trace-external-2', status: 'interrupted' })] }))
    renderTraces()
    const list = await openTraces()
    const cells = within(list.getByRole('row', { name: /trace-external-1/ })).getAllByRole('cell')

    expect(cells[COLUMN.status]).toHaveTextContent('Succeeded')
    expect(within(list.getByRole('row', { name: /trace-external-2/ })).getAllByRole('cell')[COLUMN.status]).toHaveTextContent('Interrupted')
    // The stored value is a wire fact, not console copy: rendering it verbatim put
    // an English enum in the middle of a translated page.
    expect(list.queryByText('succeeded')).not.toBeInTheDocument()
    expect(list.queryByText('interrupted')).not.toBeInTheDocument()
    // …and the mobile card's own state line is the same localized word, in its
    // header and in the field row alike, never the stored enum.
    const cards = document.querySelector('.mobile-resource-list') as HTMLElement
    expect(within(cards).getAllByText('Succeeded').length).toBeGreaterThan(0)
    expect(within(cards).queryByText('succeeded')).not.toBeInTheDocument()

    await i18n.changeLanguage('zh-CN')
    // Both surfaces — the table and the card — say it in the active language.
    expect((await screen.findAllByText(i18n.t('statusSucceeded', { lng: 'zh-CN' }))).length).toBeGreaterThan(0)
    expect(screen.queryByText('succeeded')).not.toBeInTheDocument()
    await i18n.changeLanguage('en')
  })

  it('keeps the lifecycle filter reachable when the default view is empty, so an archived trace is restorable', async () => {
    const fetchMock = mockApi({ traces: [] })
    vi.stubGlobal('fetch', fetchMock)
    renderTraces()
    await userEvent.click(await screen.findByRole('tab', { name: 'Traces' }))

    // An archived-only trace set leaves the default view empty — the table is not
    // rendered at all. The filter is the only way back, so it has to survive the
    // empty state instead of being hidden with the rows.
    expect(await screen.findByText('No traces yet.')).toBeInTheDocument()
    await userEvent.click(await screen.findByRole('combobox', { name: 'Lifecycle' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Archived' }))

    await waitFor(() => expect(listUrls(fetchMock).some((url) => url.includes('lifecycle=archived'))).toBe(true))
  })

  it('sends each control its own action instead of a neighbour it happens to sit beside', async () => {
    const fetchMock = mockApi({ traces: [traceRow(), traceRow({ id: 'internal-trace-2', external_id: 'trace-external-2', lifecycle: 'archived' }), traceRow({ id: 'internal-trace-3', external_id: 'trace-external-3', lifecycle: 'retained' })] })
    vi.stubGlobal('fetch', fetchMock)
    renderTraces()
    const list = await openTraces()
    const sent = () => lifecycleCalls(fetchMock).map(([, init]) => JSON.parse(String((init as RequestInit).body)).action as string)

    await userEvent.click(within(list.getByRole('row', { name: /trace-external-1/ })).getByRole('button', { name: 'Retain trace-external-1' }))
    await waitFor(() => expect(sent()).toEqual(['retain']))
    await userEvent.click(within(list.getByRole('row', { name: /trace-external-2/ })).getByRole('button', { name: 'Restore trace-external-2' }))
    await waitFor(() => expect(sent()).toEqual(['retain', 'unarchive']))
    await userEvent.click(within(list.getByRole('row', { name: /trace-external-3/ })).getByRole('button', { name: 'Stop retaining trace-external-3' }))
    await waitFor(() => expect(sent()).toEqual(['retain', 'unarchive', 'unretain']))
  })

  it('offers only the transitions the row can actually take', async () => {
    vi.stubGlobal('fetch', mockApi({ traces: [traceRow(), traceRow({ id: 'internal-trace-2', external_id: 'trace-external-2', lifecycle: 'archived' }), traceRow({ id: 'internal-trace-3', external_id: 'trace-external-3', lifecycle: 'retained' })] }))
    renderTraces()
    const list = await openTraces()

    const active = list.getByRole('row', { name: /trace-external-1/ })
    expect(within(active).getByRole('button', { name: 'Archive trace-external-1' })).toBeInTheDocument()
    expect(within(active).getByRole('button', { name: 'Retain trace-external-1' })).toBeInTheDocument()
    expect(within(active).queryByRole('button', { name: 'Restore trace-external-1' })).not.toBeInTheDocument()
    expect(within(active).queryByRole('button', { name: 'Stop retaining trace-external-1' })).not.toBeInTheDocument()

    const archived = list.getByRole('row', { name: /trace-external-2/ })
    expect(within(archived).getByRole('button', { name: 'Restore trace-external-2' })).toBeInTheDocument()
    expect(within(archived).queryByRole('button', { name: 'Archive trace-external-2' })).not.toBeInTheDocument()
    expect(within(archived).queryByRole('button', { name: 'Retain trace-external-2' })).not.toBeInTheDocument()

    const retained = list.getByRole('row', { name: /trace-external-3/ })
    expect(within(retained).getByRole('button', { name: 'Stop retaining trace-external-3' })).toBeInTheDocument()
    expect(within(retained).queryByRole('button', { name: 'Retain trace-external-3' })).not.toBeInTheDocument()
    expect(within(retained).queryByRole('button', { name: 'Archive trace-external-3' })).not.toBeInTheDocument()
  })

  it('reports a refused archive once, inside the dialog where the decision was made', async () => {
    const base = mockApi({ traces: [traceRow()] })
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      if (String(input).includes('/lifecycle')) return Promise.resolve(new Response(JSON.stringify({ error: { type: 'invalid_trace_transition', message: 'trace is archived; archive requires active' } }), { status: 409, headers: { 'Content-Type': 'application/json' } }))
      return base(input)
    })
    vi.stubGlobal('fetch', fetchMock)
    renderTraces()
    const list = await openTraces()

    await userEvent.click(within(list.getByRole('row', { name: /trace-external-1/ })).getByRole('button', { name: 'Archive trace-external-1' }))
    await userEvent.click(within(await screen.findByRole('dialog', { name: 'Archive this trace?' })).getByRole('button', { name: 'Archive' }))

    const dialog = await screen.findByRole('dialog', { name: 'Archive this trace?' })
    const alerts = await within(dialog).findAllByRole('alert')
    expect(alerts).toHaveLength(1)
    expect(alerts[0]).toHaveTextContent('trace is archived; archive requires active')
    // One accepted write, one outcome: the same sentence must not also arrive as a
    // toast, and a refusal must never be announced as a success.
    expect(toast.error).not.toHaveBeenCalled()
    expect(toast.success).not.toHaveBeenCalled()
  })

  it('asks before hiding a trace, and sends the confirmed action to that trace in that project', async () => {
    const fetchMock = mockApi({ traces: [traceRow()] })
    vi.stubGlobal('fetch', fetchMock)
    renderTraces()
    const list = await openTraces()
    const asked = listUrls(fetchMock).length
    const archive = () => within(list.getByRole('row', { name: /trace-external-1/ })).getByRole('button', { name: 'Archive trace-external-1' })

    await userEvent.click(archive())
    const dialog = await screen.findByRole('dialog', { name: 'Archive this trace?' })
    // The confirmation has to say what archiving costs the operator.
    expect(dialog).toHaveTextContent('default view')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(lifecycleCalls(fetchMock)).toHaveLength(0)

    await userEvent.click(archive())
    await userEvent.click(within(await screen.findByRole('dialog', { name: 'Archive this trace?' })).getByRole('button', { name: 'Archive' }))

    await waitFor(() => expect(lifecycleCalls(fetchMock)).toHaveLength(1))
    const [path, init] = lifecycleCalls(fetchMock)[0]
    expect(String(path)).toBe('/api/admin/v1/projects/p1/traces/internal-trace-1/lifecycle')
    expect((init as RequestInit).method).toBe('POST')
    expect(JSON.parse(String((init as RequestInit).body))).toEqual({ action: 'archive' })
    // The exact project's list is the one that is re-read.
    await waitFor(() => expect(listUrls(fetchMock).length).toBeGreaterThan(asked))
    expect(toast.success).toHaveBeenCalledTimes(1)
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('cannot send a direct action twice while the first one is in flight', async () => {
    let release: (() => void) | null = null
    const base = mockApi({ traces: [traceRow({ lifecycle: 'retained' })] })
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input).includes('/lifecycle')) return new Promise<Response>((resolve) => { release = () => resolve(new Response(JSON.stringify({ id: 'internal-trace-1', lifecycle: 'active' }), { status: 200, headers: { 'Content-Type': 'application/json' } })) })
      return base(input)
    })
    vi.stubGlobal('fetch', fetchMock)
    renderTraces()
    const list = await openTraces()
    const control = within(list.getByRole('row', { name: /trace-external-1/ })).getByRole('button', { name: 'Stop retaining trace-external-1' })

    await userEvent.click(control)
    await waitFor(() => expect(lifecycleCalls(fetchMock)).toHaveLength(1))
    expect(control).toBeDisabled()
    fireEvent.click(control)
    expect(lifecycleCalls(fetchMock)).toHaveLength(1)

    release!()
    await waitFor(() => expect(toast.success).toHaveBeenCalledTimes(1))
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('reports every direct action failure as one toast and never as a success', async () => {
    const base = mockApi({ traces: [traceRow(), traceRow({ id: 'internal-trace-2', external_id: 'trace-external-2', lifecycle: 'archived' }), traceRow({ id: 'internal-trace-3', external_id: 'trace-external-3', lifecycle: 'retained' })] })
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      if (String(input).includes('/lifecycle')) return Promise.resolve(new Response(JSON.stringify({ error: { type: 'invalid_trace_transition', message: 'trace is archived; retain requires active' } }), { status: 409, headers: { 'Content-Type': 'application/json' } }))
      return base(input)
    })
    vi.stubGlobal('fetch', fetchMock)
    renderTraces()
    const list = await openTraces()

    // Each direct action owns its own refusal: one toast each, and never a dialog
    // (which is the archive path's own place to report).
    const direct = [
      ['trace-external-1', 'Retain trace-external-1'],
      ['trace-external-2', 'Restore trace-external-2'],
      ['trace-external-3', 'Stop retaining trace-external-3'],
    ] as const
    for (const [index, [name, label]] of direct.entries()) {
      await userEvent.click(within(list.getByRole('row', { name: new RegExp(name) })).getByRole('button', { name: label }))
      await waitFor(() => expect(toast.error).toHaveBeenCalledTimes(index + 1))
    }

    // One accepted write, one outcome: a refusal is never also announced as a
    // success, and the message is the server's own.
    expect(toast.success).not.toHaveBeenCalled()
    expect(toast.error).toHaveBeenCalledWith('trace is archived; retain requires active')
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  /**
   * The shell's project switcher; the harness drives the same provider state the
   * switcher does, so the test can move the console off a project mid-flight.
   */
  function ProjectSwitch() {
    const { project: active, projects, setProjectId } = useProject()
    return <div>
      <span data-testid="active-project">{active.id}</span>
      {projects.map((candidate) => <button key={candidate.id} type="button" onClick={() => setProjectId(candidate.id)}>{candidate.name}</button>)}
    </div>
  }

  it('does not announce or refresh a write to a project the console has already left', async () => {
    const other = { id: 'p2', name: 'Project B', slug: 'project-b', owner_user_id: 'u1', is_default: false, enabled: true }
    let release: (() => void) | null = null
    const base = mockApi({ traces: [traceRow({ lifecycle: 'retained' })] })
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project, other])
      if (path.includes('/lifecycle')) return new Promise<Response>((resolve) => { release = () => resolve(new Response(JSON.stringify({ id: 'internal-trace-1', lifecycle: 'active' }), { status: 200, headers: { 'Content-Type': 'application/json' } })) })
      return base(input)
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/operations']}><ProjectProvider><OperationsPage /><ProjectSwitch /><ConfirmHost /></ProjectProvider></MemoryRouter></QueryClientProvider>)

    const list = await openTraces()
    const readsInP2 = () => listUrlsFor(fetchMock, 'p2').length
    await userEvent.click(within(list.getByRole('row', { name: /trace-external-1/ })).getByRole('button', { name: 'Stop retaining trace-external-1' }))
    await waitFor(() => expect(lifecycleCalls(fetchMock)).toHaveLength(1))

    // The operator leaves p1 while the write is unanswered, and p2's own list is
    // read because p2 is now what the console shows. From here on, any further read
    // of p2 would be this write's doing.
    await userEvent.click(screen.getByRole('button', { name: 'Project B' }))
    await waitFor(() => expect(screen.getByTestId('active-project')).toHaveTextContent('p2'))
    await waitFor(() => expect(readsInP2()).toBeGreaterThan(0))
    const readsBefore = readsInP2()

    release!()
    // The p1 list is the one the write belongs to, and it is the one marked stale.
    await waitFor(() => {
      const p1 = client.getQueryCache().findAll({ queryKey: ['resource', 'p1', 'traces'] })
      expect(p1.length).toBeGreaterThan(0)
      expect(p1.every((query) => query.state.isInvalidated)).toBe(true)
    })

    // The p2 screen says nothing about a p1 change, and p2's own list is untouched.
    expect(toast.success).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
    expect(client.getQueryCache().findAll({ queryKey: ['resource', 'p2', 'traces'] }).some((query) => query.state.isInvalidated)).toBe(false)
    expect(readsInP2()).toBe(readsBefore)
  })

  it('keeps the state and the actions named and reachable on a phone-width card', async () => {
    vi.stubGlobal('fetch', mockApi({ traces: [traceRow({ lifecycle: 'retained' })] }))
    renderTraces()
    await openTraces()

    const cards = document.querySelector('.mobile-resource-list') as HTMLElement
    expect(cards).not.toBeNull()
    // The first three fields are the card's own: outcome, lifecycle and the view
    // action, in that order. The outcome keeps whatever the card already showed.
    expect(within(cards).getAllByText('Succeeded').length).toBeGreaterThan(0)
    expect(within(cards).getByText('Retained')).toBeInTheDocument()
    expect(within(cards).getByRole('link')).toHaveAttribute('href', '/operations/traces/internal-trace-1')
    // An icon-only control still has a name, and it is the only valid action here.
    expect(within(cards).getAllByRole('button', { name: 'Stop retaining trace-external-1' })).toHaveLength(1)
    expect(within(cards).queryByRole('button', { name: 'Archive trace-external-1' })).not.toBeInTheDocument()
  })
})
