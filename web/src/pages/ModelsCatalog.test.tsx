import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import i18n from '../i18n'
import { ConfirmHost } from '../components'
import { ProjectProvider } from '../project'
import ModelsPage, { nextDueAt } from './ModelsPage'

/**
 * Catalog sync status is reported from what the API returns: the last attempt,
 * the last success, when the next refresh is due, and which snapshot is active
 * with its own signature verdict. A refresh reports the server's outcome, and a
 * batch refresh is available for everything that is due.
 */
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
/** The API's error envelope: `{ error: { type, message } }` with the status the backend maps the failure to. */
const apiError = (status: number, type: string, message: string) => json({ error: { type, message } }, status)
const paged = (rows: unknown[]) => json({ data: rows, total: rows.length, offset: 0, limit: 25 })
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const now = Math.floor(Date.now() / 1000)

const source = (overrides: Record<string, unknown> = {}) => ({
  id: 's1',
  name: 'acme',
  url: 'https://catalog.example/catalog.json',
  priority: 100,
  refresh_interval_secs: 3600,
  enabled: true,
  signature_policy: 'required',
  public_key: null,
  revision: 4,
  last_attempt_at: now - 600,
  last_success_at: now - 900,
  last_error: null,
  active_snapshot_id: 'snap-2',
  previous_snapshot_id: 'snap-1',
  ...overrides,
})

const snapshots = [
  { id: 'snap-2', version: '7', digest: 'abcdef1234567890', source_url: 'https://catalog.example/catalog.json', signature_verified: true, created_at: 1_700_000_000 },
  { id: 'snap-1', version: '6', digest: '0123456789abcdef', source_url: 'https://catalog.example/catalog.json', signature_verified: false, created_at: 1_699_000_000 },
]

const renderPage = () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ProjectProvider><ModelsPage /><ConfirmHost /></ProjectProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

const base = (path: string, init?: RequestInit) => {
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('/catalog/sources/refresh-due') && init?.method === 'POST') return json([])
  if (path.includes('/snapshots')) return json(snapshots)
  if (path.includes('/catalog/sources')) return json([source()])
  return paged([])
}

describe('catalog sync status', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en')
    vi.clearAllMocks()
  })

  it('renders the last attempt, the last success and the next due time', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => base(String(input), init)))
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Subscriptions' }))

    expect(await screen.findByText('Last attempt')).toBeInTheDocument()
    expect(screen.getByText('Last success')).toBeInTheDocument()
    expect(screen.getByText('Next due')).toBeInTheDocument()

    const formatted = (seconds: number) => new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(seconds * 1000))
    expect(screen.getByText(formatted(now - 600))).toBeInTheDocument()
    expect(screen.getByText(formatted(now - 900))).toBeInTheDocument()
    // The next attempt follows the configured interval, not an invented value.
    expect(screen.getByText(formatted(now - 600 + 3600))).toBeInTheDocument()
  })

  it('marks the active snapshot with its time and signature verdict', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => base(String(input), init)))
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Subscriptions' }))

    expect(await screen.findByText('7')).toBeInTheDocument()
    expect(screen.getByText('Signature verified')).toBeInTheDocument()
    const formatted = new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(1_700_000_000 * 1000))
    expect(screen.getByText(`Created ${formatted}`)).toBeInTheDocument()

    // The history view names the active snapshot and refuses to roll back to it.
    await userEvent.click(screen.getByRole('button', { name: 'Rollback acme' }))
    const history = await screen.findByRole('dialog', { name: 'Rollback' })
    expect(within(history).getByText(/7 · abcdef123456/)).toBeInTheDocument()
    expect(within(history).getByText(/Active/)).toBeInTheDocument()
    expect(within(history).getByText(/No signature/)).toBeInTheDocument()
    const rollbackButtons = within(history).getAllByRole('button', { name: 'Rollback' })
    expect(rollbackButtons[0]).toBeDisabled()
    expect(rollbackButtons[1]).toBeEnabled()
  })

  it('shows a due-now state for a source that has never been attempted', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.includes('/catalog/sources') && !path.includes('/snapshots')) return json([source({ last_attempt_at: null, last_success_at: null, active_snapshot_id: null, enabled: true })])
      return base(path, init)
    }))
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Subscriptions' }))
    expect(await screen.findByText('Due now')).toBeInTheDocument()
    // A disabled source is never due, and the two '—' cells are the two unmeasured times.
    expect(nextDueAt({ ...source({ enabled: false }) }, now)).toEqual({ dueAt: null, dueNow: false })
  })

  it('reports the outcome the server recorded for a single refresh', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.includes('/catalog/sources/s1/refresh')) return json({ source_id: 's1', status: 'activated', snapshot_id: 'snap-987654321' })
      return base(path, init)
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Subscriptions' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Refresh acme' }))

    await waitFor(() => expect(vi.mocked(toast.success)).toHaveBeenCalledWith('Activated snapshot snap-9876543'))
    expect(vi.mocked(toast.error)).not.toHaveBeenCalled()
  })

  it('reports an unchanged catalog instead of claiming a new snapshot', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.includes('/catalog/sources/s1/refresh')) return json({ source_id: 's1', status: 'not_modified', snapshot_id: 'snap-2' })
      return base(path, init)
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Subscriptions' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Refresh acme' }))
    await waitFor(() => expect(vi.mocked(toast.success)).toHaveBeenCalledWith('The catalog did not change (not modified).'))
  })

  it('refreshes every due source and summarizes the batch', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.includes('/catalog/sources/refresh-due')) {
        return json([
          { source_id: 's1', status: 'activated', snapshot_id: 'snap-3' },
          { source_id: 's2', status: 'not_modified', snapshot_id: 'snap-4' },
          { source_id: 's3', status: 'failed', error: 'catalog source returned HTTP 500' },
        ])
      }
      return base(path, init)
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Subscriptions' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Refresh all due' }))

    await waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('refresh-due'))).toHaveLength(1))
    // A batch with a failure is reported as an error, with the counts the server returned.
    expect(vi.mocked(toast.error)).toHaveBeenCalledWith('Refreshed 2 subscriptions (1 new snapshots), 1 failed.')
  })

  it('re-reads the source row after a failed refresh instead of keeping the stale state', async () => {
    // The server records the failed attempt (`last_attempt_at`, `last_error`) before it
    // answers with the error, so the row is the durable truth: a toast alone would leave
    // the screen showing a source that looks like it never tried.
    let stored = source()
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.includes('/catalog/sources/s1/refresh')) {
        stored = source({ last_attempt_at: now, last_error: 'catalog source returned HTTP 500' })
        return apiError(502, 'upstream_error', 'catalog source returned HTTP 500')
      }
      if (path.endsWith('/catalog/sources')) return json([stored])
      return base(path, init)
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Subscriptions' }))
    await screen.findByText('acme')
    const reads = () => fetchMock.mock.calls.filter(([path]) => String(path).endsWith('/catalog/sources')).length
    // Before the refresh the row carries no recorded error.
    expect(screen.queryByText('catalog source returned HTTP 500')).not.toBeInTheDocument()
    const before = reads()

    await userEvent.click(screen.getByRole('button', { name: 'Refresh acme' }))

    // The error is reported, and the row is re-read rather than left as it was.
    await waitFor(() => expect(vi.mocked(toast.error)).toHaveBeenCalledWith('catalog source returned HTTP 500'))
    await waitFor(() => expect(reads()).toBeGreaterThan(before))
    expect(await screen.findByText('catalog source returned HTTP 500')).toBeInTheDocument()
  })

  it('re-reads the sources after a failed batch refresh', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.includes('/catalog/sources/refresh-due')) return apiError(500, 'internal_error', 'An internal error occurred')
      return base(path, init)
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Subscriptions' }))
    await screen.findByText('acme')
    const reads = () => fetchMock.mock.calls.filter(([path]) => String(path).endsWith('/catalog/sources')).length
    const before = reads()

    await userEvent.click(screen.getByRole('button', { name: 'Refresh all due' }))

    await waitFor(() => expect(vi.mocked(toast.error)).toHaveBeenCalledWith('An internal error occurred'))
    // A batch that failed at the transport level says nothing about the rows it may have
    // touched, so the list is re-read and still shows what the server holds.
    await waitFor(() => expect(reads()).toBeGreaterThan(before))
    expect(screen.getByText('acme')).toBeInTheDocument()
  })

  it('says so when nothing is due', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => base(String(input), init))
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Subscriptions' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Refresh all due' }))
    await waitFor(() => expect(vi.mocked(toast.success)).toHaveBeenCalledWith('No subscription is due.'))
  })
})

