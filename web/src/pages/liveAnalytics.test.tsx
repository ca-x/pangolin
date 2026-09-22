import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, useLocation } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { ThemeProvider } from '../theme'
import AnalyticsPage from './AnalyticsPage'
import OperationsPage from './OperationsPage'

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'project-a', name: 'Project A', slug: 'project-a', owner_user_id: 'owner', is_default: true, enabled: true }

function LocationProbe() {
  const location = useLocation()
  return <output aria-label="location">{location.search}</output>
}

function renderPage(page: React.ReactElement, path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><ThemeProvider><MemoryRouter initialEntries={[path]}><ProjectProvider>{page}<LocationProbe /></ProjectProvider></MemoryRouter></ThemeProvider></QueryClientProvider>)
}

describe('live requests and dedicated analytics', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en')
    vi.clearAllMocks()
  })

  it('keeps analytics filters in the URL and renders nullable performance, throughput and cost facts', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/analytics?')) return json({ data: [
        { dimension: 'model-alpha', requests: 700, attempts: 701, errors: 1, sample_count: 700, confidence_level: 'high', usage_measured: true, input_tokens: 120, output_tokens: 30, cache_hit_tokens: 4, cache_savings_micros: 12, cost_micros: 3456, latency_ms: 820, ttft_ms: 95, tokens_per_second: 36.5 },
        { dimension: 'model-low', requests: 1, attempts: 1, errors: 0, sample_count: 1, confidence_level: 'low', usage_measured: true, input_tokens: 8, output_tokens: 2, cache_hit_tokens: 0, cache_savings_micros: 0, cost_micros: 25, latency_ms: 900, ttft_ms: 120, tokens_per_second: 2.2 },
        { dimension: 'model-unmeasured', requests: 1, attempts: 1, errors: 1, sample_count: 0, confidence_level: 'low', usage_measured: false, input_tokens: 0, output_tokens: 0, cache_hit_tokens: 0, cache_savings_micros: 0, cost_micros: 0, latency_ms: null, ttft_ms: null, tokens_per_second: null },
      ] })
      return json({})
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<AnalyticsPage />, '/analytics?range=7d&dimension=model')

    expect(await screen.findByRole('heading', { name: 'Analytics' })).toBeInTheDocument()
    const performanceRegion = await screen.findByRole('region', { name: 'Performance and cost breakdown' })
    expect(performanceRegion).toHaveAttribute('data-scrollarea-viewport')
    expect(performanceRegion).toHaveAttribute('tabindex', '0')
    expect((await screen.findAllByText('model-alpha')).length).toBeGreaterThan(0)
    expect((await screen.findAllByText('model-unmeasured')).length).toBeGreaterThan(0)
    expect(screen.getAllByText('700 samples').length).toBeGreaterThan(0)
    expect(screen.getAllByText('High confidence').length).toBeGreaterThan(0)
    expect(screen.getAllByText('1 sample').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Low confidence').length).toBeGreaterThan(0)
    expect(screen.getAllByText('—').length).toBeGreaterThanOrEqual(3)
    await waitFor(() => {
      const url = new URL(String(fetchMock.mock.calls.find(([input]) => String(input).includes('/analytics?'))?.[0]), 'https://pangolin.test')
      expect(url.searchParams.get('dimension')).toBe('model')
      expect(Number(url.searchParams.get('until')) - Number(url.searchParams.get('from'))).toBe(604800)
    })

    await i18n.changeLanguage('zh-CN')
    expect((await screen.findAllByText('1 个样本')).length).toBeGreaterThan(0)
    expect(screen.getAllByText('低可信度').length).toBeGreaterThan(0)
    await i18n.changeLanguage('en')

    await userEvent.click(screen.getByRole('combobox', { name: 'Dimension' }))
    await userEvent.click(await screen.findByRole('option', { name: 'API key' }))
    await waitFor(() => expect(screen.getByLabelText('location')).toHaveTextContent('dimension=api_key'))
    expect(screen.getByLabelText('location')).toHaveTextContent('range=7d')
    await waitFor(() => expect(fetchMock.mock.calls.some(([input]) => String(input).includes('dimension=api_key'))).toBe(true))

  })

  it.each([
    ['provider', 'provider-6bd824dc', 'Primary provider', '/operations/channels', { data: [{ id: 'provider-6bd824dc', name: 'Primary provider' }], total: 1 }],
    ['model', 'model-6bd824dc', 'GPT production', '/operations/models', { data: [{ id: 'model-6bd824dc', public_name: 'GPT production' }], total: 1 }],
    ['api_key', 'key-6bd824dc', 'Production client', '/api-keys', [{ id: 'key-6bd824dc', name: 'Production client' }]],
    ['user', 'user-6bd824dc', 'Ada Operator', '/api/admin/v1/users', [{ id: 'user-6bd824dc', display_name: 'Ada Operator', email: 'ada@example.test' }]],
    ['project', 'project-a', 'Project A', null, null],
  ] as const)('resolves the %s dimension to its operator-facing name', async (dimension, id, name, sourcePath, sourcePayload) => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/analytics?')) return json({ data: [
        { dimension: id, requests: 3, attempts: 3, errors: 0, usage_measured: true, input_tokens: 120, output_tokens: 30, cache_hit_tokens: 4, cache_savings_micros: 12, cost_micros: 3456, latency_ms: 820, ttft_ms: 95, tokens_per_second: 36.5 },
      ] })
      if (sourcePath && path.includes(sourcePath)) return json(sourcePayload)
      return json({ data: [], total: 0 })
    }))

    renderPage(<AnalyticsPage />, `/analytics?range=7d&dimension=${dimension}`)

    expect((await screen.findAllByText(name)).length).toBeGreaterThan(0)
    expect(screen.queryByText(id)).not.toBeInTheDocument()
  })

  it('shows only the payload-free live facts when preview is enabled', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/api/v1/bootstrap')) return json({ initialized: true, authenticated: true, observability_available: true })
      if (path.includes('/settings/request-logging')) return json({ enabled: true, default_level: 'metadata', key_override_enabled: false, key_disable_allowed: false, live_preview_enabled: true })
      if (path.includes('/live-requests')) return json({ enabled: true, data: [{ model: 'gpt-live', channel_id: 'channel-safe', api_key_id: 'key-safe', started_at: 1_700_000_000 }] })
      if (path.includes('/observability/requests')) return json({ data: [], total: 0 })
      return json({ data: [], total: 0 })
    }))
    renderPage(<OperationsPage />, '/operations')

    expect(await screen.findByRole('heading', { name: 'Live requests' })).toBeInTheDocument()
    expect(screen.getByText('gpt-live')).toBeInTheDocument()
    expect(screen.getByText('channel-safe')).toBeInTheDocument()
    expect(screen.getByText('key-safe')).toBeInTheDocument()
    expect(screen.queryByText(/prompt|payload|response body/i)).not.toBeInTheDocument()
  })

  it('uses an explicit empty state when live preview is enabled with no active request', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/api/v1/bootstrap')) return json({ initialized: true, authenticated: true, observability_available: true })
      if (path.includes('/settings/request-logging')) return json({ enabled: true, default_level: 'metadata', key_override_enabled: false, key_disable_allowed: false, live_preview_enabled: true })
      if (path.includes('/live-requests')) return json({ enabled: true, data: [] })
      return json({ data: [], total: 0 })
    }))
    renderPage(<OperationsPage />, '/operations')

    expect(await screen.findByText('Nothing is currently in flight.')).toBeInTheDocument()
    expect(screen.queryByText('0')).not.toBeInTheDocument()
  })

})
