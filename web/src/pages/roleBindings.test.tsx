import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, fireEvent, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfirmHost } from '../components'
import i18n from '../i18n'
import { ProjectProvider, useProject } from '../project'
import AccessPage from './AccessPage'

/**
 * `GET`/`DELETE /api/admin/v1/users/{user_id}/role-bindings` existed with no
 * console consumer, so a binding could be created and never seen or revoked.
 * The list is read for one user at a time, the role name is resolved from the
 * project's role list, and the revocation goes through the shared confirmation
 * dialog. Listing reads the project-scoped route, which needs `role:manage` on
 * requires (`src/access.rs::list_role_bindings`), so a principal without it is
 * told why instead of being sent a request that can only fail.
 *
 * The request path names the project, so the cache entry must name it too:
 * otherwise the same user id in another project renders the first project's rows.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const otherProject = { id: 'p2', name: 'Project B', slug: 'project-b', owner_user_id: 'u1', is_default: false, enabled: true }
const roles = [{ id: 'r-dev', name: 'Developer' }]
const otherRoles = [{ id: 'r-review', name: 'Reviewer' }]
const bindings = [
  { id: 'b1', user_id: 'u-1', role_id: 'r-dev', project_id: 'p1', created_at: 1_700_000_000 },
  // A binding whose role this console cannot resolve, and a global one with no
  // project and no recorded time: data, shown verbatim, never invented.
  { id: 'b2', user_id: 'u-1', role_id: 'r-unknown', project_id: null, created_at: 0 },
]
const otherBindings = [{ id: 'b3', user_id: 'u-1', role_id: 'r-review', project_id: 'p2', created_at: 1_700_000_500 }]
const mock = (options: { permissions?: string[]; bindings?: unknown; otherBindings?: unknown; failList?: boolean } = {}) => vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
  const path = String(input)
  if (init?.method === 'DELETE') return json({})
  if (init?.method === 'POST') return json({})
  if (path.endsWith('/projects')) return json([project, otherProject])
  if (path.includes('/permissions')) return json(options.permissions ?? ['user:manage', 'role:manage'])
  if (path.includes('/role-bindings')) {
    if (options.failList) return Promise.reject(new Error('bindings unavailable'))
    // Each project answers with its own rows for the same user.
    return json(path.includes('/projects/p2/') ? options.otherBindings ?? otherBindings : options.bindings ?? bindings)
  }
  if (path.includes('/roles')) return json(path.includes('/projects/p2/') ? otherRoles : roles)
  return json([])
})

/** The project switcher the shell owns; the harness drives the same provider state. */
function ProjectSwitch() {
  const { project: active, projects, setProjectId } = useProject()
  return (
    <div>
      <span data-testid="active-project">{active.id}</span>
      {projects.map((candidate) => <button key={candidate.id} type="button" onClick={() => setProjectId(candidate.id)}>{candidate.name}</button>)}
    </div>
  )
}

const renderPage = (fetchMock?: ReturnType<typeof vi.fn>) => {
  if (fetchMock) vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /><ProjectSwitch /><ConfirmHost /></ProjectProvider></MemoryRouter></QueryClientProvider>)
  return { client, view }
}
/** The role-binding half of the assignments panel, addressed by its own heading. */
const bindingForm = async () => {
  await userEvent.click(await screen.findByRole('tab', { name: 'Roles' }))
  // The assignments live in a collapsed accordion item.
  await userEvent.click(await screen.findByRole('button', { name: 'Membership and role assignments' }))
  return (await screen.findByRole('heading', { name: 'Role binding' })).closest('form') as HTMLElement
}
const loadBindings = async (user = 'u-1') => {
  const form = await bindingForm()
  await userEvent.type(within(form).getByLabelText(/User ID/), user)
  await userEvent.click(within(form).getByRole('button', { name: 'Show bindings' }))
}
const listCalls = (fetchMock: ReturnType<typeof vi.fn>) => fetchMock.mock.calls.filter(([path]) => String(path).includes('/role-bindings'))
const listCallsFor = (fetchMock: ReturnType<typeof vi.fn>, projectId: string) => listCalls(fetchMock).filter(([path]) => String(path).includes(`/projects/${projectId}/`))
const deletes = (fetchMock: ReturnType<typeof vi.fn>) => fetchMock.mock.calls.filter(([, init]) => (init as RequestInit | undefined)?.method === 'DELETE')
/** A response the test releases itself, so a project switch can happen while it is in flight. */
const deferred = () => {
  let release: () => void = () => {}
  const gate = new Promise<void>((resolve) => { release = resolve })
  return { gate, release: () => release() }
}
const jsonResponse = (value: unknown) => new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } })
/**
 * Answering the target project's permissions before the switch takes the loading interval
 * out of the way, so these cases stay on the cache identity they are about. The interval
 * itself is covered by "the access page while the active project’s permissions are in
 * flight" below, which deliberately leaves them unanswered.
 */
