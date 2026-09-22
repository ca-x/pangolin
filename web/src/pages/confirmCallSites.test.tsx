import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { toast } from 'sonner'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { ThemeProvider } from '../theme'
import { ConfirmHost } from '../components'
import AccessPage from './AccessPage'
import ChannelsPage from './ChannelsPage'
import ModelsPage from './ModelsPage'
import SystemPage from './SystemPage'

/**
 * The five call sites that used to ask through the synchronous browser
 * `confirm()`. Each one now opens the shared dialog, so the same three
 * properties are checked everywhere: the dialog names the object, backing out
 * leaves the server untouched, and confirming sends exactly one request to the
 * endpoint that was already there.
 */
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const paged = (rows: unknown[]) => json({ data: rows, total: rows.length, offset: 0, limit: 25 })
const renderPage = (page: React.ReactElement) => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <ThemeProvider>
        <MemoryRouter>
          <ProjectProvider>{page}<ConfirmHost /></ProjectProvider>
        </MemoryRouter>
      </ThemeProvider>
    </QueryClientProvider>,
  )
}
const calls = (fetchMock: ReturnType<typeof vi.fn>, method: string) => fetchMock.mock.calls.filter(([, init]) => (init as RequestInit | undefined)?.method === method)
const nativeConfirm = () => vi.fn(() => true)

