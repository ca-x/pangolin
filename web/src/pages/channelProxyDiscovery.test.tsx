import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import ChannelsPage from './ChannelsPage'

vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const channel = { id: 'c1', name: 'Primary', kind: 'openai', base_url: 'https://api.openai.com/v1', enabled: true }
const settings = { id: 'c1', provider_id: 'c1', proxy_url: 'socks5h://proxy.internal:1080', proxy_username: 'operator', proxy_password_configured: true, proxy_reuse_connections: false, model_sync_error: null, model_synced_at: 1_700_000_000, model_sync_count: 2, endpoint_mappings: { version: 1 }, model_rules: { version: 1 }, parameter_overrides: { version: 1 }, retry_statuses: { version: 1, statuses: [429] }, auto_disable_policy: { version: 1, enabled: false } }

function setup() {
  const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['*'])
    if (path.includes('operations/health') || path.includes('credential-health')) return json({ data: [], total: 0 })
    if (path.includes('operations/model-sync') && init?.method === 'POST') return json({ id: 'job-1' })
    if (path.includes('operations/channel-settings')) return init?.method === 'POST' ? json({ id: 'c1' }) : json({ data: [settings], total: 1 })
    if (path.includes('operations/channels')) return json({ data: [channel], total: 1 })
    return json({ data: [], total: 0 })
  })
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><ChannelsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
  return fetchMock
}

describe('channel proxy and model discovery controls', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('keeps the proxy password write-only and submits a replacement separately', async () => {
    const fetchMock = setup()
    await userEvent.click(await screen.findByRole('tab', { name: 'Channel policies' }))
    const row = await screen.findByRole('row', { name: /Primary/ })
    expect(within(row).getByText('socks5h://proxy.internal:1080')).toBeInTheDocument()
    expect(within(row).getByText(/2 models discovered/)).toBeInTheDocument()
    await userEvent.click(within(row).getByRole('button', { name: 'Edit' }))
    const dialog = await screen.findByRole('dialog')
    const password = within(dialog).getByLabelText('Proxy password')
    expect(password).toHaveValue('')
    await userEvent.type(password, 'replacement-secret')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      const call = fetchMock.mock.calls.find(([input, init]) => String(input).includes('operations/channel-settings') && (init as RequestInit | undefined)?.method === 'POST')
      expect(call).toBeTruthy()
      const body = JSON.parse(String((call![1] as RequestInit).body))
      expect(body.proxy_password).toBe('replacement-secret')
      expect(JSON.stringify(settings)).not.toContain('replacement-secret')
    })
  })

  it('queues discovery for the channel from its row', async () => {
    const fetchMock = setup()
    const table = within(await screen.findByRole('region', { name: 'Channels' }))
    await userEvent.click(await table.findByRole('button', { name: 'Sync models Primary' }))
    await waitFor(() => {
      const call = fetchMock.mock.calls.find(([input, init]) => String(input).includes('operations/model-sync') && (init as RequestInit | undefined)?.method === 'POST')
      expect(JSON.parse(String((call![1] as RequestInit).body))).toEqual({ provider_id: 'c1' })
    })
  })
})
