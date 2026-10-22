import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import ModelsPage from './ModelsPage'
import type { CatalogSource } from './ModelsDevSubscription'

const endpoint = '/api/admin/v1/catalog/sources'
const officialUrl = 'https://models.dev/api.json'
const preset = { name: 'models.dev', url: officialUrl, priority: 50, enabled: true, refresh_interval_secs: 86400, signature_policy: 'none', public_key: null }
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const source = (overrides: Partial<CatalogSource> = {}): CatalogSource & { etag: null; last_modified: null } => ({
  ...preset, id: 'modelsdev-1', revision: 3, etag: null, last_modified: null, proxy_preset_id: null,
  last_attempt_at: null, last_success_at: null, last_error: null, active_snapshot_id: null, previous_snapshot_id: null,
  ...overrides,
})
const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const apiError = (message: string, status = 502) => json({ error: { type: 'upstream_error', message } }, status)

function setup({ sources = [], path = '/models?tab=subscriptions', permissions = ['*'], handle }: {
  sources?: ReturnType<typeof source>[]
  path?: string
  permissions?: string[]
  handle?: (path: string, init?: RequestInit) => Promise<Response> | undefined
} = {}) {
  let rows = sources
  const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    const result = handle?.(url, init)
    if (result) return result
    if (url.endsWith('/projects')) return json([project])
    if (url.includes('/permissions')) return json(permissions)
    if (url === endpoint && init?.method === 'POST') {
      const created = source(JSON.parse(String(init.body)))
      rows = [...rows, created]
      return json(created, 201)
    }
    if (url === `${endpoint}/modelsdev-1` && init?.method === 'PUT') {
      const updated = JSON.parse(String(init.body))
      const current = rows.find((row) => row.id === 'modelsdev-1') || source()
      rows = [{ ...current, ...updated.source, revision: updated.revision + 1 }]
      return json(rows[0])
    }
    if (url === endpoint) return json(rows)
    if (url.endsWith('/refresh') && init?.method === 'POST') {
      rows = rows.map((row) => ({ ...row, active_snapshot_id: 'snapshot-new', last_success_at: 1_790_000_000, last_error: null }))
      return json({ source_id: 'modelsdev-1', status: 'activated', snapshot_id: 'snapshot-new' })
    }
    if (url.endsWith('/snapshots')) return json([])
    return json({ data: [], total: 0 })
  })
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[path]}><ProjectProvider><ModelsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
  const writes = () => fetchMock.mock.calls.filter(([, init]) => init?.method && init.method !== 'GET')
  return { client, fetchMock, writes }
}

beforeEach(async () => { await i18n.changeLanguage('en') })

