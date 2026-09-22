import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { Status } from '../components'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import OperationsPage from './OperationsPage'

/**
 * A status the gateway never observed is not a status. The projection used to
 * carry a `200` for every success and a `502` for every failure, so the log
 * claimed an upstream answer that was never received — and a rebuilt or restored
 * projection could only repeat the invention. Now `null` is the only honest value
 * and it must read `—`: `0` is not an HTTP status, and a green pill would claim a
 * success nobody measured.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const bootstrap = { initialized: true, authenticated: true, user: { id: 'u1', email: 'owner@example.test', role: 'admin', language: 'en', theme: 'system:bronze', created_at: 1 }, capture_payloads: false, observability_available: true, branding: { instance_name: 'Pangolin', branding_name: 'Pangolin', favicon_url: '/logo.webp', onboarding_complete: true } }
const policy = { enabled: true, default_level: 'metadata', key_override_enabled: false, key_disable_allowed: false }
const row = (overrides: Record<string, unknown> = {}) => ({ internal_id: `internal-${overrides.request_id ?? 'r1'}`, request_id: 'r1', started_at: 1_700_000_000, endpoint: '/v1/chat/completions', provider: 'a', requested_model: 'demo', resolved_model: 'demo', status_code: 200, error_kind: null, latency_ms: 12, input_tokens: 1, output_tokens: 1, cost_micros: 0, api_key_id: null, ...overrides })

const mockApi = (rows: unknown[], total: number) => vi.fn((input: RequestInfo | URL) => {
  const path = String(input)
  if (path.includes('/api/v1/bootstrap')) return json(bootstrap)
  if (path.includes('settings/request-logging')) return json(policy)
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('/observability/summary')) return json({ requests: 0, errors: 0, error_rate: 0, p95_latency_ms: 0, input_tokens: 0, output_tokens: 0, cost_micros: 0, series: [] })
  if (path.includes('/observability/requests')) return json({ data: rows, total, offset: 0, limit: 25 })
  return json({ data: [], total: 0 })
})
const renderList = () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/operations']}><ProjectProvider><OperationsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

describe('a status nobody measured', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('reads — instead of a number or a success pill', () => {
    const { container } = render(<Status code={null} />)
    expect(container).toHaveTextContent('—')
    expect(container.querySelector('.status-badge')).toBeNull()
    expect(container).not.toHaveTextContent('0')
  })

  it('still badges a status that was measured, including a non-200 success', () => {
    const { container: created } = render(<Status code={201} />)
    expect(created).toHaveTextContent('201')
    expect(created.querySelector('.status-badge')).not.toBeNull()
    const { container: limited } = render(<Status code={429} />)
    expect(limited).toHaveTextContent('429')
  })
})

describe('the request log with an unmeasured status', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('shows — for the unmeasured row and the exact code for the measured one', async () => {
    vi.stubGlobal('fetch', mockApi([
      row({ request_id: 'r-unmeasured', requested_model: 'unmeasured-model', status_code: null }),
      row({ request_id: 'r-created', requested_model: 'created-model', status_code: 201 }),
      row({ request_id: 'r-limited', requested_model: 'limited-model', status_code: 429, error_kind: 'failed' }),
    ], 3))
    renderList()

    expect(await screen.findByRole('columnheader', { name: 'Status' })).toBeInTheDocument()
    const unmeasured = await screen.findByRole('row', { name: /unmeasured-model/ })
    // The status column is the first cell; the row's other unmeasured facts read
    // `—` too, so the cell is what proves the status itself is unmeasured.
    const statusCell = within(unmeasured).getAllByRole('cell')[0]
    expect(statusCell).toHaveTextContent('—')
    expect(statusCell.querySelector('.status-badge')).toBeNull()
    expect(within(unmeasured).queryByText('200')).toBeNull()
    expect(within(unmeasured).queryByText('0')).toBeNull()
    expect(within(unmeasured).queryByText('502')).toBeNull()
    expect(within(screen.getByRole('row', { name: /created-model/ })).getByText('201')).toBeInTheDocument()
    expect(within(screen.getByRole('row', { name: /limited-model/ })).getByText('429')).toBeInTheDocument()
  })

  it('offers only measured codes as facets', async () => {
    const fetchMock = mockApi([
      row({ request_id: 'r-unmeasured', status_code: null }),
      row({ request_id: 'r-created', status_code: 201 }),
    ], 2)
    vi.stubGlobal('fetch', fetchMock)
    renderList()

    const facet = await screen.findByRole('combobox', { name: 'Status code' })
    await userEvent.click(facet)
    const options = await screen.findAllByRole('option')
    const offered = options.map((option) => option.textContent)
    expect(offered).toContain('201')
    expect(offered).not.toContain('0')
    expect(offered).not.toContain('null')
    await userEvent.click(options[0])
    await waitFor(() => {
      const asked = fetchMock.mock.calls.map(([path]) => String(path)).filter((path) => path.includes('/observability/requests?'))
      expect(asked.some((path) => path.includes('status_code=201'))).toBe(true)
    })
  })
})
