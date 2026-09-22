import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import ChannelsPage from './ChannelsPage'

/**
 * The gateway already records why a channel was taken out of rotation —
 * `channel_health_state` and `credential_health_state` are served on
 * `/operations/health` and `/operations/credential-health` — and the console
 * used to render none of it, so an operator saw only "disabled" and had to
 * guess. These tests pin the surface: real recorded values only, `—` when the
 * projection has nothing to say, and an explicit failure state when it cannot be
 * read at all.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const now = () => Math.floor(Date.now() / 1000)
const channel = (overrides: Record<string, unknown> = {}) => ({ id: 'c1', name: 'Primary', kind: 'openai', base_url: 'https://api.openai.com/v1', enabled: true, ...overrides })
const credential = (overrides: Record<string, unknown> = {}) => ({ id: 'k1', provider_id: 'c1', provider_name: 'Primary', credential_type: 'api_key', suffix: 'abcd', priority: 100, enabled: true, ...overrides })
/** A recorded health row: the projection's own field names, nothing invented. */
const health = (overrides: Record<string, unknown> = {}) => ({ id: 'c1', provider_id: 'c1', consecutive_failures: 0, disabled_until: null, backoff_until: null, reason: null, updated_at: 1, ...overrides })
const credentialHealth = (overrides: Record<string, unknown> = {}) => ({ id: 'k1', credential_id: 'k1', consecutive_failures: 0, disabled_until: null, updated_at: 1, ...overrides })

type Options = { channels?: unknown[]; credentials?: unknown[]; health?: unknown[]; credentialHealth?: unknown[]; healthFails?: boolean; channelOptionsFail?: boolean }
const mockApi = (options: Options = {}) => {
  const fetchMock = vi.fn((input: RequestInfo | URL) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['*'])
    if (path.includes('operations/health')) return options.healthFails ? Promise.reject(new Error('health unavailable')) : json({ data: options.health ?? [], total: (options.health ?? []).length })
    if (path.includes('operations/credential-health')) return json({ data: options.credentialHealth ?? [], total: (options.credentialHealth ?? []).length })
    if (path.includes('operations/credentials')) return json({ data: options.credentials ?? [], total: (options.credentials ?? []).length })
    if (path.includes('operations/channels')) {
      // The picker's own lookup (`limit=500`) fails while the table's page still loads.
      if (options.channelOptionsFail && path.includes('limit=500')) return Promise.reject(new Error('channels unavailable'))
      return json({ data: options.channels ?? [channel()], total: (options.channels ?? [channel()]).length })
    }
    return json({ data: [], total: 0 })
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}
const renderPage = () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><ChannelsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}
const rowFor = async (name: string) => within(await screen.findByRole('row', { name: new RegExp(name) }))