describe('destructive actions ask through the shared dialog', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('names a channel row and the collection it is removed from', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'DELETE') return json({})
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      return paged([{ id: 'c1', name: 'openai-prod', kind: 'openai', base_url: 'https://api.openai.com', enabled: true }])
    })
    vi.stubGlobal('fetch', fetchMock)
    const confirm = nativeConfirm()
    vi.stubGlobal('confirm', confirm)

    renderPage(<ChannelsPage />)
    await userEvent.click(await screen.findByRole('button', { name: 'Delete openai-prod' }))

    const dialog = screen.getByRole('dialog', { name: 'Delete “openai-prod”?' })
    expect(within(dialog).getByText('This removes the entry from Channels and cannot be undone.')).toBeInTheDocument()
    expect(confirm).not.toHaveBeenCalled()

    await userEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    expect(calls(fetchMock, 'DELETE')).toHaveLength(0)
  })

  it('deletes the named channel once, and only after the confirmation', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'DELETE') return json({})
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      return paged([{ id: 'c1', name: 'openai-prod', kind: 'openai', base_url: 'https://api.openai.com', enabled: true }])
    })
    vi.stubGlobal('fetch', fetchMock)

    renderPage(<ChannelsPage />)
    await userEvent.click(await screen.findByRole('button', { name: 'Delete openai-prod' }))
    expect(calls(fetchMock, 'DELETE')).toHaveLength(0)

    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(calls(fetchMock, 'DELETE')).toHaveLength(1))
    expect(String(calls(fetchMock, 'DELETE')[0][0])).toBe('/api/admin/v1/projects/p1/operations/channels/c1')
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  })

  it('shows a refused delete inside the dialog instead of only in a toast', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'DELETE') return Promise.resolve(new Response(JSON.stringify({ error: { message: 'channel still has active credentials' } }), { status: 409, headers: { 'Content-Type': 'application/json' } }))
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      return paged([{ id: 'c1', name: 'openai-prod', kind: 'openai', base_url: 'https://api.openai.com', enabled: true }])
    })
    vi.stubGlobal('fetch', fetchMock)

    renderPage(<ChannelsPage />)
    await userEvent.click(await screen.findByRole('button', { name: 'Delete openai-prod' }))
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete' }))

    const dialog = await screen.findByRole('dialog', { name: 'Delete “openai-prod”?' })
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('channel still has active credentials')
    // The refusal is visible where the decision was made, and the row is still there.
    expect(within(dialog).getByRole('button', { name: 'Delete' })).not.toBeDisabled()
    expect(calls(fetchMock, 'DELETE')).toHaveLength(1)
    // The dialog is where the decision was made, so the refusal belongs there and
    // nowhere else: a toast said the same sentence a second time.
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('asks the same question from the mobile card list', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'DELETE') return json({})
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      return paged([{ id: 'c1', name: 'openai-prod', kind: 'openai', base_url: 'https://api.openai.com', enabled: true }])
    })
    vi.stubGlobal('fetch', fetchMock)

    renderPage(<ChannelsPage />)
    // The card list's delete control is labelled with the bare verb, the table's
    // carries the row name, so this targets the mobile affordance.
    await userEvent.click(await screen.findByRole('button', { name: 'Delete' }))
    expect(screen.getByRole('dialog', { name: 'Delete “openai-prod”?' })).toBeInTheDocument()
    await userEvent.keyboard('{Escape}')
    expect(calls(fetchMock, 'DELETE')).toHaveLength(0)
  })

  it('names the API key before archiving and revoking it', async () => {
    let archived = false
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'POST' && path.endsWith('/archive')) { archived = true; return json({}) }
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['api_key:manage'])
      if (path.includes('/api-keys')) return json([{ id: 'k1', name: 'staging-ci', key_prefix: 'pk_live_1', key_type: 'service', scopes: ['gateway:use'], budget_micros: null, spent_micros: 0, allowed_ips_json: '[]', denied_ips_json: '[]', enabled: !archived, lifecycle: archived ? 'archived' : 'active', archived_at: archived ? 1 : null, profile_id: null, expires_at: null, last_used_at: null }])
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)

    renderPage(<AccessPage />)
    await userEvent.click(await screen.findByRole('button', { name: 'Archive staging-ci' }))
    const dialog = screen.getByRole('dialog', { name: 'Archive API key “staging-ci”?' })
    expect(within(dialog).getByText('This archives the key in “Project A”, immediately revoking access. It cannot be edited, enabled, or rotated again.')).toBeInTheDocument()
    await userEvent.click(within(dialog).getByRole('button', { name: 'Archive' }))
    await waitFor(() => expect(calls(fetchMock, 'POST')).toHaveLength(1))
    expect(String(calls(fetchMock, 'POST')[0][0])).toBe('/api/admin/v1/projects/p1/api-keys/k1/archive')
  })

  it('names the invitation before revoking it', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'DELETE') return json({})
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['project:manage'])
      if (path.endsWith('/roles')) return json([])
      if (path.endsWith('/invitations')) return json([{ id: 'i1', email: 'invited@example.test', role_id: 'r1', expires_at: 1, accepted_at: null }])
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)

    renderPage(<AccessPage />)
    await userEvent.click(await screen.findByRole('tab', { name: 'Invitations' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Delete invited@example.test' }))
    const dialog = screen.getByRole('dialog', { name: 'Delete the invitation for “invited@example.test”?' })
    expect(within(dialog).getByText('The invitation link stops working immediately and cannot be undone.')).toBeInTheDocument()
    await userEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    expect(calls(fetchMock, 'DELETE')).toHaveLength(0)

    await userEvent.click(screen.getByRole('button', { name: 'Delete invited@example.test' }))
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(calls(fetchMock, 'DELETE')).toHaveLength(1))
    expect(String(calls(fetchMock, 'DELETE')[0][0])).toBe('/api/admin/v1/projects/p1/invitations/i1')
  })

  it('names a catalog override by its id and type', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'DELETE') return json({})
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)

    renderPage(<ModelsPage />)
    await userEvent.click(await screen.findByRole('tab', { name: 'Model catalog' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Local catalog overrides' }))
    // Required Mantine labels carry a marker span, so the label text is not an
    // exact match for the field name.
    await userEvent.type(await screen.findByLabelText(/^ID/), 'gpt-4o')
    // `userEvent.type` reads braces as key descriptors, and this is JSON.
    fireEvent.change(screen.getByLabelText(/Override entry JSON/), { target: { value: '{"id":"gpt-4o"}' } })
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }))

    const dialog = screen.getByRole('dialog', { name: 'Delete override “gpt-4o”?' })
    expect(within(dialog).getByText('The local Model override is removed and the catalog value applies again.')).toBeInTheDocument()
    await userEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    expect(calls(fetchMock, 'DELETE')).toHaveLength(0)
  })

  it('asks before rolling the active catalog back to a snapshot', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'POST') return json({})
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      // Snapshot ids are TEXT in the catalog schema, and the rollback body is checked below.
      if (path.includes('/snapshots')) return json([{ id: '7', version: '3', digest: 'abcdef1234567890', signature_verified: true, created_at: 1_700_000_000 }])
      if (path.endsWith('/catalog/sources')) return json([{ id: 's1', name: 'acme', url: 'https://catalog.example/catalog.json', priority: 100, refresh_interval_secs: 3600, enabled: true, signature_policy: 'required', public_key: null, revision: 4, last_success_at: null, last_error: null }])
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)

    renderPage(<ModelsPage />)
    await userEvent.click(await screen.findByRole('tab', { name: 'Subscriptions' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Rollback acme' }))
    // The history modal names each snapshot and marks the active one; a
    // non-active row is the one that offers the rollback action.
    await userEvent.click(await screen.findByRole('button', { name: 'Rollback' }))

    // The snapshot list stays open behind the confirmation, so the dialog is
    // addressed by its own name rather than by role alone — and it has to be
    // the one on top, or the question is asked behind another modal. Mantine
    // only assigns stacking z-index inside <ModalStack>, which this shell does
    // not mount, so the order of the portals is what keeps it in front: the
    // host is rendered after <App/> in main.tsx.
    const dialog = screen.getByRole('dialog', { name: 'Roll “acme” back to revision 3?' })
    const snapshotModal = screen.getByRole('dialog', { name: 'Rollback' })
    expect(snapshotModal.compareDocumentPosition(dialog) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(within(dialog).getByText('The active catalog is replaced by this snapshot and cannot be undone.')).toBeInTheDocument()
    expect(within(dialog).getByRole('button', { name: 'Rollback' })).toBeInTheDocument()

    await userEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('/rollback'))).toHaveLength(0)

    await userEvent.click(await screen.findByRole('button', { name: 'Rollback' }))
    const reopened = screen.getByRole('dialog', { name: 'Roll “acme” back to revision 3?' })
    await userEvent.click(within(reopened).getByRole('button', { name: 'Rollback' }))
    await waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('/rollback'))).toHaveLength(1))
    const [path, init] = fetchMock.mock.calls.find(([candidate]) => String(candidate).includes('/rollback')) as [string, RequestInit]
    expect(path).toBe('/api/admin/v1/catalog/sources/s1/rollback')
    expect(JSON.parse(String(init.body))).toEqual({ revision: 4, snapshot_id: '7' })
  })

  it('asks before restoring a backup over the project', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'POST') return json({ restored: 3 })
      if (path.includes('bootstrap')) return json({ initialized: true, authenticated: true, user: { id: 'u1', email: 'owner@example.test', role: 'admin', language: 'en', theme: 'system:bronze', created_at: 1 }, capture_payloads: false })
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)

    renderPage(<SystemPage />)
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))
    const artifact = { version: 1, id: 'artifact-1', project_id: 'p1', created_at: 1_700_000_000, digest: 'digest', resources: ['providers'], envelope: 'envelope' }
    fireEvent.change(await screen.findByLabelText(/Backup artifact file/), { target: { files: [new File([JSON.stringify(artifact)], 'backup.json', { type: 'application/json' })] } })
    // The file is read asynchronously, and Restore stays disabled until the
    // artifact is understood.
    await screen.findByText(/Artifact artifact-1/)
    await userEvent.click(screen.getByRole('button', { name: 'Restore' }))

    const dialog = screen.getByRole('dialog', { name: 'Restore a backup into “Project A”?' })
    expect(within(dialog).getByText('The artifact is written into this project with the fail conflict strategy and cannot be undone.')).toBeInTheDocument()
    await userEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('/backup/restore'))).toHaveLength(0)

    await userEvent.click(screen.getByRole('button', { name: 'Restore' }))
    const reopened = screen.getByRole('dialog', { name: 'Restore a backup into “Project A”?' })
    await userEvent.click(within(reopened).getByRole('button', { name: 'Restore' }))
    await waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('/backup/restore'))).toHaveLength(1))
    const [, init] = fetchMock.mock.calls.find(([path]) => String(path).includes('/backup/restore')) as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toEqual({ artifact, strategy: 'fail' })
  })
})