describe('catalog model creation', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en')
    vi.clearAllMocks()
  })

  const card = {
    id: 'acme/stable-chat-id',
    upstream_id: 'acme-chat-v2',
    name: 'Acme Chat V2',
    developer: 'acme',
    type: 'chat',
    capabilities: { tools: true },
    cost_defaults: { input: 1, output: 2, currency: 'USD', unit: 'per_million_tokens' },
  }

  it('sends the selected stable card id through the ordinary model create request', async () => {
    let created: Record<string, unknown> | undefined
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.includes('/catalog/models')) return paged([card])
      if (path.includes('/operations/channels')) return paged([{ id: 'channel-1', name: 'Channel one' }])
      if (path.includes('/operations/models') && init?.method === 'POST') {
        created = JSON.parse(String(init.body))
        return json({ id: 'model-1' })
      }
      return base(path, init)
    }))
    renderPage()

    await userEvent.click(await screen.findByRole('button', { name: 'Add model' }))
    await userEvent.click(await screen.findByRole('combobox', { name: 'Catalog model' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Acme Chat V2' }))
    const dialog = screen.getByRole('dialog', { name: 'Add model' })
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Public model name' }), 'public-acme')
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Upstream model' }), 'acme-chat-v2')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(created).toBeDefined())
    expect(created?.catalog_model_id).toBe('acme/stable-chat-id')
  })

  it('keeps a useful manual fallback when the effective catalog is empty', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.includes('/catalog/models')) return paged([])
      if (path.includes('/operations/channels')) return paged([{ id: 'channel-1', name: 'Channel one' }])
      return base(path, init)
    }))
    renderPage()

    await userEvent.click(await screen.findByRole('button', { name: 'Add model' }))
    expect(await screen.findByText('No catalog cards are available. Enter the model manually.')).toBeInTheDocument()
    expect(screen.getByRole('combobox', { name: 'Catalog model' })).toHaveValue('Manual entry')
    expect(within(screen.getByRole('dialog', { name: 'Add model' })).getByRole('textbox', { name: 'Public model name' })).toBeEnabled()
  })

  it('shows a retryable catalog failure without blocking manual creation', async () => {
    let catalogReads = 0
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.includes('/catalog/models')) {
        catalogReads += 1
        return apiError(500, 'internal_error', 'An internal error occurred')
      }
      if (path.includes('/operations/channels')) return paged([{ id: 'channel-1', name: 'Channel one' }])
      return base(path, init)
    }))
    renderPage()

    await userEvent.click(await screen.findByRole('button', { name: 'Add model' }))
    expect(await screen.findByText('The model catalog could not be loaded.')).toBeInTheDocument()
    expect(screen.getByRole('combobox', { name: 'Catalog model' })).toHaveValue('Manual entry')
    expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled()
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await waitFor(() => expect(catalogReads).toBeGreaterThan(1))
  })
})