describe('the channels page states recorded channel health', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('renders the health the projection recorded, per channel', async () => {
    mockApi({
      channels: [channel(), channel({ id: 'c2', name: 'Broken' }), channel({ id: 'c3', name: 'Unreported' })],
      health: [health(), health({ id: 'c2', provider_id: 'c2', consecutive_failures: 3, disabled_until: now() + 600, reason: 'rate_limited' })],
    })
    renderPage()

    expect(await screen.findByRole('columnheader', { name: 'Channel health' })).toBeInTheDocument()
    expect((await rowFor('Primary')).getByText('Healthy')).toBeInTheDocument()
    expect((await rowFor('Broken')).getByText(/^Auto-disabled until/)).toBeInTheDocument()
    // No recorded state is unknown, never "healthy".
    expect((await rowFor('Unreported')).getAllByText('—').length).toBeGreaterThanOrEqual(1)
  })

  it('names the recorded failure count, and the backoff window while it lasts', async () => {
    mockApi({
      channels: [channel(), channel({ id: 'c2', name: 'Backing' })],
      health: [health({ consecutive_failures: 2 }), health({ id: 'c2', provider_id: 'c2', consecutive_failures: 1, backoff_until: now() + 30 })],
    })
    renderPage()
    expect((await rowFor('Primary')).getByText('2 consecutive failures')).toBeInTheDocument()
    expect((await rowFor('Backing')).getByText(/^Backing off until/)).toBeInTheDocument()
  })

  it('renders the health recorded for each credential', async () => {
    mockApi({ credentials: [credential(), credential({ id: 'k2', suffix: 'efgh' })], credentialHealth: [credentialHealth({ consecutive_failures: 4, disabled_until: now() + 300 })] })
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Credentials' }))

    expect(await screen.findByRole('columnheader', { name: 'Credential health' })).toBeInTheDocument()
    expect((await rowFor('abcd')).getByText(/^Auto-disabled until/)).toBeInTheDocument()
    expect((await rowFor('efgh')).getAllByText('—').length).toBeGreaterThanOrEqual(1)
  })

  // The fixture above carries both `id` and `credential_id`, so it passed while the
  // console matched nothing in the real response: the projection aliases the
  // credential id to `id` (`src/api/operations_api.rs:321`). A browser run found
  // the column blank for every credential. This case uses the shape the API
  // actually sends.
  it('renders credential health from the id the projection really emits', async () => {
    mockApi({ credentials: [credential()], credentialHealth: [{ id: 'k1', consecutive_failures: 4, disabled_until: now() + 300, updated_at: now() }] })
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Credentials' }))

    expect((await rowFor('abcd')).getByText(/^Auto-disabled until/)).toBeInTheDocument()
  })

  it('says so when the health projection cannot be read, and retries it', async () => {
    const fetchMock = mockApi({ healthFails: true })
    renderPage()
    expect(await screen.findByText(/Channel health could not be read/i)).toBeInTheDocument()
    const before = fetchMock.mock.calls.filter(([path]) => String(path).includes('operations/health')).length
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await vi.waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('operations/health')).length).toBeGreaterThan(before))
  })
})

describe('the probes table says why a probe failed', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('shows the recorded error code next to the status', async () => {
    mockApi()
    vi.mocked(fetch).mockImplementation((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('operations/probes')) return json({ data: [{ id: 'pr1', provider_id: 'c1', success: 0, status_code: 429, latency_ms: 12, probed_at: 1, error: 'rate_limited' }], total: 1 })
      return json({ data: [], total: 0 })
    })
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Probes' }))

    expect(await screen.findByRole('columnheader', { name: 'Error' })).toBeInTheDocument()
    expect(await screen.findByText('rate_limited')).toBeInTheDocument()
  })
})

  // The probes table rendered the projection's raw values: `probed_at` as epoch
  // seconds, a failed probe's absent HTTP status as `0`, and the channel as a bare
  // uuid. None of those is a measurement or a name an operator can use.
describe('the probes table renders values an operator can use', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('renders the probe time, the channel name and an unmeasured status honestly', async () => {
    mockApi()
    vi.mocked(fetch).mockImplementation((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('operations/probes')) return json({ data: [{ id: 'pr1', provider_id: 'c1', success: 0, status_code: 0, latency_ms: 0, probed_at: 1, error: 'timeout' }], total: 1 })
      if (path.includes('operations/channels')) return json({ data: [channel()], total: 1 })
      return json({ data: [], total: 0 })
    })
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Probes' }))

    const row = await screen.findByRole('row', { name: /Primary/ })
    expect(within(row).getByText(/1970/)).toBeInTheDocument()
    expect(within(row).queryByText('c1')).not.toBeInTheDocument()
    // A probe that never got an HTTP answer has no status, and a rejected probe has
    // no latency; `0` claims both were measured.
    expect(within(row).getAllByText('—').length).toBeGreaterThanOrEqual(2)
    expect(within(row).queryByText('0')).not.toBeInTheDocument()
  })
})

describe('a picker whose option list failed says so', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('replaces the empty channel picker with an error and a retry', async () => {
    const fetchMock = mockApi({ channelOptionsFail: true })
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Credentials' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Add credential' }))

    expect(await screen.findByText('The option list could not be loaded.')).toBeInTheDocument()
    const before = fetchMock.mock.calls.filter(([path]) => String(path).includes('limit=500')).length
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await vi.waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('limit=500')).length).toBeGreaterThan(before))
  })

  it('still offers the picker once the option list loads', async () => {
    mockApi()
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Credentials' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Add credential' }))
    expect(await screen.findByRole('combobox', { name: /Channel/ })).toBeInTheDocument()
    expect(screen.queryByText('The option list could not be loaded.')).not.toBeInTheDocument()
  })
})