const warmOtherProject = (client: QueryClient) => client.setQueryData(['project-permissions', 'p2'], ['user:manage', 'role:manage'])

describe('role bindings can be listed and revoked', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })

  it('lists the bindings of the user it was asked about', async () => {
    const fetchMock = mock()
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await loadBindings()

    expect(await screen.findByRole('button', { name: 'Revoke Developer' })).toBeInTheDocument()
    const [path] = listCalls(fetchMock)[0] as [string]
    expect(path).toBe('/api/admin/v1/projects/p1/users/u-1/role-bindings')
    // A role the console cannot resolve keeps its id, and the absent project and
    // time read as unmeasured.
    const unresolved = screen.getByRole('row', { name: /r-unknown/ })
    expect(within(unresolved).getAllByText('—')).toHaveLength(2)
  })

  it('asks through the shared dialog and deletes only the confirmed binding', async () => {
    const fetchMock = mock()
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await loadBindings()
    await screen.findByRole('button', { name: 'Revoke Developer' })

    await userEvent.click(screen.getByRole('button', { name: 'Revoke Developer' }))
    const dialog = screen.getByRole('dialog', { name: 'Revoke the “Developer” role binding?' })
    expect(within(dialog).getByText(/loses the permissions this role granted/i)).toBeInTheDocument()
    await userEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    expect(deletes(fetchMock)).toHaveLength(0)

    await userEvent.click(screen.getByRole('button', { name: 'Revoke Developer' }))
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Revoke' }))
    await waitFor(() => expect(deletes(fetchMock)).toHaveLength(1))
    expect(String(deletes(fetchMock)[0][0])).toBe('/api/admin/v1/users/u-1/role-bindings/b1')
  })

  it('says the user has no bindings instead of showing an empty table', async () => {
    vi.stubGlobal('fetch', mock({ bindings: [] }))
    renderPage()
    await loadBindings()
    expect(await screen.findByText('This user has no role bindings.')).toBeInTheDocument()
  })

  it('reports a failed list with a retry that re-reads it', async () => {
    const fetchMock = mock({ failList: true })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await loadBindings()

    expect(await screen.findByRole('alert')).toHaveTextContent('The request failed. Try again shortly.')
    const before = listCalls(fetchMock).length
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await waitFor(() => expect(listCalls(fetchMock).length).toBeGreaterThan(before))
  })

  it('shows the switched project’s bindings for the same user instead of the first project’s rows', async () => {
    const fetchMock = mock()
    const { client } = renderPage(fetchMock)
    await loadBindings()
    expect(await screen.findByRole('button', { name: 'Revoke Developer' })).toBeInTheDocument()
    const readsForFirst = listCallsFor(fetchMock, 'p1').length
    warmOtherProject(client)

    await userEvent.click(screen.getByRole('button', { name: 'Project B' }))

    // The second project's request runs — the cache entry names the project — and the
    // first project's row is gone rather than reused.
    await waitFor(() => expect(listCallsFor(fetchMock, 'p2').length).toBeGreaterThan(0))
    expect(await screen.findByRole('button', { name: 'Revoke Reviewer' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Revoke Developer' })).not.toBeInTheDocument()
    // The row shown is the second project's binding, not the first project's reused row.
    const bindingsTable = screen.getByRole('button', { name: 'Revoke Reviewer' }).closest('table') as HTMLElement
    expect(within(bindingsTable).getByText('p2')).toBeInTheDocument()
    expect(listCallsFor(fetchMock, 'p1')).toHaveLength(readsForFirst)
  })

  it('revokes against the active project and refreshes only that project’s list', async () => {
    const fetchMock = mock()
    const { client } = renderPage(fetchMock)
    await userEvent.click(await screen.findByRole('button', { name: 'Project B' }))
    await loadBindings()
    await screen.findByRole('button', { name: 'Revoke Reviewer' })
    const firstProjectReads = listCallsFor(fetchMock, 'p1').length
    // Seed the other project's entry so an over-broad invalidation would show up here.
    client.setQueryData(['role-bindings', 'p1', 'u-1'], bindings)
    // The active project's list lives under its own project-scoped key.
    expect(client.getQueryState(['role-bindings', 'p2', 'u-1'])).toBeTruthy()
    const before = listCallsFor(fetchMock, 'p2').length

    await userEvent.click(screen.getByRole('button', { name: 'Revoke Reviewer' }))
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Revoke' }))

    await waitFor(() => expect(listCallsFor(fetchMock, 'p2').length).toBeGreaterThan(before))
    // The other project's list was neither re-read nor invalidated.
    expect(listCallsFor(fetchMock, 'p1')).toHaveLength(firstProjectReads)
    expect(client.getQueryState(['role-bindings', 'p1', 'u-1'])?.isInvalidated).toBe(false)
  })

  it('shows a newly created binding without a second “Show bindings” round-trip', async () => {
    // The POST only changes the server's state once it has been processed, so a list
    // read issued before it lands would answer with the pre-create rows.
    let stored: unknown[] = []
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'DELETE') return json({})
      if (init?.method === 'POST' && path.includes('/role-bindings')) {
        return new Promise<Response>((resolve) => {
          setTimeout(() => {
            stored = [{ id: 'b-new', user_id: 'u-9', role_id: 'r-dev', project_id: 'p1', created_at: 1_700_000_999 }]
            void json({}).then(resolve)
          }, 0)
        })
      }
      if (path.endsWith('/projects')) return json([project, otherProject])
      if (path.includes('/permissions')) return json(['user:manage', 'role:manage'])
      if (path.includes('/role-bindings')) return json(stored)
      if (path.includes('/roles')) return json(roles)
      return json([])
    })
    renderPage(fetchMock)
    const form = await bindingForm()
    await userEvent.type(within(form).getByLabelText(/User ID/), 'u-9')
    await userEvent.type(within(form).getByLabelText(/Role ID/), 'r-dev')
    await userEvent.click(within(form).getByRole('button', { name: 'Assign' }))

    expect(await screen.findByRole('button', { name: 'Revoke Developer' })).toBeInTheDocument()
    expect(screen.queryByText('This user has no role bindings.')).not.toBeInTheDocument()
  })

  it('invalidates the project a binding was created in, not the one on screen when it settles', async () => {
    const created = deferred()
    let stored: unknown[] = []
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'DELETE') return json({})
      if (init?.method === 'POST' && path.includes('/role-bindings')) {
        // The create lands only when the test releases it, so the operator can switch
        // projects while it is in flight.
        return created.gate.then(() => {
          stored = [{ id: 'b-new', user_id: 'u-9', role_id: 'r-dev', project_id: 'p1', created_at: 1_700_000_999 }]
          return jsonResponse({})
        })
      }
      if (path.endsWith('/projects')) return json([project, otherProject])
      if (path.includes('/permissions')) return json(['user:manage', 'role:manage'])
      if (path.includes('/role-bindings')) return json(path.includes('/projects/p2/') ? otherBindings : stored)
      if (path.includes('/roles')) return json(path.includes('/projects/p2/') ? otherRoles : roles)
      return json([])
    })
    const { client } = renderPage(fetchMock)
    warmOtherProject(client)
    const form = await bindingForm()
    await userEvent.type(within(form).getByLabelText(/User ID/), 'u-9')
    await userEvent.type(within(form).getByLabelText(/Role ID/), 'r-dev')
    await userEvent.click(within(form).getByRole('button', { name: 'Assign' }))
    await waitFor(() => expect(client.getQueryState(['role-bindings', 'p1', 'u-9'])).toBeTruthy())

    // The operator switches projects before the create settles.
    await userEvent.click(screen.getByRole('button', { name: 'Project B' }))
    await waitFor(() => expect(screen.getByTestId('active-project')).toHaveTextContent('p2'))
    const secondProjectReads = listCallsFor(fetchMock, 'p2').length

    created.release()

    // The list the create changed is project A's, so that is the entry invalidated...
    await waitFor(() => expect(client.getQueryState(['role-bindings', 'p1', 'u-9'])?.isInvalidated).toBe(true))
    // ...and the project that happened to be on screen is left alone.
    expect(listCallsFor(fetchMock, 'p2')).toHaveLength(secondProjectReads)
    expect(client.getQueryState(['role-bindings', 'p2', 'u-9'])?.isInvalidated).toBe(false)

    // Going back to project A shows the binding that was created, without asking for the
    // list by hand.
    await userEvent.click(screen.getByRole('button', { name: 'Project A' }))
    expect(await screen.findByRole('button', { name: 'Revoke Developer' })).toBeInTheDocument()
  })

  it('invalidates the project a revoke happened in, not the one on screen when it settles', async () => {
    const removed = deferred()
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'DELETE') return removed.gate.then(() => jsonResponse({}))
      if (init?.method === 'POST') return json({})
      if (path.endsWith('/projects')) return json([project, otherProject])
      if (path.includes('/permissions')) return json(['user:manage', 'role:manage'])
      if (path.includes('/role-bindings')) return json(path.includes('/projects/p2/') ? otherBindings : bindings)
      if (path.includes('/roles')) return json(path.includes('/projects/p2/') ? otherRoles : roles)
      return json([])
    })
    const { client } = renderPage(fetchMock)
    warmOtherProject(client)
    await loadBindings()
    await screen.findByRole('button', { name: 'Revoke Developer' })
    await userEvent.click(screen.getByRole('button', { name: 'Revoke Developer' }))
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Revoke' }))
    await waitFor(() => expect(deletes(fetchMock)).toHaveLength(1))

    // The confirmation dialog stays open while the delete is in flight, so the switch is
    // fired directly: a shell that changes project without this panel's knowledge is
    // exactly the case the callback must survive.
    fireEvent.click(screen.getByRole('button', { name: 'Project B' }))
    await waitFor(() => expect(screen.getByTestId('active-project')).toHaveTextContent('p2'))
    const secondProjectReads = listCallsFor(fetchMock, 'p2').length

    removed.release()

    // Project A is the project the revoke changed, so its list is the one invalidated.
    await waitFor(() => expect(client.getQueryState(['role-bindings', 'p1', 'u-1'])?.isInvalidated).toBe(true))
    expect(listCallsFor(fetchMock, 'p2')).toHaveLength(secondProjectReads)
    expect(client.getQueryState(['role-bindings', 'p2', 'u-1'])?.isInvalidated).toBe(false)
  })
})

