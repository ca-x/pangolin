import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { ThemeProvider } from '../theme'
import ChannelsPage from './ChannelsPage'
import SystemPage from './SystemPage'

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }

function renderPage(page: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><ThemeProvider><MemoryRouter><ProjectProvider>{page}</ProjectProvider></MemoryRouter></ThemeProvider></QueryClientProvider>)
}

describe('OAuth and HTTP policy settings', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en')
  })

  it('edits exact CORS origins and the bounded request timeout', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/settings/system') && init?.method === 'PUT') return json({ ok: true })
      if (path.includes('/settings/system')) return json({ instance_name: 'Pangolin', branding_name: 'Pangolin / 鲮鲤', favicon_url: '/logo.webp', onboarding_complete: false, cors_allowed_origins: ['https://old.example'], request_timeout_ms: 600000 })
      return json({ data: [] })
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<SystemPage />)
    const origins = await screen.findByLabelText(/Allowed cross-origin origins/)
    const form = origins.closest('form') as HTMLFormElement
    await userEvent.clear(origins)
    await userEvent.type(origins, 'https://one.example\nhttps://two.example')
    const timeout = within(form).getByLabelText(/Request timeout/)
    await userEvent.clear(timeout)
    await userEvent.type(timeout, '2500')
    await userEvent.click(within(form).getByRole('button', { name: 'Save' }))
    await waitFor(() => {
      const call = fetchMock.mock.calls.find(([path, init]) => String(path).includes('/settings/system') && init?.method === 'PUT')
      expect(JSON.parse(String(call?.[1]?.body))).toMatchObject({ cors_allowed_origins: ['https://one.example', 'https://two.example'], request_timeout_ms: 2500 })
    })
  })

  it('starts a provider flow without rendering a token field', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/operations/channels')) return json({ data: [{ id: 'channel-1', name: 'Primary', kind: 'openai', base_url: 'https://api.openai.com', enabled: true }], total: 1 })
      if (path.includes('/oauth/codex/start') && init?.method === 'POST') return json({ state: 'opaque-state', authorization_url: 'https://idp.example/authorize?state=opaque-state' })
      return json({ data: [], total: 0 })
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<ChannelsPage />)
    await userEvent.click(await screen.findByRole('tab', { name: 'Credentials' }))
    expect(screen.queryByLabelText(/OAuth client ID/)).not.toBeInTheDocument()
    expect(await screen.findByRole('button', { name: 'Add API key' })).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Add OAuth' }))
    await userEvent.type(await screen.findByLabelText(/OAuth client ID/), 'client-id')
    await userEvent.click(screen.getByRole('button', { name: 'Start authorization' }))
    expect(await screen.findByRole('link', { name: 'Open provider in a new tab' })).toHaveAttribute('href', expect.stringContaining('opaque-state'))
    expect(screen.queryByLabelText(/access token/i)).not.toBeInTheDocument()
    const call = fetchMock.mock.calls.find(([path]) => String(path).includes('/oauth/codex/start'))
    expect(JSON.parse(String(call?.[1]?.body))).toMatchObject({ client_id: 'client-id', redirect_uri: expect.stringContaining('/channels') })
  })

  it('shows OAuth credentials in the shared list and closes the authorization dialog on success', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/operations/channels')) return json({ data: [{ id: 'channel-1', name: 'Primary', kind: 'openai', enabled: true }], total: 1 })
      if (path.includes('/operations/credentials')) return json({ data: [{ id: 'cred-1', provider_id: 'channel-1', provider_name: 'Primary', credential_type: 'oauth_codex', suffix: '1234', enabled: true, priority: 100, state: 'ready' }], total: 1 })
      if (path.includes('/oauth/codex/start') && init?.method === 'POST') return json({ state: 'opaque-state', authorization_url: 'https://idp.example/authorize' })
      if (path.includes('/oauth/codex/complete') && init?.method === 'POST') return json({ status: 'created' })
      return json({ data: [], total: 0 })
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<ChannelsPage />)
    await userEvent.click(await screen.findByRole('tab', { name: 'Credentials' }))
    expect((await screen.findAllByText('1234')).length).toBeGreaterThan(0)
    await userEvent.click(screen.getByRole('button', { name: 'Add OAuth' }))
    await userEvent.type(screen.getByLabelText(/OAuth client ID/), 'client-id')
    await waitFor(() => expect(screen.getByRole('button', { name: 'Start authorization' })).toBeEnabled())
    await userEvent.click(screen.getByRole('button', { name: 'Start authorization' }))
    expect(await screen.findByRole('link', { name: 'Open provider in a new tab' })).toBeInTheDocument()
    await userEvent.type(await screen.findByLabelText(/Authorization code/), 'code')
    await userEvent.click(screen.getByRole('button', { name: 'Save encrypted credential' }))
    await waitFor(() => expect(screen.queryByLabelText(/OAuth client ID/)).not.toBeInTheDocument())
    expect(fetchMock.mock.calls.some(([path]) => String(path).includes('/oauth/codex/complete'))).toBe(true)
  })
})
