import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import ChannelsPage from './ChannelsPage'
import ModelsPage from './ModelsPage'

const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const reply = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const renderPage = (page: ReactNode) => render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><MemoryRouter><ProjectProvider>{page}</ProjectProvider></MemoryRouter></QueryClientProvider>)

describe('B58-B61 console workflows', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('names an unrecoverable credential and replaces it through the one-time flow', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return reply([project])
      if (path.includes('/permissions')) return reply(['*'])
      if (path.includes('/recovery-token')) return reply({ token: 'pcr_once' })
      if (path.endsWith('/recover')) return reply({ ok: true })
      if (path.includes('credential-health')) return reply({ data: [], total: 0 })
      if (path.includes('operations/credentials')) return reply({ data: [{ id: 'k1', provider_id: 'c1', provider_name: 'Primary', credential_type: 'api_key', suffix: 'abcd', priority: 100, enabled: true, state: 'unrecoverable' }], total: 1 })
      if (path.includes('operations/channels')) return reply({ data: [{ id: 'c1', name: 'Primary', kind: 'openai', base_url: 'https://example.test', enabled: true }], total: 1 })
      return reply({ data: [], total: 0 })
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<ChannelsPage />)
    await userEvent.click(await screen.findByRole('tab', { name: 'Credentials' }))
    expect(await screen.findByText('Unrecoverable')).toBeInTheDocument()
    await userEvent.click(screen.getAllByRole('button', { name: 'Replace credential' }).at(-1)!)
    await vi.waitFor(() => expect(fetchMock.mock.calls.some(([input]) => String(input).includes('/recovery-token'))).toBe(true))
    const recoveryDialog = await screen.findByRole('dialog')
    const secret = recoveryDialog.querySelector('input[type="password"]') as HTMLInputElement
    expect(secret).toBeInTheDocument()
    await userEvent.type(secret, 'replacement-secret')
    await userEvent.click(within(recoveryDialog).getByRole('button', { name: 'Replace credential' }))
    await vi.waitFor(() => expect(fetchMock.mock.calls.some(([input, init]) => String(input).endsWith('/recover') && init?.method === 'POST')).toBe(true))
  })

  it('renders unknown quota as unmeasured and exposes clone, merge, and guarded bulk delete', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return reply([project])
      if (path.includes('/permissions')) return reply(['*'])
      if (path.includes('operations/channels')) return reply({ data: [{ id: 'c1', name: 'Primary', kind: 'openai', base_url: 'https://example.test', enabled: true }], total: 1 })
      if (path.includes('operations/health')) return reply({ data: [], total: 0 })
      if (path.includes('operations/quotas')) return reply({ data: [{ id: 'q1', provider_id: 'c1', remaining_micros: null, quota: { measured: false, period: 'weekly', source_url: 'https://example.test/quota' }, period_end: null, collected_at: 1 }], total: 1 })
      return reply({ data: [], total: 0 })
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<ChannelsPage />)
    expect(await screen.findAllByRole('button', { name: 'Clone channel' })).not.toHaveLength(0)
    expect(screen.getAllByRole('button', { name: 'Merge channel' })).not.toHaveLength(0)
    await userEvent.click(screen.getByLabelText('Select Primary'))
    expect(screen.getAllByRole('button', { name: 'Delete' }).length).toBeGreaterThanOrEqual(1)
    await userEvent.click(screen.getByRole('tab', { name: 'Quotas' }))
    const row = within(await screen.findByRole('row', { name: /weekly/ }))
    expect(row.getAllByText('—').length).toBeGreaterThanOrEqual(1)
    expect(row.getByText('https://example.test/quota')).toBeInTheDocument()
  })

  it('round-trips unknown developer rules and offers the per-model inheritance toggle', async () => {
    const developerDocument = { version: 1, rules: [{ developer: 'future-vendor', associations: [], reasoning_effort: { auto: 'high' } }] }
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return reply([project])
      if (path.includes('/permissions')) return reply(['*'])
      if (path.includes('/settings/developers')) return init?.method === 'PUT' ? reply({ ok: true }) : reply(developerDocument)
      if (path.includes('/operations/models')) return reply({ data: [{ id: 'm1', provider_id: 'c1', provider_name: 'Primary', public_name: 'model', upstream_name: 'model', capabilities: ['chat'], enabled: true, lifecycle: 'active', disable_developer_settings_inheritance: false }], total: 1 })
      if (path.includes('/operations/channels')) return reply({ data: [{ id: 'c1', name: 'Primary' }], total: 1 })
      if (path.includes('/catalog/models')) return reply({ data: [], total: 0 })
      return reply({ data: [], total: 0 })
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<ModelsPage />)
    await userEvent.click(await screen.findByRole('button', { name: 'Developer settings' }))
    expect(await screen.findByDisplayValue(/future-vendor/)).toBeInTheDocument()
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Save' }))
    await vi.waitFor(() => expect(fetchMock.mock.calls.some(([input, init]) => String(input).includes('/settings/developers') && init?.method === 'PUT' && String(init.body).includes('future-vendor'))).toBe(true))
    await vi.waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    await userEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0])
    await screen.findByRole('dialog')
    expect(document.querySelector('input[name="disable_developer_settings_inheritance"]')).toBeInTheDocument()
  })
})
