import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import i18n from '../i18n'
import { ProjectProvider, useProject } from '../project'
import AccessPage from './AccessPage'
import { ConfirmHost } from '../components'

/**
 * The reference selects rows in its key table too, and the report's row 1 names
 * "the channels, models and API-key tables". The keys table is hand-built rather
 * than a `ResourcePage`, so it needed its own selection; `bulk-toggle` accepts
 * `keys` now, scoped to the project, which is what makes the action possible.
 *
 * The bulk action is a mutation of the list this panel renders, so the test holds
 * the mock's server-side key state: the POST changes it and the list must be read
 * again, because the row the operator sees is the state admission enforces.
 */
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const key = (overrides: Record<string, unknown>) => ({ id: 'k', name: 'key', key_prefix: 'pk', key_type: 'service', profile_id: null, allowed_ips_json: null, denied_ips_json: null, budget_micros: 5_000_000, spent_micros: 0, expires_at: null, last_used_at: null, enabled: true, lifecycle: 'active', archived_at: null, ...overrides })
const keys = [key({ id: 'k1', name: 'ci-runner' }), key({ id: 'k2', name: 'dashboard' })]
const keysPath = '/api/admin/v1/projects/p1/api-keys'

const mockApi = () => {
  let stored = keys.map((row) => ({ ...row }))
  return vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['*'])
    if (path.includes('bulk-toggle')) {
      // The route answers with the number of rows it changed, and the change is durable.
      const body = JSON.parse(String(init?.body)) as { ids: string[]; enabled: boolean }
      stored = stored.map((row) => (body.ids.includes(row.id) ? { ...row, enabled: body.enabled } : row))
      return json({ updated: body.ids.length })
    }
    if (path.includes('bulk-archive')) {
      const body = JSON.parse(String(init?.body)) as { ids: string[] }
      stored = stored.map((row) => (body.ids.includes(row.id) ? { ...row, enabled: false, lifecycle: 'archived' } : row))
      return json({ archived_ids: body.ids, archived_count: body.ids.length })
    }
    if (path.endsWith('/rotate')) return json({ key: stored.find((row) => path.includes(`/${row.id}/`)), token: 'pg_rotated_once' })
    if (path.endsWith('/archive')) {
      const id = path.split('/').at(-2)
      stored = stored.map((row) => row.id === id ? { ...row, enabled: false, lifecycle: 'archived' } : row)
      return json(stored.find((row) => row.id === id))
    }
    if (path.includes('key-profiles')) return json({ data: [], total: 0 })
    if (path.includes('api-keys')) return json(stored)
    return json([])
  })
}