describe('models.dev explicit subscription', () => {
  it('does not opt in on mount and explains metadata, daily refresh and routing/billing boundaries', async () => {
    const { fetchMock, writes } = setup()
    const card = await screen.findByRole('region', { name: 'models.dev reference metadata' })
    expect(within(card).getByRole('button', { name: 'Connect models.dev' })).toBeEnabled()
    expect(within(card).getByText(/only after you connect/i)).toBeInTheDocument()
    expect(within(card).getByText(/does not enable providers, change routes or update live billing prices/i)).toBeInTheDocument()
    expect(writes()).toHaveLength(0)
    expect(fetchMock.mock.calls.every(([path]) => String(path).startsWith('/api/'))).toBe(true)
  })

  it('creates the exact named preset using the real Source response, then refreshes it', async () => {
    const { writes } = setup()
    await userEvent.click(await screen.findByRole('button', { name: 'Connect models.dev' }))
    await waitFor(() => expect(writes()).toHaveLength(2))
    const [create, refresh] = writes()
    expect(create[0]).toBe(endpoint)
    expect(create[1]?.method).toBe('POST')
    expect(JSON.parse(String(create[1]?.body))).toEqual(preset)
    expect(create[1]?.headers).toMatchObject({ 'X-Pangolin-CSRF': '1' })
    expect(refresh[0]).toBe(`${endpoint}/modelsdev-1/refresh`)
    expect(refresh[1]?.method).toBe('POST')
    expect(JSON.parse(String(refresh[1]?.body))).toEqual({})
    expect(await screen.findByText('Reference metadata ready')).toBeInTheDocument()
  })

  it('rechecks the current list before creation and reuses an existing canonical URL', async () => {
    let reads = 0
    const { writes } = setup({ handle: (path, init) => {
      if (path === endpoint && !init?.method) return json(++reads === 1 ? [] : [source({ url: 'https://MODELS.DEV:443/api.json', name: 'Existing reference' })])
    } })
    await userEvent.click(await screen.findByRole('button', { name: 'Connect models.dev' }))
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(writes()[0][0]).toBe(`${endpoint}/modelsdev-1/refresh`)
  })

  it('enables an existing disabled source without resetting signature, interval, priority or proxy settings', async () => {
    const existing = source({ enabled: false, name: 'Pinned reference', refresh_interval_secs: 7200, priority: 7, signature_policy: 'required', public_key: 'configured-public-key', proxy_preset_id: 'proxy-1' })
    const { writes } = setup({ sources: [existing] })
    await userEvent.click(await screen.findByRole('button', { name: 'Enable and sync' }))
    await waitFor(() => expect(writes()).toHaveLength(2))
    expect(writes()[0][0]).toBe(`${endpoint}/modelsdev-1`)
    expect(writes()[0][1]?.method).toBe('PUT')
    expect(JSON.parse(String(writes()[0][1]?.body))).toEqual({ revision: 3, source: {
      name: existing.name, url: existing.url, priority: 7, enabled: true, refresh_interval_secs: 7200,
      signature_policy: 'required', public_key: existing.public_key, proxy_preset_id: 'proxy-1',
    } })
    expect(writes()[1][0]).toBe(`${endpoint}/modelsdev-1/refresh`)
  })

  it('honors a disabled source returned by an idempotent create during a cross-client race', async () => {
    const existing = source({ enabled: false, signature_policy: 'required', public_key: 'configured-public-key', proxy_preset_id: 'proxy-1' })
    const { writes } = setup({ handle: (path, init) => {
      if (path === endpoint && init?.method === 'POST') return json(existing, 201)
    } })
    await userEvent.click(await screen.findByRole('button', { name: 'Connect models.dev' }))
    await waitFor(() => expect(writes()).toHaveLength(3))
    expect(writes()[0][0]).toBe(endpoint)
    expect(writes()[1][1]?.method).toBe('PUT')
    expect(JSON.parse(String(writes()[1][1]?.body))).toMatchObject({ revision: 3, source: { enabled: true, signature_policy: 'required', public_key: existing.public_key, proxy_preset_id: 'proxy-1' } })
    expect(writes()[2][0]).toBe(`${endpoint}/modelsdev-1/refresh`)
  })

  it('disables catalog participation and fetching while preserving the saved snapshot and configured project models', async () => {
    const { client, writes } = setup({ sources: [source({ active_snapshot_id: 'snapshot-good', last_success_at: 1_790_000_000 })] })
    const card = await screen.findByRole('region', { name: 'models.dev reference metadata' })
    expect(within(card).getByText('Disabling stops automatic fetching and excludes this source from the effective catalog. Saved snapshots and configured project models are preserved.')).toBeInTheDocument()
    await userEvent.click(within(card).getByRole('button', { name: 'Disable subscription' }))
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(JSON.parse(String(writes()[0][1]?.body)).source.enabled).toBe(false)
    expect(await within(card).findByText('Subscription disabled')).toBeInTheDocument()
    expect(within(card).getByText('snapshot-good')).toBeInTheDocument()
    expect(within(card).getByText('Saved snapshot')).toBeInTheDocument()
    expect(within(card).getByRole('button', { name: 'Enable and sync' })).toBeEnabled()
    expect(client.getQueryData<CatalogSource[]>(['catalog-sources'])?.[0]).toMatchObject({ enabled: false, active_snapshot_id: 'snapshot-good', last_success_at: 1_790_000_000 })
    expect(writes().some(([path]) => String(path).includes('/projects/'))).toBe(false)
  })

  it('opens the existing editor and preserves a configured proxy when editing the source', async () => {
    const { writes } = setup({ sources: [source({ proxy_preset_id: 'proxy-1' })] })
    await userEvent.click(await screen.findByRole('button', { name: 'Edit subscription' }))
    const dialog = await screen.findByRole('dialog', { name: 'Edit subscription' })
    const name = within(dialog).getByRole('textbox', { name: 'Name' })
    await userEvent.clear(name)
    await userEvent.type(name, 'My model reference')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(JSON.parse(String(writes()[0][1]?.body))).toMatchObject({ revision: 3, source: { name: 'My model reference', proxy_preset_id: 'proxy-1' } })
    expect(writes()[0][1]?.method).toBe('PUT')
  })

  it('keeps failed edits and the server error inline so the operator can retry', async () => {
    let fail = true
    const { writes } = setup({ sources: [source()], handle: (path, init) => {
      if (path === `${endpoint}/modelsdev-1` && init?.method === 'PUT' && fail) return apiError('Subscription could not be saved', 503)
    } })
    await userEvent.click(await screen.findByRole('button', { name: 'Edit subscription' }))
    const dialog = await screen.findByRole('dialog', { name: 'Edit subscription' })
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('Subscription could not be saved')
    expect(within(dialog).getByRole('textbox', { name: 'Name' })).toHaveValue('models.dev')
    fail = false
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(writes()).toHaveLength(2)
  })

  it('keeps last-known-good metadata visible after failure and retries without creating a duplicate', async () => {
    let fail = true
    const { writes } = setup({ sources: [source({ active_snapshot_id: 'snapshot-good', last_success_at: 1_790_000_000, last_error: 'Catalog fetch failed' })], handle: (path, init) => {
      if (path.endsWith('/refresh') && init?.method === 'POST' && fail) return apiError('Catalog fetch failed')
    } })
    const card = await screen.findByRole('region', { name: 'models.dev reference metadata' })
    expect(within(card).getByText('Last-known-good metadata remains active.')).toBeInTheDocument()
    await userEvent.click(within(card).getByRole('button', { name: 'Retry sync' }))
    const error = await within(card).findByRole('alert')
    expect(error).toHaveTextContent('Catalog fetch failed')
    fail = false
    await userEvent.click(within(error).getByRole('button', { name: 'Retry' }))
    await waitFor(() => expect(writes()).toHaveLength(2))
    expect(writes().every(([path]) => String(path).endsWith('/refresh'))).toBe(true)
    expect(await within(card).findByText('Reference metadata ready')).toBeInTheDocument()
  })

  it('retries a failed first refresh using the source that was already created', async () => {
    let fail = true
    const { writes } = setup({ handle: (path, init) => {
      if (path.endsWith('/refresh') && init?.method === 'POST' && fail) return apiError('Catalog fetch failed')
    } })
    await userEvent.click(await screen.findByRole('button', { name: 'Connect models.dev' }))
    const error = await screen.findByRole('alert')
    fail = false
    await userEvent.click(within(error).getByRole('button', { name: 'Retry' }))
    await waitFor(() => expect(writes()).toHaveLength(3))
    expect(writes().filter(([path]) => path === endpoint)).toHaveLength(1)
  })

  it('fails closed when the source lookup fails and allows an explicit retry', async () => {
    let fail = false
    const { writes } = setup({ handle: (path, init) => {
      if (path === endpoint && !init?.method && fail) return apiError('Subscriptions unavailable', 503)
    } })
    const connect = await screen.findByRole('button', { name: 'Connect models.dev' })
    fail = true
    await userEvent.click(connect)
    const card = screen.getByRole('region', { name: 'models.dev reference metadata' })
    const error = await within(card).findByRole('alert')
    expect(error).toHaveTextContent('Subscriptions unavailable')
    expect(writes()).toHaveLength(0)
    fail = false
    await userEvent.click(within(error).getByRole('button', { name: 'Retry' }))
    expect(await within(card).findByText('Reference metadata ready')).toBeInTheDocument()
    expect(writes()).toHaveLength(2)
  })

  it('keeps a failed source without a snapshot distinct from last-known-good metadata', async () => {
    setup({ sources: [source({ last_error: 'Catalog fetch failed' })] })
    const card = await screen.findByRole('region', { name: 'models.dev reference metadata' })
    expect(within(card).getByText('No metadata has been synced yet.')).toBeInTheDocument()
    expect(within(card).queryByText('Last-known-good metadata remains active.')).not.toBeInTheDocument()
    expect(within(card).getByRole('button', { name: 'Edit subscription' })).toBeEnabled()
  })

  it('describes a disabled failed source snapshot as saved but excluded, never active', async () => {
    setup({ sources: [source({ enabled: false, active_snapshot_id: 'snapshot-good', last_error: 'Catalog fetch failed' })] })
    const card = await screen.findByRole('region', { name: 'models.dev reference metadata' })
    expect(within(card).getByText('The last-known-good snapshot is saved but excluded from the effective catalog while this subscription is disabled.')).toBeInTheDocument()
    expect(within(card).getByText('snapshot-good')).toBeInTheDocument()
    expect(within(card).getByText('Last attempt failed: Catalog fetch failed')).toBeInTheDocument()
    expect(within(card).queryByText('Last-known-good metadata remains active.')).not.toBeInTheDocument()
    expect(within(card).queryByText('Active snapshot')).not.toBeInTheDocument()
  })

  it('localizes disabled catalog participation and snapshot preservation in Chinese', async () => {
    await i18n.changeLanguage('zh-CN')
    setup({ sources: [source({ enabled: false, active_snapshot_id: 'snapshot-good', last_error: 'Catalog fetch failed' })] })
    const card = await screen.findByRole('region', { name: 'models.dev 参考信息' })
    expect(within(card).getByText('订阅已禁用')).toBeInTheDocument()
    expect(within(card).getByRole('button', { name: '启用并同步' })).toBeEnabled()
    expect(within(card).getByText('禁用后会停止自动获取，并将此来源从生效目录中排除。已保存的快照和已配置的项目模型仍会保留。')).toBeInTheDocument()
    expect(within(card).getByText('上次成功同步的快照已保存；订阅禁用期间不会纳入生效目录。')).toBeInTheDocument()
    expect(within(card).queryByText('上次成功同步的信息仍然生效。')).not.toBeInTheDocument()
  })

  it('keeps subscription table status text at the operational font size, not the tiny badge default', async () => {
    setup({ sources: [source()] })
    const status = await screen.findByText('Enabled')
    expect(status.closest('.mantine-Badge-root')).toHaveAttribute('style', expect.stringContaining('font-size: var(--mantine-font-size-md)'))
  })

  it('localizes the opt-in, status and reference-only explanation in Chinese', async () => {
    await i18n.changeLanguage('zh-CN')
    setup()
    const card = await screen.findByRole('region', { name: 'models.dev 参考信息' })
    expect(within(card).getByRole('button', { name: '连接 models.dev' })).toBeInTheDocument()
    expect(within(card).getByText(/不会启用提供商、更改路由或更新实际计费价格/)).toBeInTheDocument()
    expect(within(card).getByText('未连接')).toBeInTheDocument()
  })

  it('makes automatic information discoverable in the catalog', async () => {
    setup({ path: '/models?tab=catalog' })
    await userEvent.click(await screen.findByRole('button', { name: 'Automatic model info' }))
    expect(await screen.findByRole('button', { name: 'Connect models.dev' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Subscriptions' })).toHaveAttribute('aria-selected', 'true')
  })

  it('does not expose the subscription shortcut or writes to project-only managers', async () => {
    const { writes } = setup({ path: '/models?tab=catalog', permissions: ['project:manage'] })
    await screen.findByRole('heading', { level: 1, name: 'Model catalog' })
    expect(screen.queryByRole('button', { name: 'Automatic model info' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Connect models.dev' })).not.toBeInTheDocument()
    expect(screen.queryByRole('tab', { name: 'Subscriptions' })).not.toBeInTheDocument()
    expect(writes()).toHaveLength(0)
  })

  it('keeps project-only direct links away from subscriptions', async () => {
    const { fetchMock } = setup({ permissions: ['project:manage'] })
    await screen.findByRole('tab', { name: 'Models & routes', selected: true })
    expect(screen.queryByRole('region', { name: 'models.dev reference metadata' })).not.toBeInTheDocument()
    expect(fetchMock.mock.calls.some(([path]) => String(path).includes('/catalog/sources'))).toBe(false)
  })

  it('lets catalog-only managers connect sources without exposing project mutations', async () => {
    const { fetchMock, writes } = setup({ path: '/models', permissions: ['catalog:manage'] })
    await screen.findByRole('heading', { level: 1, name: 'Model catalog' })
    expect(screen.queryByRole('tab', { name: 'Models & routes' })).not.toBeInTheDocument()
    expect(screen.queryByRole('tab', { name: 'Model routing' })).not.toBeInTheDocument()
    expect(fetchMock.mock.calls.some(([path]) => String(path).includes('/operations/models') || String(path).includes('/operations/channels'))).toBe(false)
    await userEvent.click(screen.getByRole('button', { name: 'Automatic model info' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Connect models.dev' }))
    await waitFor(() => expect(writes()).toHaveLength(2))
  })

  it('requires a successful list lookup before offering opt-in after an initial loading error', async () => {
    let failed = true
    const { writes } = setup({ handle: (path, init) => {
      if (path === endpoint && !init?.method && failed) return apiError('Subscriptions unavailable', 503)
    } })
    const error = await screen.findByRole('alert')
    expect(screen.queryByRole('button', { name: 'Connect models.dev' })).not.toBeInTheDocument()
    failed = false
    await userEvent.click(within(error).getByRole('button', { name: 'Retry' }))
    expect(await screen.findByRole('button', { name: 'Connect models.dev' })).toBeEnabled()
    expect(writes()).toHaveLength(0)
  })

  it('removes an already-open catalog import form when catalog management permission is lost', async () => {
    const { client, writes } = setup({ path: '/models?tab=catalog' })
    await userEvent.click(await screen.findByRole('button', { name: 'Import' }))
    expect(screen.getByRole('button', { name: 'Validate and import' })).toBeInTheDocument()
    await act(async () => { client.setQueryData(['project-permissions', 'p1'], ['project:manage']) })
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Import' })).not.toBeInTheDocument())
    expect(screen.queryByRole('button', { name: 'Validate and import' })).not.toBeInTheDocument()
    expect(writes()).toHaveLength(0)
  })

  it('shows pending feedback, prevents double creation and leaves new keyboard actions motion-free', async () => {
    let finish: (response: Response) => void = () => {}
    const created = new Promise<Response>((resolve) => { finish = resolve })
    const { writes } = setup({ handle: (path, init) => {
      if (path === endpoint && init?.method === 'POST') return created
    } })
    const connect = await screen.findByRole('button', { name: 'Connect models.dev' })
    connect.focus()
    await userEvent.keyboard('{Enter}')
    expect(await screen.findByText('Syncing reference metadata…')).toBeInTheDocument()
    expect(connect).toBeDisabled()
    expect(connect).toHaveStyle({ transform: 'none', transition: 'none' })
    await userEvent.keyboard('{Enter}')
    expect(writes()).toHaveLength(1)
    finish(new Response(JSON.stringify(source()), { status: 201, headers: { 'Content-Type': 'application/json' } }))
    await waitFor(() => expect(writes()).toHaveLength(2))
    expect(writes()[1][0]).toBe(`${endpoint}/modelsdev-1/refresh`)
  })

  it('reports unchanged metadata without claiming a new snapshot', async () => {
    setup({ sources: [source({ active_snapshot_id: 'snapshot-good' })], handle: (path) => {
      if (path.endsWith('/refresh')) return json({ source_id: 'modelsdev-1', status: 'not_modified', snapshot_id: 'snapshot-good' })
    } })
    await userEvent.click(await screen.findByRole('button', { name: 'Sync now' }))
    expect(await screen.findByText('The catalog did not change (not modified).')).toBeInTheDocument()
  })

  it('prevents duplicate connection attempts after switching away and back during creation', async () => {
    let finish: (response: Response) => void = () => {}
    const creating = new Promise<Response>((resolve) => { finish = resolve })
    const { writes } = setup({ handle: (path, init) => {
      if (path === endpoint && init?.method === 'POST') return creating
    } })
    await userEvent.click(await screen.findByRole('button', { name: 'Connect models.dev' }))
    await waitFor(() => expect(writes()).toHaveLength(1))
    await userEvent.click(screen.getByRole('tab', { name: 'Model catalog' }))
    await userEvent.click(screen.getByRole('tab', { name: 'Subscriptions' }))
    const connect = await screen.findByRole('button', { name: 'Connect models.dev' })
    expect(connect).toBeDisabled()
    await userEvent.click(connect)
    expect(writes()).toHaveLength(1)
    finish(new Response(JSON.stringify(source()), { status: 201, headers: { 'Content-Type': 'application/json' } }))
    await waitFor(() => expect(writes()).toHaveLength(2))
  })

  it('does not silently enable a source disabled by another client just before sync', async () => {
    let reads = 0
    const { writes } = setup({ sources: [source()], handle: (path, init) => {
      if (path === endpoint && !init?.method) return json([source({ enabled: ++reads === 1 })])
    } })
    await userEvent.click(await screen.findByRole('button', { name: 'Sync now' }))
    const card = screen.getByRole('region', { name: 'models.dev reference metadata' })
    expect(await within(card).findByRole('alert')).toHaveTextContent('The subscription is disabled.')
    expect(writes()).toHaveLength(0)
    expect(await within(card).findByRole('button', { name: 'Enable and sync' })).toBeEnabled()
  })

  it('treats a failed refresh outcome as a failure instead of a success', async () => {
    setup({ sources: [source()], handle: (path) => {
      if (path.endsWith('/refresh')) return json({ source_id: 'modelsdev-1', status: 'failed', snapshot_id: null, error: 'Catalog validation failed' })
    } })
    await userEvent.click(await screen.findByRole('button', { name: 'Sync now' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Catalog validation failed')
    expect(screen.queryByText('Reference metadata synced. Providers, routes and live billing prices are unchanged.')).not.toBeInTheDocument()
  })
})