/**
 * The permissions query is keyed by the active project, so a switch leaves it unanswered
 * for the project on screen. "Not answered yet" is not "no permissions": while the answer
 * is in flight the page says it is reading this project's permissions and keeps the panel
 * the operator was working in mounted — hidden and unreachable — so their work survives.
 * Nothing of the next project is offered or read until that project's own answer arrives;
 * only an answer granting no module shows the no-access card, and an answer that could not
 * be read shows its own retry instead.
 */
describe('the access page while the active project’s permissions are in flight', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })

  const noAccess = 'No access-control modules are available for your permissions.'
  const loadingCopy = () => i18n.t('accessPermissionsLoading', { lng: 'en' })
  const retry = () => i18n.t('retry', { lng: 'en' })

  it('keeps the panel and the work in it instead of claiming there is no access', async () => {
    const next = deferred()
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'POST' || init?.method === 'DELETE') return json({})
      if (path.endsWith('/projects')) return json([project, otherProject])
      if (path.includes('/projects/p2/permissions')) return next.gate.then(() => jsonResponse(['user:manage', 'role:manage']))
      if (path.includes('/permissions')) return json(['user:manage', 'role:manage'])
      if (path.includes('/role-bindings')) return json(path.includes('/projects/p2/') ? otherBindings : bindings)
      if (path.includes('/roles')) return json(path.includes('/projects/p2/') ? otherRoles : roles)
      return json([])
    })
    renderPage(fetchMock)
    const form = await bindingForm()
    await userEvent.type(within(form).getByLabelText(/User ID/), 'u-1')
    await userEvent.type(within(form).getByLabelText(/Role ID/), 'r-dev')
    await userEvent.click(within(form).getByRole('button', { name: 'Show bindings' }))
    await screen.findByRole('button', { name: 'Revoke Developer' })

    fireEvent.click(screen.getByRole('button', { name: 'Project B' }))

    // The page names what it is reading and claims nothing else about project B.
    await waitFor(() => expect(screen.getByTestId('active-project')).toHaveTextContent('p2'))
    expect(screen.getByText(loadingCopy())).toBeInTheDocument()
    expect(screen.getByRole('status', { name: i18n.t('loading', { lng: 'en' }) })).toBeInTheDocument()
    expect(screen.queryByText(noAccess)).not.toBeInTheDocument()
    // Nothing of project B is offered while its permissions are unknown...
    expect(screen.queryByRole('tab')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Show bindings' })).not.toBeInTheDocument()
    // ...and nothing of project B is read on the strength of project A's answer.
    expect(listCallsFor(fetchMock, 'p2')).toHaveLength(0)

    next.release()

    // The work in progress survived the switch and now belongs to the project on screen.
    expect(await screen.findByRole('button', { name: 'Revoke Reviewer' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Revoke Developer' })).not.toBeInTheDocument()
    expect(screen.queryByText(noAccess)).not.toBeInTheDocument()
    const kept = (await screen.findByRole('heading', { name: 'Role binding' })).closest('form') as HTMLElement
    expect(within(kept).getByLabelText(/User ID/)).toHaveValue('u-1')
  })

  it('invalidates the project a create happened in while the next project is still loading', async () => {
    const created = deferred()
    const next = deferred()
    let stored: unknown[] = []
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'DELETE') return json({})
      if (init?.method === 'POST' && path.includes('/role-bindings')) {
        return created.gate.then(() => {
          stored = [{ id: 'b-new', user_id: 'u-9', role_id: 'r-dev', project_id: 'p1', created_at: 1_700_000_999 }]
          return jsonResponse({})
        })
      }
      if (init?.method === 'POST') return json({})
      if (path.endsWith('/projects')) return json([project, otherProject])
      if (path.includes('/projects/p2/permissions')) return next.gate.then(() => jsonResponse(['user:manage', 'role:manage']))
      if (path.includes('/permissions')) return json(['user:manage', 'role:manage'])
      if (path.includes('/role-bindings')) return json(path.includes('/projects/p2/') ? otherBindings : stored)
      if (path.includes('/roles')) return json(path.includes('/projects/p2/') ? otherRoles : roles)
      return json([])
    })
    const { client } = renderPage(fetchMock)
    const form = await bindingForm()
    await userEvent.type(within(form).getByLabelText(/User ID/), 'u-9')
    await userEvent.type(within(form).getByLabelText(/Role ID/), 'r-dev')
    await userEvent.click(within(form).getByRole('button', { name: 'Assign' }))
    await waitFor(() => expect(client.getQueryState(['role-bindings', 'p1', 'u-9'])).toBeTruthy())

    // The operator switches while the create is in flight and project B's permissions
    // have not been answered yet.
    fireEvent.click(screen.getByRole('button', { name: 'Project B' }))
    await waitFor(() => expect(screen.getByTestId('active-project')).toHaveTextContent('p2'))
    expect(screen.queryByRole('tab')).not.toBeInTheDocument()

    created.release()

    // The list the create changed is project A's, whatever is on screen when it settles.
    await waitFor(() => expect(client.getQueryState(['role-bindings', 'p1', 'u-9'])?.isInvalidated).toBe(true))
    expect(client.getQueryState(['role-bindings', 'p2', 'u-9'])?.isInvalidated).toBe(false)
    expect(listCallsFor(fetchMock, 'p2')).toHaveLength(0)

    // Back on project A the created binding is on screen, without asking for the list by
    // hand: the panel and the user it was showing survived the switch.
    fireEvent.click(screen.getByRole('button', { name: 'Project A' }))
    expect(await screen.findByRole('button', { name: 'Revoke Developer' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Revoke Reviewer' })).not.toBeInTheDocument()
  })

  it('shows the no-access card only once the next project’s answer grants nothing', async () => {
    const next = deferred()
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'POST' || init?.method === 'DELETE') return json({})
      if (path.endsWith('/projects')) return json([project, otherProject])
      if (path.includes('/projects/p2/permissions')) return next.gate.then(() => jsonResponse([]))
      if (path.includes('/permissions')) return json(['user:manage', 'role:manage'])
      if (path.includes('/role-bindings')) return json(bindings)
      if (path.includes('/roles')) return json(roles)
      return json([])
    })
    renderPage(fetchMock)
    await screen.findByRole('tab', { name: 'Roles' })

    fireEvent.click(screen.getByRole('button', { name: 'Project B' }))
    await waitFor(() => expect(screen.getByTestId('active-project')).toHaveTextContent('p2'))
    expect(screen.getByText(loadingCopy())).toBeInTheDocument()
    expect(screen.queryByText(noAccess)).not.toBeInTheDocument()

    next.release()

    expect(await screen.findByText(noAccess)).toBeInTheDocument()
    expect(screen.queryByRole('tab')).not.toBeInTheDocument()
  })

  it('offers the retry instead of the no-access card when the answer cannot be read', async () => {
    let reads = 0
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'POST' || init?.method === 'DELETE') return json({})
      if (path.endsWith('/projects')) return json([project, otherProject])
      if (path.includes('/projects/p2/permissions')) {
        reads += 1
        return reads === 1 ? Promise.reject(new Error('permissions unavailable')) : json(['user:manage', 'role:manage'])
      }
      if (path.includes('/permissions')) return json(['user:manage', 'role:manage'])
      if (path.includes('/role-bindings')) return json(bindings)
      if (path.includes('/roles')) return json(roles)
      return json([])
    })
    renderPage(fetchMock)
    await screen.findByRole('tab', { name: 'Roles' })

    fireEvent.click(screen.getByRole('button', { name: 'Project B' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(i18n.t('networkError', { lng: 'en' }))
    expect(screen.queryByText(noAccess)).not.toBeInTheDocument()
    expect(screen.queryByRole('tab')).not.toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: retry() }))

    expect(await screen.findByRole('tab', { name: 'Roles' })).toBeInTheDocument()
    expect(screen.queryByText(noAccess)).not.toBeInTheDocument()
    expect(reads).toBe(2)
  })

  it('stops offering the project’s controls when its permissions can no longer be read', async () => {
    // The console holds an answer for project B — the cache the switch tests warm — and the
    // read that would confirm it fails. A cached answer is not a licence to keep offering
    // the controls it authorized, and a failed read is not a denial either, so the console
    // ends up showing its retry rather than the no-access card.
    let reads = 0
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'POST' || init?.method === 'DELETE') return json({})
      if (path.endsWith('/projects')) return json([project, otherProject])
      if (path.includes('/projects/p2/permissions')) {
        reads += 1
        return reads === 1
          ? Promise.resolve(new Response(JSON.stringify({ error: { type: 'internal_error', message: 'An internal error occurred' } }), { status: 500, headers: { 'Content-Type': 'application/json' } }))
          : json(['user:manage', 'role:manage'])
      }
      if (path.includes('/permissions')) return json(['user:manage', 'role:manage'])
      if (path.includes('/role-bindings')) return json(path.includes('/projects/p2/') ? otherBindings : bindings)
      if (path.includes('/roles')) return json(path.includes('/projects/p2/') ? otherRoles : roles)
      return json([])
    })
    const { client } = renderPage(fetchMock)
    await screen.findByRole('tab', { name: 'Roles' })
    warmOtherProject(client)

    await userEvent.click(screen.getByRole('button', { name: 'Project B' }))
    await waitFor(() => expect(screen.getByTestId('active-project')).toHaveTextContent('p2'))

    // The answer project B was carrying cannot be confirmed, so the controls it authorized
    // are gone and the retry is offered in their place...
    await waitFor(() => expect(screen.queryAllByRole('tab')).toHaveLength(0))
    expect(screen.getByRole('alert')).toHaveTextContent(i18n.t('networkError', { lng: 'en' }))
    // ...and a read that failed is not a denial.
    expect(screen.queryByText(noAccess)).not.toBeInTheDocument()
    expect(reads).toBe(1)

    await userEvent.click(screen.getByRole('button', { name: retry() }))

    expect(await screen.findByRole('tab', { name: 'Roles' })).toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    expect(reads).toBe(2)
  })

  it('says it is reading the project’s permissions in both languages', () => {
    for (const language of ['en', 'zh-CN']) expect(i18n.exists('accessPermissionsLoading', { lng: language })).toBe(true)
    expect(i18n.t('accessPermissionsLoading', { lng: 'en' })).not.toBe(i18n.t('accessPermissionsLoading', { lng: 'zh-CN' }))
  })
})