const renderPage = (fetchMock: ReturnType<typeof vi.fn>) => {
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /><ConfirmHost /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

const rowFor = (name: string) => (screen.getByText(name).closest('tr') as HTMLElement)
const listReads = (fetchMock: ReturnType<typeof vi.fn>) => fetchMock.mock.calls.filter(([path]) => String(path).endsWith(keysPath)).length

/** The shell's project switcher; the harness drives the same provider state. */
function ProjectSwitch() {
  const { project: active, projects, setProjectId } = useProject()
  return (
    <div>
      <span data-testid="active-project">{active.id}</span>
      {projects.map((candidate) => <button key={candidate.id} type="button" onClick={() => setProjectId(candidate.id)}>{candidate.name}</button>)}
    </div>
  )
}

describe('api keys can be selected and acted on together', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('sends the selected keys to the bulk endpoint', async () => {
    const fetchMock = mockApi()
    renderPage(fetchMock)

    await userEvent.click(await screen.findByRole('checkbox', { name: 'Select ci-runner' }))
    await userEvent.click(screen.getByRole('checkbox', { name: 'Select dashboard' }))
    expect(screen.getByText('2 selected')).toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: 'Disable' }))

    await waitFor(() => {
      const call = fetchMock.mock.calls.find(([input, init]) => String(input).includes('bulk-toggle') && (init as RequestInit | undefined)?.method === 'POST')
      expect(call).toBeTruthy()
      expect(JSON.parse(String((call![1] as RequestInit).body))).toEqual({ resource: 'keys', ids: ['k1', 'k2'], enabled: false })
    })
  })

  it('selects every key from the header', async () => {
    renderPage(mockApi())
    await userEvent.click(await screen.findByRole('checkbox', { name: 'Select all' }))
    expect(screen.getByText('2 selected')).toBeInTheDocument()
    // The container names itself, so the table is reachable as a region like every
    // other list in the console.
    const region = screen.getAllByRole('region').find((element) => element.getAttribute('aria-label') === 'API keys')
    expect(region).toBeTruthy()
    expect(within(region as HTMLElement).getAllByRole('checkbox')).toHaveLength(3)
  })

  it('re-reads the project’s key list after a bulk disable and shows the recorded state', async () => {
    const fetchMock = mockApi()
    renderPage(fetchMock)
    await userEvent.click(await screen.findByRole('checkbox', { name: 'Select ci-runner' }))
    await userEvent.click(screen.getByRole('checkbox', { name: 'Select dashboard' }))
    // Both keys start usable, which is the state the console reads from `enabled`.
    expect(within(rowFor('ci-runner')).getByText('Usable')).toBeInTheDocument()
    const before = listReads(fetchMock)

    await userEvent.click(screen.getByRole('button', { name: 'Disable' }))

    // The list is read again — the cached list was the pre-action state — and the rows
    // now read the state the server recorded.
    await waitFor(() => expect(listReads(fetchMock)).toBeGreaterThan(before))
    await waitFor(() => {
      expect(within(rowFor('ci-runner')).getByText('Disabled')).toBeInTheDocument()
      expect(within(rowFor('dashboard')).getByText('Disabled')).toBeInTheDocument()
    })
    // The per-row action now offers the opposite transition.
    expect(screen.getByRole('button', { name: 'Enable ci-runner' })).toBeInTheDocument()
    // Selection is cleared only because the mutation was accepted, and the bar is gone.
    expect(screen.queryByText('2 selected')).not.toBeInTheDocument()
    // One click, one result — and here it is the success the operator can trust, because
    // the list it names was re-read before "Saved" was said.
    expect(vi.mocked(toast.success)).toHaveBeenCalledWith('Saved')
    expect(vi.mocked(toast.error)).not.toHaveBeenCalled()
  })

  it('keeps the selection and reports the error when the bulk action fails', async () => {
    const fetchMock = mockApi()
    const failing = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input).includes('bulk-toggle')) return Promise.resolve(new Response(JSON.stringify({ error: { type: 'permission_error', message: 'permission denied' } }), { status: 403, headers: { 'Content-Type': 'application/json' } }))
      return fetchMock(input, init)
    })
    renderPage(failing)
    await userEvent.click(await screen.findByRole('checkbox', { name: 'Select ci-runner' }))
    await userEvent.click(screen.getByRole('button', { name: 'Disable' }))

    await waitFor(() => expect(vi.mocked(toast.error)).toHaveBeenCalledWith('permission denied'))
    // A rejected action must not look like it landed: the selection survives so the
    // operator can retry, and the rows still show the enabled state.
    expect(screen.getByText('1 selected')).toBeInTheDocument()
    expect(within(rowFor('ci-runner')).getByText('Usable')).toBeInTheDocument()
  })

  it('reports one result when the action landed but the list could not be re-read', async () => {
    // The action is accepted, but the list it changed cannot be re-read. One click must
    // produce one result: the write landed durably, so the write's own success is not
    // contradicted, and the list that could not be confirmed is named as exactly that
    // instead of the internal cause behind it. The selection stays (those are the rows
    // that changed) and the list keeps its own error/retry state.
    let stored = keys.map((row) => ({ ...row }))
    let reads = 0
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('bulk-toggle')) {
        const body = JSON.parse(String(init?.body)) as { ids: string[]; enabled: boolean }
        stored = stored.map((row) => (body.ids.includes(row.id) ? { ...row, enabled: body.enabled } : row))
        return json({ updated: body.ids.length })
      }
      if (path.includes('key-profiles')) return json({ data: [], total: 0 })
      if (path.includes('api-keys')) {
        reads += 1
        // The first read is the list as loaded; the read that follows the action fails,
        // and the retry after it answers with the state the action recorded.
        return reads === 2
          ? Promise.resolve(new Response(JSON.stringify({ error: { type: 'internal_error', message: 'An internal error occurred' } }), { status: 500, headers: { 'Content-Type': 'application/json' } }))
          : json(stored)
      }
      return json([])
    })
    renderPage(fetchMock)
    await userEvent.click(await screen.findByRole('checkbox', { name: 'Select ci-runner' }))
    await userEvent.click(screen.getByRole('button', { name: 'Disable' }))

    // The list reports its own unreadability where the list is.
    expect(await screen.findByRole('alert')).toHaveTextContent('The request failed. Try again shortly.')

    // …and the action reports the single combined outcome: no success claim over rows
    // nobody re-read, no raw internal error, one message naming both facts. The old
    // behaviour said "Saved" and then the raw internal error — two contradictory
    // outcomes for one click.
    await waitFor(() => expect(vi.mocked(toast.error)).toHaveBeenCalled())
    expect({ success: vi.mocked(toast.success).mock.calls, error: vi.mocked(toast.error).mock.calls })
      .toEqual({ success: [], error: [['Saved, but the list could not be refreshed. Retry the list.']] })

    // The selection is not cleared over rows that were never re-read.
    expect(screen.getByText('1 selected')).toBeInTheDocument()

    // The list's own retry recovers the rows the action changed.
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await waitFor(() => expect(within(rowFor('ci-runner')).getByText('Disabled')).toBeInTheDocument())
  })

  it('does not carry a selection of the previous project’s keys across a switch', async () => {
    // The selection names keys of the project it was made in, and the panel is kept
    // mounted across a project switch, so it must not stand over another project's rows.
    const otherProject = { id: 'p2', name: 'Project B', slug: 'project-b', owner_user_id: 'u1', is_default: false, enabled: true }
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project, otherProject])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('key-profiles')) return json({ data: [], total: 0 })
      if (path.includes('api-keys')) return json(path.includes('/projects/p2/') ? [key({ id: 'k9', name: 'p2-key' })] : keys)
      return json([])
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /><ProjectSwitch /></ProjectProvider></MemoryRouter></QueryClientProvider>)

    await userEvent.click(await screen.findByRole('checkbox', { name: 'Select ci-runner' }))
    expect(screen.getByText('1 selected')).toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: 'Project B' }))

    // The other project's keys are on screen, and nothing is selected or offered.
    expect(await screen.findByText('p2-key')).toBeInTheDocument()
    expect(screen.queryByText('ci-runner')).not.toBeInTheDocument()
    expect(screen.queryByText('1 selected')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Disable' })).not.toBeInTheDocument()
  })

  it('confirms rotate, shows its secret once, and gives archived rows no mutable actions', async () => {
    const fetchMock = mockApi()
    renderPage(fetchMock)
    await screen.findByRole('button', { name: 'Rotate ci-runner' })
    const row = rowFor('ci-runner')
    await userEvent.click(within(row).getByRole('button', { name: 'Rotate ci-runner' }))
    expect(await screen.findByRole('dialog')).toHaveTextContent('Rotate API key “ci-runner”?')
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Rotate' }))
    expect(await screen.findByText('pg_rotated_once')).toBeInTheDocument()
    expect(within(screen.getByRole('dialog')).getByText(/ci-runner/)).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(screen.queryByText('pg_rotated_once')).not.toBeInTheDocument()

    await userEvent.click(within(rowFor('ci-runner')).getByRole('button', { name: 'Archive ci-runner' }))
    expect(await screen.findByRole('dialog')).toHaveTextContent('Archive API key “ci-runner”?')
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Archive' }))
    await waitFor(() => expect(within(rowFor('ci-runner')).getByText('Archived')).toBeInTheDocument())
    expect(within(rowFor('ci-runner')).queryByRole('button', { name: /Enable|Edit|Rotate|Archive/ })).not.toBeInTheDocument()
  })

  it('bulk archives selected keys through one confirmed truthful refresh', async () => {
    const fetchMock = mockApi()
    renderPage(fetchMock)
    await userEvent.click(await screen.findByRole('checkbox', { name: 'Select ci-runner' }))
    await userEvent.click(screen.getByRole('checkbox', { name: 'Select dashboard' }))
    await userEvent.click(screen.getByRole('button', { name: 'Archive' }))
    expect(await screen.findByRole('dialog')).toHaveTextContent('Archive 2 API keys?')
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Archive' }))
    await waitFor(() => expect(within(rowFor('ci-runner')).getByText('Archived')).toBeInTheDocument())
    expect(vi.mocked(toast.success)).toHaveBeenCalledTimes(1)
  })

  it('keeps a confirmed rotate attributed to the project and key where it started', async () => {
    const otherProject = { id: 'p2', name: 'Project B', slug: 'project-b', owner_user_id: 'u1', is_default: false, enabled: true }
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project, otherProject])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('key-profiles')) return json({ data: [], total: 0 })
      if (path.endsWith('/rotate')) return json({ key: key({ id: 'k1', name: 'ci-runner' }), token: 'pg_p1_rotated' })
      if (path.includes('api-keys')) return json(path.includes('/projects/p2/') ? [key({ id: 'k9', name: 'p2-key' })] : keys)
      return json([])
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /><ProjectSwitch /><ConfirmHost /></ProjectProvider></MemoryRouter></QueryClientProvider>)

    await userEvent.click(await screen.findByRole('button', { name: 'Rotate ci-runner' }))
    await userEvent.click(screen.getByRole('button', { name: 'Project B' }))
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Rotate' }))

    await screen.findByText('pg_p1_rotated')
    const call = fetchMock.mock.calls.find(([input]) => String(input).endsWith('/projects/p1/api-keys/k1/rotate'))
    expect(call).toBeTruthy()
    expect(within(screen.getByRole('dialog')).getByText(/ci-runner/)).toBeInTheDocument()
    expect(within(screen.getByRole('dialog')).getByText(/Project A/)).toBeInTheDocument()
  })
})
