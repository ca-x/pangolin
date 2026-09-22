import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfirmHost } from '../components'
import i18n from '../i18n'
import { ProjectProvider, useProject } from '../project'
import AccessPage from './AccessPage'

/**
 * Project members are a per-project surface like roles and invitations, so the
 * console reaches them through the Access tabs and the project selector rather
 * than a second picker of its own. The columns are the fields
 * `access::MembershipView` returns; the role name and the ownership badge are
 * resolved from data the console already has (the project roles list and the
 * selected project's `owner_user_id`). The identity column reads the joined
 * `display_name` and `email`, which the gateway returns nullable, so a
 * membership whose user carries neither still shows its opaque id.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const jsonResponse = (value: unknown) => new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } })
const deferred = () => {
  let release: () => void = () => {}
  const gate = new Promise<void>((resolve) => { release = resolve })
  return { gate, release }
}
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u-owner', is_default: true, enabled: true }
const otherProject = { id: 'p2', name: 'Project B', slug: 'project-b', owner_user_id: 'u-other-owner', is_default: false, enabled: true }
const roles = [{ id: 'r-owner', name: 'Owner' }, { id: 'r-dev', name: 'Developer' }]
const members = [
  { id: 'm1', project_id: 'p1', user_id: 'u-owner', email: null, display_name: null, role_id: 'r-owner', status: 'active', created_at: 1_700_000_000, updated_at: 1_700_000_000 },
  { id: 'm2', project_id: 'p1', user_id: 'u-member', email: null, display_name: null, role_id: 'r-dev', status: 'suspended', created_at: 1_700_000_100, updated_at: 1_700_000_200 },
]
// A membership row the backend has not filled in: an absent role, status or
// timestamp must read as unmeasured rather than as a plausible value.
const sparse = { id: 'm3', project_id: 'p1', user_id: 'u-sparse', email: null, display_name: null, role_id: '', status: '', created_at: 0 }
// The roster names the member when the gateway could join the user, so these
// rows carry a display name, an email alone and neither.
const named = { id: 'm4', project_id: 'p1', user_id: 'u-named', email: 'dee@example.com', display_name: 'Dee Veloper', role_id: 'r-dev', status: 'active', created_at: 1_700_000_300, updated_at: 1_700_000_300 }
const emailOnly = { id: 'm5', project_id: 'p1', user_id: 'u-email', email: 'solo@example.com', display_name: null, role_id: 'r-dev', status: 'active', created_at: 1_700_000_400, updated_at: 1_700_000_400 }
const anonymous = { id: 'm6', project_id: 'p1', user_id: 'u-anon', email: null, display_name: null, role_id: 'r-dev', status: 'active', created_at: 1_700_000_500, updated_at: 1_700_000_500 }

const mock = (options: { members?: unknown; failMembers?: boolean; failPost?: boolean; permissions?: string[] } = {}) => vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
  const path = String(input)
  if (init?.method === 'POST') return options.failPost
    ? Promise.resolve(new Response(JSON.stringify({ error: { type: 'invalid_request', message: 'This role cannot be granted.' } }), { status: 400, headers: { 'Content-Type': 'application/json' } }))
    : json({})
  if (init?.method === 'DELETE') return json({})
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(options.permissions ?? ['project:manage'])
  if (path.includes('/members')) return options.failMembers ? Promise.reject(new Error('members unavailable')) : json(options.members ?? members)
  if (path.endsWith('/roles')) return json(roles)
  return json([])
})
const sent = (fetchMock: ReturnType<typeof vi.fn>, method: string) => fetchMock.mock.calls.filter(([, init]) => (init as RequestInit | undefined)?.method === method)
function ProjectSwitch() {
  const { projects, setProjectId } = useProject()
  return <>{projects.map((candidate) => <button key={candidate.id} type="button" onClick={() => setProjectId(candidate.id)}>{candidate.name}</button>)}</>
}
const renderPage = () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ProjectProvider>
          <AccessPage />
          <ProjectSwitch />
          <ConfirmHost />
        </ProjectProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { client, view }
}
const openMembers = async () => { await userEvent.click(await screen.findByRole('tab', { name: 'Members' })) }

describe('project members', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })

  it('renders the fields the API returns and shows — for absent ones', async () => {
    vi.stubGlobal('fetch', mock({ members: [...members, sparse] }))
    renderPage()
    await openMembers()

    for (const header of ['Member identity', 'Role', 'Ownership', 'Status', 'Created', 'Updated']) {
      expect(await screen.findByRole('columnheader', { name: header })).toBeInTheDocument()
    }
    const owner = await screen.findByRole('row', { name: /u-owner/ })
    expect(within(owner).getByText('Project owner')).toBeInTheDocument()
    expect(within(owner).getByText('Owner')).toBeInTheDocument()
    expect(within(owner).getByText('Active')).toBeInTheDocument()

    const member = screen.getByRole('row', { name: /u-member/ })
    expect(within(member).getByText('Member')).toBeInTheDocument()
    expect(within(member).getByText('Developer')).toBeInTheDocument()
    expect(within(member).getByText('Suspended')).toBeInTheDocument()

    // Role, status, created and updated are all absent on this row.
    expect(within(screen.getByRole('row', { name: /u-sparse/ })).getAllByText('—')).toHaveLength(4)
  })

  it('names the member and keeps the email as secondary text', async () => {
    vi.stubGlobal('fetch', mock({ members: [named] }))
    renderPage()
    await openMembers()

    // The row is reachable by the name, not by the opaque id it no longer shows.
    const row = await screen.findByRole('row', { name: /Dee Veloper/ })
    expect(within(row).getByText('dee@example.com')).toBeInTheDocument()
    expect(within(row).queryByText('u-named')).not.toBeInTheDocument()
  })

  it('shows the email alone when the member has no display name', async () => {
    vi.stubGlobal('fetch', mock({ members: [emailOnly] }))
    renderPage()
    await openMembers()

    const row = await screen.findByRole('row', { name: /solo@example.com/ })
    expect(within(row).queryByText('u-email')).not.toBeInTheDocument()
  })

  it('falls back to the user id when the API returns neither a name nor an email', async () => {
    vi.stubGlobal('fetch', mock({ members: [anonymous] }))
    renderPage()
    await openMembers()

    const row = await screen.findByRole('row', { name: /u-anon/ })
    expect(within(row).getByText('u-anon')).toBeInTheDocument()
    // The identity cell is never blank and never invented, so it carries no dash.
    expect(within(row).queryByText('—')).not.toBeInTheDocument()
  })

  it('posts the membership the add dialog collected', async () => {
    const fetchMock = mock()
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await openMembers()

    await userEvent.click(await screen.findByRole('button', { name: 'Add member' }))
    const dialog = await screen.findByRole('dialog', { name: 'Add member' })
    await userEvent.type(within(dialog).getByLabelText(/User ID/), 'u-new')
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Role' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Developer' }))
    await userEvent.click(within(dialog).getByRole('button', { name: 'Add member' }))

    await waitFor(() => expect(sent(fetchMock, 'POST')).toHaveLength(1))
    const [path, init] = sent(fetchMock, 'POST')[0] as [string, RequestInit]
    expect(path).toBe('/api/admin/v1/projects/p1/members')
    expect(JSON.parse(String(init.body))).toEqual({ user_id: 'u-new', role_id: 'r-dev', status: 'active' })
  })

  it('edits a member in place through POST and refreshes that project roster', async () => {
    const fetchMock = mock()
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await openMembers()

    const roster = await screen.findByRole('region', { name: 'Members' })
    const member = within(roster).getByRole('row', { name: /u-member/ })
    const readsBefore = fetchMock.mock.calls.filter(([path, init]) => String(path).includes('/projects/p1/members') && !init?.method).length
    fireEvent.click(within(member).getByRole('button', { name: 'Edit u-member' }))
    const dialog = await screen.findByRole('dialog')
    expect(dialog).toHaveAccessibleName('Edit u-member')
    expect(within(dialog).getByLabelText('User ID')).toHaveValue('u-member')
    expect(within(dialog).getByLabelText('User ID')).toBeDisabled()
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Status' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Active' }))
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save changes' }))

    await waitFor(() => expect(sent(fetchMock, 'POST')).toHaveLength(1))
    const [path, init] = sent(fetchMock, 'POST')[0] as [string, RequestInit]
    expect(path).toBe('/api/admin/v1/projects/p1/members')
    expect(JSON.parse(String(init.body))).toEqual({ user_id: 'u-member', role_id: 'r-dev', status: 'active' })
    await waitFor(() => expect(fetchMock.mock.calls.filter(([readPath, readInit]) => String(readPath).includes('/projects/p1/members') && !readInit?.method).length).toBeGreaterThan(readsBefore))
  })

  it('keeps an edit failure beside the form that caused it', async () => {
    vi.stubGlobal('fetch', mock({ failPost: true }))
    renderPage()
    await openMembers()
    const roster = await screen.findByRole('region', { name: 'Members' })
    fireEvent.click(within(within(roster).getByRole('row', { name: /u-member/ })).getByRole('button', { name: 'Edit u-member' }))
    const dialog = await screen.findByRole('dialog', { name: 'Edit u-member' })

    fireEvent.click(within(dialog).getByRole('button', { name: 'Save changes' }))

    expect(await within(dialog).findByRole('alert')).toHaveTextContent('This role cannot be granted.')
    expect(dialog).toBeInTheDocument()
  })

  it('reports a missing user id in the form instead of posting it', async () => {
    const fetchMock = mock()
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await openMembers()

    await userEvent.click(await screen.findByRole('button', { name: 'Add member' }))
    const dialog = await screen.findByRole('dialog', { name: 'Add member' })
    await userEvent.click(within(dialog).getByRole('button', { name: 'Add member' }))

    expect(await within(dialog).findByText('Enter the user ID.')).toBeInTheDocument()
    expect(sent(fetchMock, 'POST')).toHaveLength(0)
  })

  it('asks for confirmation before removing a member, and only then deletes', async () => {
    const fetchMock = mock()
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await openMembers()

    const roster = await screen.findByRole('region', { name: 'Members' })
    const memberRow = await within(roster).findByRole('row', { name: /u-member/ })
    await within(memberRow).findByText('Developer')
    fireEvent.click(within(memberRow).getByRole('button', { name: 'Remove u-member from the project' }))
    const dialog = await screen.findByRole('dialog', { name: 'Remove “u-member” from “Project A”?' })
    expect(within(dialog).getByText(/loses access to this project immediately/i)).toBeInTheDocument()
    expect(sent(fetchMock, 'DELETE')).toHaveLength(0)

    await userEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    expect(sent(fetchMock, 'DELETE')).toHaveLength(0)

    fireEvent.click(within(memberRow).getByRole('button', { name: 'Remove u-member from the project' }))
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Remove from project' }))
    await waitFor(() => expect(sent(fetchMock, 'DELETE')).toHaveLength(1))
    expect(String(sent(fetchMock, 'DELETE')[0][0])).toBe('/api/admin/v1/projects/p1/members/u-member')
  })

  it('offers no removal for the project owner, which the backend refuses', async () => {
    vi.stubGlobal('fetch', mock())
    renderPage()
    await openMembers()

    await screen.findByRole('row', { name: /u-owner/ })
    expect(screen.queryByRole('button', { name: 'Remove u-owner from the project' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Edit u-owner' })).not.toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Remove u-member from the project' })).toHaveLength(2)
  })

  it('keeps the roster readable with project:read and exposes no write controls', async () => {
    vi.stubGlobal('fetch', mock({ permissions: ['project:read'] }))
    renderPage()
    await openMembers()

    expect(await screen.findByRole('region', { name: 'Members' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Add member' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Edit u-member' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Remove u-member from the project' })).not.toBeInTheDocument()
  })

  it('searches and paginates the roster and renders the shared mobile cards', async () => {
    const roster = Array.from({ length: 26 }, (_, index) => ({
      ...anonymous,
      id: `m-${index}`,
      user_id: `u-${String(index).padStart(2, '0')}`,
      created_at: 1_700_000_500 + index,
      updated_at: 1_700_000_500 + index,
    }))
    vi.stubGlobal('fetch', mock({ members: roster }))
    const { view } = renderPage()
    await openMembers()

    const table = await screen.findByRole('region', { name: 'Members' })
    // This fixture deliberately renders the 25 visible records twice (desktop
    // rows and mobile cards). Rebuilding the full accessibility tree for every
    // pagination/search poll made this test exceed Vitest's five-second budget
    // under the full-suite load. The region itself is still located by its
    // accessible name; exact identity text is the observable list result here.
    expect(within(table).getByText('u-00')).toBeInTheDocument()
    expect(within(table).queryByText('u-25')).not.toBeInTheDocument()
    const mobile = view.container.querySelector('.mobile-resource-list') as HTMLElement
    expect(mobile).toBeInTheDocument()
    expect(within(mobile).getByText('u-00')).toBeInTheDocument()
    const currentTable = () => view.container.querySelector('.desktop-resource-table') as HTMLElement

    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() => expect(within(currentTable()).getByText('u-25')).toBeInTheDocument())

    fireEvent.change(screen.getByLabelText('Search'), { target: { value: 'u-07' } })
    await waitFor(() => expect(within(currentTable()).getByText('u-07')).toBeInTheDocument())
    expect(within(currentTable()).queryByText('u-25')).not.toBeInTheDocument()
  })

  it('refreshes only the roster captured by an edit when the project switches in flight', async () => {
    const updated = deferred()
    const otherMember = { ...anonymous, id: 'm-other', project_id: 'p2', user_id: 'u-other' }
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'POST' && path.includes('/members')) return updated.gate.then(() => jsonResponse({}))
      if (init?.method === 'DELETE') return json({})
      if (path.endsWith('/projects')) return json([project, otherProject])
      if (path.includes('/permissions')) return json(['project:manage'])
      if (path.includes('/members')) return json(path.includes('/projects/p2/') ? [otherMember] : members)
      if (path.includes('/roles')) return json(roles)
      return json([])
    })
    vi.stubGlobal('fetch', fetchMock)
    const { client } = renderPage()
    await openMembers()
    const roster = await screen.findByRole('region', { name: 'Members' })
    fireEvent.click(within(within(roster).getByRole('row', { name: /u-member/ })).getByRole('button', { name: 'Edit u-member' }))
    const dialog = await screen.findByRole('dialog', { name: 'Edit u-member' })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save changes' }))
    await waitFor(() => expect(sent(fetchMock, 'POST')).toHaveLength(1))

    client.setQueryData(['project-permissions', 'p2'], ['project:manage'])
    fireEvent.click(screen.getByRole('button', { name: 'Project B' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('The selected project changed. Close this form and reopen it from the current project.')
    expect(within(screen.getByRole('dialog', { name: 'Edit u-member' })).getByRole('button', { name: 'Save changes' })).toBeDisabled()
    expect(await screen.findByRole('row', { name: /u-other/ })).toBeInTheDocument()
    const firstKey = ['resource', 'p1', 'members', '/api/admin/v1/projects/p1/members', 0, 25, '']
    const secondKey = ['resource', 'p2', 'members', '/api/admin/v1/projects/p2/members', 0, 25, '']
    expect(client.getQueryState(secondKey)?.isInvalidated).toBe(false)
    const secondReads = fetchMock.mock.calls.filter(([path, init]) => String(path).includes('/projects/p2/members') && !init?.method).length

    updated.release()

    await waitFor(() => expect(client.getQueryState(firstKey)?.isInvalidated).toBe(true))
    expect(client.getQueryState(secondKey)?.isInvalidated).toBe(false)
    expect(fetchMock.mock.calls.filter(([path, init]) => String(path).includes('/projects/p2/members') && !init?.method)).toHaveLength(secondReads)
  })

  it('shows the empty state with one call to action when the project has no members', async () => {
    vi.stubGlobal('fetch', mock({ members: [] }))
    renderPage()
    await openMembers()

    expect(await screen.findByText('No members yet.')).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Add member' })).toHaveLength(1)
  })

  it('shows a retryable error when the membership list fails', async () => {
    const fetchMock = mock({ failMembers: true })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await openMembers()

    expect(await screen.findByRole('alert')).toHaveTextContent('The request failed. Try again shortly.')
    const before = fetchMock.mock.calls.filter(([path]) => String(path).includes('/members')).length
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('/members')).length).toBeGreaterThan(before))
  })
})
