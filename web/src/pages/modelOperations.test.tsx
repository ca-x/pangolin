import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfirmHost } from '../components'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import ModelsPage from './ModelsPage'

const response = (body: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }))
const paged = (data: unknown[]) => response({ data, total: data.length })
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const channel = { id: 'channel-1', name: 'Primary' }
const active = { id: 'model-active', provider_id: channel.id, provider_name: channel.name, public_name: 'active-model', upstream_name: 'active-upstream', capabilities: ['chat'], enabled: true, lifecycle: 'active' }
const archived = { ...active, id: 'model-archived', public_name: 'archived-model', lifecycle: 'archived' }
const card = { id: 'acme/chat-v2', upstream_id: 'acme-chat-v2', name: 'Acme Chat V2', developer: 'acme', type: 'chat' }

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><ModelsPage /><ConfirmHost /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

function common(path: string) {
  if (path.endsWith('/projects')) return response([project])
  if (path.includes('/permissions')) return response(['*'])
  if (path.includes('/operations/channels')) return paged([channel])
  if (path.includes('/catalog/models')) return paged([card])
  if (path.includes('/api-keys')) return response([])
  return null
}

describe('B40-B42 model operations', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('edits a service group ratio and assigned channel through the audited resource write', async () => {
    let saved: Record<string, unknown> | undefined
    const group = { id: 'group-1', name: 'Priority traffic', tier: 'priority', ratio_millionths: 1_250_000, channels: [channel.id], enabled: true }
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      const shared = common(path)
      if (shared) return shared
      if (path.includes('/operations/groups') && init?.method === 'POST') { saved = JSON.parse(String(init.body)); return response({ id: group.id }) }
      if (path.includes('/operations/groups')) return paged([group])
      if (path.includes('/operations/models')) return paged([active])
      return paged([])
    }))
    renderPage()

    await userEvent.click(await screen.findByRole('tab', { name: 'Service groups' }))
    const table = await screen.findByRole('region', { name: 'Service groups' })
    await userEvent.click(within(table).getByRole('button', { name: 'Edit' }))
    const dialog = screen.getByRole('dialog', { name: 'Edit Service groups' })
    expect(within(dialog).getByRole('textbox', { name: 'Per-request cost ratio' })).toHaveValue('1.25')
    expect(within(dialog).getByRole('checkbox', { name: 'Primary' })).toBeChecked()
    fireEvent.change(within(dialog).getByRole('textbox', { name: 'Per-request cost ratio' }), { target: { value: '1.5' } })
    await userEvent.click(within(dialog).getByRole('checkbox', { name: 'Primary' }))
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saved).toEqual(expect.objectContaining({ id: group.id, ratio: 1.5, channels: [] })))
  })

  it('imports selected catalog cards in one batch request', async () => {
    let batch: Record<string, unknown> | undefined
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      const shared = common(path)
      if (shared) return shared
      if (path.endsWith('/models/batch') && init?.method === 'POST') { batch = JSON.parse(String(init.body)); return response({ created_ids: ['created'], created_count: 1 }) }
      if (path.includes('/operations/models')) return paged([active])
      return paged([])
    }))
    renderPage()

    await userEvent.click(await screen.findByRole('button', { name: 'Import catalog models' }))
    const dialog = screen.getByRole('dialog', { name: 'Import catalog models' })
    await userEvent.click(within(dialog).getByRole('checkbox', { name: /Acme Chat V2/ }))
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Target channel' }))
    await userEvent.click(screen.getByRole('option', { name: 'Primary' }))
    await userEvent.click(within(dialog).getByRole('button', { name: 'Import selected (1)' }))

    await waitFor(() => expect(batch).toEqual({ models: [{ catalog_model_id: card.id, provider_id: channel.id, public_name: card.name, upstream_name: card.upstream_id }] }))
  })

  it('uses the safe bulk endpoint for active archive and archived delete', async () => {
    const bulk: unknown[] = []
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      const shared = common(path)
      if (shared) return shared
      if (path.endsWith('/models/bulk') && init?.method === 'POST') { bulk.push(JSON.parse(String(init.body))); return response({ changed_count: 1 }) }
      if (path.includes('/operations/models')) return path.includes('lifecycle=archived') ? paged([archived]) : paged([active])
      return paged([])
    }))
    renderPage()

    await screen.findByRole('checkbox', { name: 'Select active-model' })
    await userEvent.click(screen.getByRole('checkbox', { name: 'Select all' }))
    const activeBulk = await screen.findByRole('region', { name: 'Bulk actions' })
    await userEvent.click(within(activeBulk).getByRole('button', { name: 'Archive' }))
    await userEvent.click(within(await screen.findByRole('dialog', { name: 'Archive 1 models?' })).getByRole('button', { name: 'Archive' }))
    await waitFor(() => expect(bulk).toContainEqual({ ids: [active.id], action: 'archive' }))

    await userEvent.click(screen.getByRole('combobox', { name: 'Lifecycle' }))
    await userEvent.click(screen.getByRole('option', { name: 'Archived' }))
    await screen.findByRole('checkbox', { name: 'Select archived-model' })
    await userEvent.click(screen.getByRole('checkbox', { name: 'Select all' }))
    const archivedBulk = await screen.findByRole('region', { name: 'Bulk actions' })
    await userEvent.click(within(archivedBulk).getByRole('button', { name: 'Delete' }))
    await userEvent.click(within(await screen.findByRole('dialog', { name: 'Delete 1 models?' })).getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(bulk).toContainEqual({ ids: [archived.id], action: 'delete' }))
  }, 10_000)

  it('names unassociated models, links to the editor, and states the empty result', async () => {
    let unassociated = [{ id: 'loose', public_name: 'loose-model', provider_id: channel.id, provider_name: channel.name }]
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      const shared = common(path)
      if (shared) return shared
      if (path.includes('/models/unassociated')) return paged(unassociated)
      if (path.includes('/operations/models')) return paged([active])
      return paged([])
    }))
    const view = renderPage()

    await userEvent.click(await screen.findByRole('tab', { name: 'Model routing' }))
    expect(await screen.findByText('loose-model')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Edit associations' })).toHaveAttribute('href', '#association-editor')

    view.unmount()
    unassociated = []
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Model routing' }))
    expect(await screen.findByText('Every enabled model has at least one enabled matching association.')).toBeInTheDocument()
  })
})
