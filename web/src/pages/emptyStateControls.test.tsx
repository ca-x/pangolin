import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import AccessPage from './AccessPage'
import ChannelsPage from './ChannelsPage'
import OperationsPage from './OperationsPage'
import { ResourcePage } from './shared'

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const renderPage = (page: React.ReactElement) => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider>{page}</ProjectProvider></MemoryRouter></QueryClientProvider>)
}
const base = (path: string) => path.endsWith('/projects') ? json([project]) : path.includes('/permissions') ? json(['*']) : null
const requestRow = { request_id: 'r1', started_at: 1, endpoint: '/v1/chat/completions', provider: 'openai', requested_model: 'demo', resolved_model: 'demo', status_code: 200, latency_ms: 12, input_tokens: 1, output_tokens: 1, cost_micros: 0 }
// One request is listed until a status-code filter narrows it away.
const requestsMock = () => vi.fn((input: RequestInfo | URL) => base(String(input)) ?? json(String(input).includes('status_code=500')
  ? { data: [], total: 0, offset: 0, limit: 25 }
  : { data: [requestRow], total: 1, offset: 0, limit: 25 }))

describe('empty states drop controls that cannot do anything', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('hides the bulk channel form until a channel exists', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => base(String(input)) ?? json({ data: [], total: 0, offset: 0, limit: 25 })))
    renderPage(<ChannelsPage />)
    expect(await screen.findByText(/Add a channel, then attach/i)).toBeInTheDocument()
    expect(screen.queryByText('Bulk enable or disable')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Apply' })).not.toBeInTheDocument()
    // one entry point only: the empty state owns it, the header drops its button
    expect(screen.getAllByRole('button', { name: 'Add channel' })).toHaveLength(1)
  })

  it('shows the bulk channel form once a channel exists', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => base(String(input)) ?? json({ data: [{ id: 'c1', name: 'Primary', kind: 'openai', base_url: 'https://api.openai.com', enabled: true }], total: 1, offset: 0, limit: 25 })))
    renderPage(<ChannelsPage />)
    expect(await screen.findByRole('button', { name: 'Add channel' })).toBeInTheDocument()
    expect(screen.getByText('Bulk enable or disable')).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Add channel' })).toHaveLength(1)
  })

  it('hides the request filters and pause control while there are no requests', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => base(String(input)) ?? json({ data: [], total: 0, offset: 0, limit: 25 })))
    renderPage(<OperationsPage />)
    expect(await screen.findByText(/No requests are available/i)).toBeInTheDocument()
    expect(screen.queryByLabelText('Status code')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Pause' })).not.toBeInTheDocument()
  })

  it('keeps the request filters and offers to clear them when a filter matches nothing', async () => {
    vi.stubGlobal('fetch', requestsMock())
    renderPage(<OperationsPage />)
    expect(await screen.findByRole('button', { name: 'Pause' })).toBeInTheDocument()
    await userEvent.type(screen.getByLabelText('Status code'), '500')
    expect(await screen.findByRole('button', { name: 'Clear filters' })).toBeInTheDocument()
    expect(screen.getByLabelText('Status code')).toHaveValue('500')
    await userEvent.click(screen.getByRole('button', { name: 'Clear filters' }))
    expect(screen.getByLabelText('Status code')).toHaveValue('')
  })

  it('localizes the clear-filters action', async () => {
    vi.stubGlobal('fetch', requestsMock())
    renderPage(<OperationsPage />)
    await userEvent.type(await screen.findByLabelText('Status code'), '500')
    await i18n.changeLanguage('zh-CN')
    expect(await screen.findByRole('button', { name: '清除筛选条件' })).toBeInTheDocument()
    await i18n.changeLanguage('en')
  })

  it('keeps the bulk form when the count request fails', async () => {
    // The count is an auxiliary request. A failure there must not read as "no
    // rows" and silently remove a control the page can still use.
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.includes('limit=1')) return Promise.reject(new Error('count unavailable'))
      return base(path) ?? json({ data: [{ id: 'c1', name: 'Primary', kind: 'openai', base_url: 'https://api.openai.com', enabled: true }], total: 1, offset: 0, limit: 25 })
    }))
    renderPage(<ChannelsPage />)
    expect(await screen.findByText('Bulk enable or disable')).toBeInTheDocument()
  })

  it('keeps a required JSON editor open, so an invalid one can be reported', async () => {
    // A collapsed editor keeps `required` on a control the browser cannot focus,
    // which blocks Save with no actionable error.
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => base(String(input)) ?? json({ data: [], total: 0, offset: 0, limit: 25 })))
    renderPage(
      <ResourcePage
        resource="models"
        title="Models"
        description="d"
        empty="none"
        createLabel="Add model"
        columns={[{ key: 'name', label: 'Name' }]}
        fields={[{ key: 'name', label: 'Name', required: true }, { key: 'components', label: 'Components', kind: 'json', required: true }]}
      />,
    )
    await userEvent.click(await screen.findByRole('button', { name: 'Add model' }))
    const editor = await screen.findByLabelText(/^Components/)
    expect(editor.closest('[inert]')).toBeNull()
  })

  it('hides per-key request logging until a key exists', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => base(String(input)) ?? json([])))
    renderPage(<AccessPage />)
    expect(await screen.findByText(/Create a virtual key/i)).toBeInTheDocument()
    expect(screen.queryByText('Per-key request logging')).not.toBeInTheDocument()
    // the empty state owns the single call to action, so no duplicate header button
    expect(screen.getAllByRole('button', { name: 'Create API key' })).toHaveLength(1)
  })

  it('keeps the header action and per-key logging when keys exist', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => base(String(input)) ?? json([{ id: 'k1', name: 'Client', key_prefix: 'pk_1', budget_micros: null, enabled: true, key_type: 'service', profile_id: null, expires_at: null, allowed_ips_json: '[]', denied_ips_json: '[]' }])))
    renderPage(<AccessPage />)
    expect(await screen.findByText('Client')).toBeInTheDocument()
    expect(screen.getByText('Per-key request logging')).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Create API key' })).toHaveLength(1)
  })
})
