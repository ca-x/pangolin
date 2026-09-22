import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import AccessPage from './AccessPage'

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), {
  status: 200,
  headers: { 'Content-Type': 'application/json' },
}))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const key = {
  id: 'key-one', name: 'Key one', key_prefix: 'abcd1234', scopes: ['gateway:use'],
  budget_micros: null, spent_micros: 0, enabled: true, key_type: 'service',
  profile_id: null, expires_at: null, last_used_at: null,
  allowed_ips_json: '[]', denied_ips_json: '[]',
}

function mockApi(rows: unknown[] = [], analyticsFails = false) {
  return vi.fn((input: RequestInfo | URL) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['api_key:manage'])
    if (path.includes('/api-keys')) return json([key])
    if (path.includes('/key-profiles')) return json({ data: [] })
    if (path.includes('/analytics?')) return analyticsFails ? Promise.reject(new Error('analytics unavailable')) : json({ data: rows, source: 'sqlite', derived_available: true })
    return json([])
  })
}

function renderPage(fetchMock: ReturnType<typeof vi.fn>) {
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

describe('per-key usage from the key row', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })

  it('opens only that key usage and reports measured tokens and cost', async () => {
    const fetchMock = mockApi([{
      dimension: 'model-a', requests: 2, attempts: 2, errors: 0,
      input_tokens: 10, output_tokens: 5, cache_hit_tokens: 3,
      cache_savings_micros: 0, cost_micros: 1234, latency_ms: 20, ttft_ms: 4,
    }])
    renderPage(fetchMock)

    await userEvent.click(await screen.findByRole('button', { name: 'Usage Key one' }))
    const dialog = await screen.findByRole('dialog', { name: 'Token usage — Key one' })

    await waitFor(() => expect(fetchMock.mock.calls.some(([input]) => {
      const url = new URL(String(input), 'https://pangolin.test')
      return url.pathname === '/api/admin/v1/projects/p1/analytics'
        && url.searchParams.get('dimension') === 'model'
        && url.searchParams.get('api_key') === 'key-one'
    })).toBe(true))
    expect(within(dialog).getByText('10')).toBeInTheDocument()
    expect(within(dialog).getByText('5')).toBeInTheDocument()
    expect(within(dialog).getByText('15')).toBeInTheDocument()
    expect(within(dialog).getByText('$0.001234')).toBeInTheDocument()
    expect(within(dialog).getByText('model-a')).toBeInTheDocument()

    const analyticsCalls = () => fetchMock.mock.calls.filter(([input]) => String(input).includes('/analytics?'))
    const before = analyticsCalls().length
    await userEvent.click(within(dialog).getByRole('tab', { name: 'Last 7 days' }))
    await waitFor(() => expect(analyticsCalls().length).toBeGreaterThan(before))
    expect(new URL(String(analyticsCalls().at(-1)?.[0]), 'https://pangolin.test').searchParams.get('from')).not.toBe('0')

    await userEvent.click(within(dialog).getByRole('tab', { name: 'All retained history' }))
    await waitFor(() => expect(new URL(String(analyticsCalls().at(-1)?.[0]), 'https://pangolin.test').searchParams.get('from')).toBe('0'))
  })

  it('shows unmeasured marks when requests exist without a usage measurement', async () => {
    const fetchMock = mockApi([{
      dimension: 'model-unmeasured', requests: 1, attempts: 1, errors: 1,
      input_tokens: 0, output_tokens: 0, cache_hit_tokens: 0,
      cache_savings_micros: 0, cost_micros: 0, latency_ms: 20, ttft_ms: null,
      usage_measured: false,
    }])
    renderPage(fetchMock)

    await userEvent.click(await screen.findByRole('button', { name: 'Usage Key one' }))
    const dialog = await screen.findByRole('dialog', { name: 'Token usage — Key one' })

    expect((await within(dialog).findAllByText('—')).length).toBeGreaterThanOrEqual(4)
    expect(within(dialog).getByText('No usage was measured for this key in the selected window.')).toBeInTheDocument()
    expect(within(dialog).getByText('model-unmeasured')).toBeInTheDocument()
  })

  it('keeps a failed key usage read retryable', async () => {
    const fetchMock = mockApi([], true)
    renderPage(fetchMock)

    await userEvent.click(await screen.findByRole('button', { name: 'Usage Key one' }))
    const dialog = await screen.findByRole('dialog', { name: 'Token usage — Key one' })
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('The request failed. Try again shortly.')

    const before = fetchMock.mock.calls.filter(([input]) => String(input).includes('/analytics?')).length
    await userEvent.click(within(dialog).getByRole('button', { name: 'Retry' }))
    await waitFor(() => expect(fetchMock.mock.calls.filter(([input]) => String(input).includes('/analytics?')).length).toBeGreaterThan(before))
  })
})
