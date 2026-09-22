import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ConfirmHost } from '../components'
import { ProjectProvider } from '../project'
import ModelsPage from './ModelsPage'

const response = (body: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }))
const paged = (data: unknown[]) => response({ data, total: data.length })
const project = { id: 'p1', name: 'P', slug: 'p', owner_user_id: 'u', is_default: true, enabled: true }
const active = { id: 'active-id', provider_id: 'channel-1', provider_name: 'Primary', public_name: 'active-model', upstream_name: 'upstream', capabilities: ['chat'], enabled: true, lifecycle: 'active' }
const archived = { ...active, id: 'archived-id', public_name: 'archived-model', lifecycle: 'archived' }

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><ModelsPage /><ConfirmHost /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

afterEach(() => vi.unstubAllGlobals())

describe('model archive lifecycle', () => {
  it('filters lifecycle rows and offers only legal named actions', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return response([project])
      if (path.includes('/permissions')) return response(['*'])
      if (path.includes('/operations/channels')) return paged([{ id: 'channel-1', name: 'Primary' }])
      if (path.includes('/catalog/models')) return paged([])
      if (path.includes('/operations/models')) return path.includes('lifecycle=archived') ? paged([archived]) : paged([active])
      if (path.includes('/lifecycle') && init?.method === 'POST') return response({ ok: true })
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()

    const archiveButtons = await screen.findAllByRole('button', { name: 'Archive active-model' })
    expect(archiveButtons).toHaveLength(2)
    expect(screen.queryByRole('button', { name: /Restore active-model/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Review delete impact active-model/ })).not.toBeInTheDocument()
    const table = screen.getByRole('region', { name: 'Models & routes' })
    fireEvent.click(within(table).getByRole('button', { name: 'Archive active-model' }))
    const archiveDialog = await screen.findByRole('dialog', { name: 'Archive model “active-model”?' })
    await userEvent.click(within(archiveDialog).getByRole('button', { name: 'Archive' }))
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/admin/v1/projects/p1/models/active-id/lifecycle', expect.objectContaining({ method: 'POST', body: JSON.stringify({ action: 'archive' }) })))

    await userEvent.click(screen.getByRole('combobox', { name: 'Lifecycle' }))
    await userEvent.click(screen.getByRole('option', { name: 'Archived' }))
    expect(await screen.findAllByRole('button', { name: 'Restore archived-model' })).toHaveLength(2)
    expect(screen.getAllByRole('button', { name: 'Review delete impact archived-model' })).toHaveLength(2)
    expect(screen.queryByRole('button', { name: /Archive archived-model/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Edit/ })).not.toBeInTheDocument()
    const archivedTable = screen.getByRole('region', { name: 'Models & routes' })
    fireEvent.click(within(archivedTable).getByRole('button', { name: 'Restore archived-model' }))
    const restoreDialog = await screen.findByRole('dialog', { name: 'Restore model “archived-model”?' })
    expect(within(restoreDialog).getByText('The model re-enters normal enabled-state and policy checks.')).toBeInTheDocument()
    await userEvent.click(within(restoreDialog).getByRole('button', { name: 'Cancel' }))
  }, 10_000)

  it('shows preview loading and the explicit empty dependency state before delete', async () => {
    let resolvePreview!: (value: Response) => void
    const preview = new Promise<Response>((resolve) => { resolvePreview = resolve })
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return response([project])
      if (path.includes('/permissions')) return response(['*'])
      if (path.includes('/operations/channels')) return paged([{ id: 'channel-1', name: 'Primary' }])
      if (path.includes('/catalog/models')) return paged([])
      if (path.includes('/operations/models')) return paged([archived])
      if (path.includes('/delete-impact')) return preview
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()

    await screen.findAllByRole('button', { name: 'Review delete impact archived-model' })
    fireEvent.click(within(screen.getByRole('region', { name: 'Models & routes' })).getByRole('button', { name: 'Review delete impact archived-model' }))
    const dialog = await screen.findByRole('dialog', { name: 'Delete model “archived-model”?' })
    expect(within(dialog).getByRole('status')).toHaveTextContent('Loading')
    resolvePreview(new Response(JSON.stringify({ id: archived.id, lifecycle: 'archived', prices: 0, price_components: 0, usage_history: 0, associations: 0, executions: 0, blocked: false }), { status: 200, headers: { 'content-type': 'application/json' } }))
    expect(await within(dialog).findByText('No dependent records')).toBeInTheDocument()
    expect(within(dialog).getByRole('button', { name: 'Delete' })).toBeEnabled()
  })

  it('renders preview failure, retries, and explains history-blocked counts', async () => {
    let previewCalls = 0
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return response([project])
      if (path.includes('/permissions')) return response(['*'])
      if (path.includes('/operations/channels')) return paged([{ id: 'channel-1', name: 'Primary' }])
      if (path.includes('/catalog/models')) return paged([])
      if (path.includes('/operations/models')) return paged([archived])
      if (path.includes('/delete-impact')) {
        previewCalls += 1
        return previewCalls === 1 ? Promise.reject(new Error('offline')) : response({ id: archived.id, lifecycle: 'archived', prices: 1, price_components: 2, usage_history: 3, associations: 4, executions: 5, blocked: true })
      }
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()

    await screen.findAllByRole('button', { name: 'Review delete impact archived-model' })
    fireEvent.click(within(screen.getByRole('region', { name: 'Models & routes' })).getByRole('button', { name: 'Review delete impact archived-model' }))
    const dialog = await screen.findByRole('dialog', { name: 'Delete model “archived-model”?' })
    expect(await within(dialog).findByText('The request failed. Try again shortly.')).toBeInTheDocument()
    await userEvent.click(within(dialog).getByRole('button', { name: 'Retry' }))
    expect(await within(dialog).findByText('Usage history')).toBeInTheDocument()
    expect(within(dialog).getByText('3')).toBeInTheDocument()
    expect(within(dialog).getByText('Immutable price or usage history prevents deletion.')).toBeInTheDocument()
    expect(within(dialog).getByRole('button', { name: 'Delete' })).toBeDisabled()
  })
})
