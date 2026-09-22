import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider, useProject } from '../project'
import AccessPage from './AccessPage'

const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const projectA = { id: 'p1', name: 'Project One', slug: 'one', owner_user_id: 'owner', is_default: true, enabled: true }
const projectB = { id: 'p2', name: 'Project Two', slug: 'two', owner_user_id: 'owner', is_default: false, enabled: true }
const catalog = [
  { slug: 'gateway:use', level: 'project', description: 'Use gateway APIs' },
  { slug: 'project:read', level: 'project', description: 'Read project resources' },
]

function SwitchProject() {
  const { setProjectId } = useProject()
  return <button type="button" onClick={() => setProjectId('p2')}>Switch project</button>
}

function renderRoles(extra?: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage />{extra}</ProjectProvider></MemoryRouter></QueryClientProvider>)
  return client
}

describe('project role permission picker', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en')
    localStorage.clear()
  })

  it('creates a role from catalog checkboxes and submits a plain string array', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([projectA])
      if (path.endsWith('/permissions')) return json(['role:manage'])
      if (path.endsWith('/permission-catalog')) return json(catalog)
      if (path.includes('/roles') && init?.method === 'POST') return json({ id: 'new-role' }, 201)
      if (path.includes('/roles')) return json([])
      return json([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderRoles()

    await userEvent.click(await screen.findByRole('button', { name: 'Add role' }))
    const dialog = screen.getByRole('dialog')
    expect(await within(dialog).findByRole('checkbox', { name: /project:read/i })).toBeChecked()
    await userEvent.click(within(dialog).getByRole('checkbox', { name: /gateway:use/i }))
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'Gateway reader')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      const request = fetchMock.mock.calls.find(([path, request]) => String(path).endsWith('/projects/p1/roles') && request?.method === 'POST')
      expect(request).toBeDefined()
      expect(JSON.parse(String(request?.[1]?.body))).toEqual({ name: 'Gateway reader', permissions: ['gateway:use', 'project:read'] })
    })
  })

  it('round-trips an existing role and preserves an unknown stored slug read-only', async () => {
    const role = { id: 'role-1', project_id: 'p1', name: 'Existing', scope: 'project', is_system: false, permissions: '["project:read","future:delegate"]' }
    const systemRole = { id: 'system-role', project_id: null, name: 'owner', scope: 'system', is_system: true, permissions: ['*'] }
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([projectA])
      if (path.endsWith('/permissions')) return json(['role:manage'])
      if (path.endsWith('/permission-catalog')) return json(catalog)
      if (path.endsWith('/roles/role-1') && init?.method === 'PATCH') return json(role)
      if (path.includes('/roles')) return json([systemRole, role])
      return json([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderRoles()

    const rolesTable = await screen.findByRole('region', { name: 'Roles' })
    const edit = within(rolesTable).getByRole('button', { name: 'Edit' })
    expect(within(rolesTable).getAllByRole('button', { name: 'Edit' })).toHaveLength(1)
    fireEvent.click(edit)
    const dialog = screen.getByRole('dialog')
    expect(await within(dialog).findByRole('checkbox', { name: /future:delegate/i })).toBeChecked()
    expect(within(dialog).getByRole('checkbox', { name: /future:delegate/i })).toBeDisabled()
    expect(within(dialog).getByText('Unavailable in the current permission catalog')).toBeInTheDocument()
    await userEvent.click(within(dialog).getByRole('checkbox', { name: /project:read/i }))
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      const request = fetchMock.mock.calls.find(([path, request]) => String(path).endsWith('/roles/role-1') && request?.method === 'PATCH')
      expect(JSON.parse(String(request?.[1]?.body))).toEqual({ name: 'Existing', permissions: ['future:delegate'] })
    })
  })

  it('shows catalog failure with retry and then a useful empty state', async () => {
    let reads = 0
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([projectA])
      if (path.endsWith('/permissions')) return json(['role:manage'])
      if (path.endsWith('/permission-catalog')) {
        reads += 1
        return reads === 1 ? json({ error: { message: 'catalog unavailable' } }, 503) : json([])
      }
      if (path.includes('/roles')) return json([])
      return json([])
    }))
    renderRoles()

    await userEvent.click(await screen.findByRole('button', { name: 'Add role' }))
    const dialog = screen.getByRole('dialog')
    expect(await within(dialog).findByText('The permission catalog could not be loaded.')).toBeInTheDocument()
    const save = within(dialog).getByRole('button', { name: 'Save' })
    expect(save).toBeDisabled()
    await userEvent.click(within(dialog).getByRole('button', { name: 'Retry' }))
    expect(await within(dialog).findByText('No permissions are available to delegate in this project.')).toBeInTheDocument()
    expect(save).toBeEnabled()
  })

  it('closes an in-flight form on project switch and ignores the stale catalog', async () => {
    let releaseCatalog!: (value: Response) => void
    const firstCatalog = new Promise<Response>((resolve) => { releaseCatalog = resolve })
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([projectA, projectB])
      if (path.endsWith('/permissions')) return json(['role:manage'])
      if (path.endsWith('/projects/p1/permission-catalog')) return firstCatalog
      if (path.endsWith('/projects/p2/permission-catalog')) return json([{ slug: 'p2:only', level: 'project', description: 'Project two permission' }])
      if (path.includes('/roles')) return json([])
      return json([])
    }))
    renderRoles(<SwitchProject />)

    await userEvent.click(await screen.findByRole('button', { name: 'Add role' }))
    expect(within(screen.getByRole('dialog')).getByRole('status')).toHaveTextContent('Loading')
    await userEvent.click(screen.getByRole('button', { name: 'Switch project' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    await act(async () => releaseCatalog(await json([{ slug: 'p1:stale', level: 'project', description: 'Stale project permission' }])))

    await userEvent.click(await screen.findByRole('button', { name: 'Add role' }))
    const dialog = screen.getByRole('dialog')
    expect(await within(dialog).findByRole('checkbox', { name: /p2:only/i })).toBeInTheDocument()
    expect(within(dialog).queryByText(/p1:stale/i)).not.toBeInTheDocument()
  })
})
