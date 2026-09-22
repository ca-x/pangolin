import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider, useProject } from '../project'
import AccessPage from './AccessPage'

/**
 * The scoped API-key view reports `expires_at`, `budget_micros` and
 * `spent_micros`, and admission refuses a key that is disabled, expired or out
 * of budget. The table used to print "Enabled" for every enabled row, so a key
 * no client could authenticate with read as healthy. Each state below is the
 * one `src/db.rs::authenticate_api_key` enforces, and the spend is shown against
 * the budget rather than as a bare number.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const deferred = () => {
  let release: () => void = () => {}
  const gate = new Promise<void>((resolve) => { release = resolve })
  return { gate, release }
}
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const otherProject = { id: 'p2', name: 'Project B', slug: 'project-b', owner_user_id: 'u3', is_default: false, enabled: true }
const members = [
  { id: 'm1', project_id: 'p1', user_id: 'u1', email: 'owner@example.com', display_name: 'Project Owner', role_id: 'owner', status: 'active', created_at: 1, updated_at: 1 },
  { id: 'm2', project_id: 'p1', user_id: 'u2', email: 'suspended@example.com', display_name: 'Suspended User', role_id: 'member', status: 'suspended', created_at: 1, updated_at: 1 },
]
const key = (overrides: Record<string, unknown>) => ({ id: 'k', name: 'key', key_prefix: 'pk', key_type: 'service', scopes: ['gateway:use'], profile_id: null, allowed_ips_json: '[]', denied_ips_json: '[]', enabled: true, expires_at: null, budget_micros: null, spent_micros: 0, ...overrides })
// 2020: already past. 2096: far enough out that the assertion cannot go stale.
const keys = [
  key({ id: 'k1', name: 'expired-key', expires_at: 1_600_000_000 }),
  key({ id: 'k2', name: 'exhausted-key', budget_micros: 1_000_000, spent_micros: 1_000_000 }),
  key({ id: 'k3', name: 'usable-key', scopes: ['gateway:use', 'project:read'], expires_at: 4_000_000_000, budget_micros: 2_000_000, spent_micros: 500_000 }),
  key({ id: 'k4', name: 'off-key', enabled: false }),
  // A row from a build that reports neither spend nor expiry: the console must
  // not invent a state or a figure for it.
  { id: 'k5', name: 'legacy-key', key_prefix: 'pk5', key_type: 'service', profile_id: null, allowed_ips_json: '[]', denied_ips_json: '[]', enabled: true },
]
const apiMock = (options: { postError?: string; projects?: typeof project[] } = {}) => vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input)
    if (init?.method === 'POST' && path.includes('/api-keys')) {
      if (options.postError) return Promise.resolve(new Response(JSON.stringify({ error: { type: 'invalid_request', message: options.postError } }), { status: 400, headers: { 'Content-Type': 'application/json' } }))
      return json({ key: key({ id: 'created', name: 'created' }), token: 'pg_once', mode: 'generated' })
    }
    if (path.endsWith('/projects')) return json(options.projects ?? [project])
    if (path.includes('/permissions')) return json(['api_key:manage'])
    if (path.endsWith('/members')) return json(members)
    if (path.includes('/api-keys')) return json(keys)
    return json([])
  })
const sent = (fetchMock: ReturnType<typeof vi.fn>) => fetchMock.mock.calls.filter(([, init]) => (init as RequestInit | undefined)?.method === 'POST')
function ProjectSwitch() {
  const { projects, setProjectId } = useProject()
  return <>{projects.map((candidate) => <button key={candidate.id} type="button" onClick={() => setProjectId(candidate.id)}>{candidate.name}</button>)}</>
}
const renderPage = (fetchMock = apiMock()) => {
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return { fetchMock, ...render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /><ProjectSwitch /></ProjectProvider></MemoryRouter></QueryClientProvider>) }
}
const openCreate = async () => {
  await userEvent.click(await screen.findByRole('button', { name: 'Create API key' }))
  return screen.findByRole('dialog', { name: 'Create API key' })
}
const choose = async (dialog: HTMLElement, label: string, option: string) => {
  await userEvent.click(await within(dialog).findByRole('combobox', { name: label }))
  await userEvent.click(await screen.findByRole('option', { name: option }))
}

describe('the key table reports the state admission enforces', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })

  it('names expiry, spend and the derived state instead of "Enabled"', async () => {
    renderPage()
    expect(await screen.findByRole('columnheader', { name: 'Expires' })).toBeInTheDocument()
    expect(screen.getByRole('columnheader', { name: 'Spend / budget' })).toBeInTheDocument()

    const expired = await screen.findByRole('row', { name: /expired-key/ })
    expect(within(expired).getByText('Expired')).toBeInTheDocument()
    expect(within(expired).queryByText('Enabled')).not.toBeInTheDocument()

    const exhausted = screen.getByRole('row', { name: /exhausted-key/ })
    expect(within(exhausted).getByText('Out of budget')).toBeInTheDocument()
    expect(within(exhausted).getByText('$1.000000 / $1.000000')).toBeInTheDocument()

    const usable = screen.getByRole('row', { name: /usable-key/ })
    expect(within(usable).getByText('Usable')).toBeInTheDocument()
    expect(within(usable).getByText('$0.500000 / $2.000000')).toBeInTheDocument()

    const off = screen.getByRole('row', { name: /off-key/ })
    expect(within(off).getByText('Disabled')).toBeInTheDocument()
  })

  it('keeps the enable toggle as an action, not as a status claim', async () => {
    renderPage()
    const expired = await screen.findByRole('row', { name: /expired-key/ })
    expect(within(expired).getByRole('button', { name: 'Disable expired-key' })).toBeInTheDocument()
    expect(within(screen.getByRole('row', { name: /off-key/ })).getByRole('button', { name: 'Enable off-key' })).toBeInTheDocument()
  })

  it('shows — rather than a plausible number when spend or expiry is unreported', async () => {
    renderPage()
    const legacy = await screen.findByRole('row', { name: /legacy-key/ })
    expect(within(legacy).queryByText('Expired')).not.toBeInTheDocument()
    expect(within(legacy).queryByText('Out of budget')).not.toBeInTheDocument()
    expect(within(legacy).getByText('— / —')).toBeInTheDocument()
    expect(within(legacy).getByText('Usable')).toBeInTheDocument()
  })

  it('offers exactly the four API-key types accepted by the backend', async () => {
    renderPage()
    const dialog = await openCreate()

    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Key type' }))
    expect((await screen.findAllByRole('option')).map((option) => option.textContent)).toEqual([
      'User key',
      'Service key',
      'Personal key',
      'No-auth key',
    ])
  })

  it.each([
    ['User key', 'user', true],
    ['Service key', 'service', false],
    ['Personal key', 'personal', true],
    ['No-auth key', 'no_auth', false],
  ])('submits the %s type and its applicable owner', async (label, keyType, needsOwner) => {
    const { fetchMock } = renderPage()
    const dialog = await openCreate()
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Key name' }), `${keyType} test`)
    await choose(dialog, 'Key type', label)
    if (needsOwner) await choose(dialog, 'Owner', 'Project Owner (owner@example.com)')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(sent(fetchMock)).toHaveLength(1))
    const body = JSON.parse(String((sent(fetchMock)[0][1] as RequestInit).body))
    expect(body.key_type).toBe(keyType)
    expect(body.user_id).toBe(needsOwner ? 'u1' : null)
  })

  it('requires an owner for user keys', async () => {
    const { fetchMock } = renderPage()
    const dialog = await openCreate()
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Key name' }), 'owned key')
    await choose(dialog, 'Key type', 'User key')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('Select an active project member.')
    expect(sent(fetchMock)).toHaveLength(0)
  })

  it('clears a selected owner when the key type changes', async () => {
    renderPage()
    const dialog = await openCreate()
    await choose(dialog, 'Key type', 'User key')
    await choose(dialog, 'Owner', 'Project Owner (owner@example.com)')
    await choose(dialog, 'Key type', 'Service key')
    expect(within(dialog).queryByRole('combobox', { name: 'Owner' })).not.toBeInTheDocument()
    await choose(dialog, 'Key type', 'User key')
    expect(within(dialog).getByRole('combobox', { name: 'Owner' })).toHaveValue('')
  })

  it('clears the owner and blocks a stale dialog after a project switch', async () => {
    renderPage(apiMock({ projects: [project, otherProject] }))
    const dialog = await openCreate()
    await choose(dialog, 'Key type', 'Personal key')
    await choose(dialog, 'Owner', 'Project Owner (owner@example.com)')
    await userEvent.click(screen.getByRole('button', { name: 'Project B' }))

    expect(await within(dialog).findByRole('alert')).toHaveTextContent('The selected project changed. Close this dialog and create the key from the current project.')
    expect(within(dialog).getByRole('combobox', { name: 'Owner' })).toHaveValue('')
  })

  it('keeps an in-flight create bound to the project that submitted it', async () => {
    const post = deferred()
    const base = apiMock({ projects: [project, otherProject] })
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      if (init?.method === 'POST' && String(input).includes('/api-keys')) {
        return post.gate.then(() => new Response(JSON.stringify({ key: key({ id: 'created', name: 'in flight' }), token: 'pg_project_a_once', mode: 'generated' }), { status: 201, headers: { 'Content-Type': 'application/json' } }))
      }
      return base(input, init)
    })
    renderPage(fetchMock)
    const dialog = await openCreate()
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Key name' }), 'in flight')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(sent(fetchMock)).toHaveLength(1))

    await userEvent.click(screen.getByRole('button', { name: 'Project B' }))
    post.release()

    const secretDialog = await screen.findByRole('dialog', { name: 'API key created' })
    expect(within(secretDialog).getByText('pg_project_a_once')).toBeInTheDocument()
    expect(sent(fetchMock)[0][0]).toBe('/api/admin/v1/projects/p1/api-keys')
    expect(sent(fetchMock).some(([path]) => String(path).includes('/projects/p2/'))).toBe(false)
  })

  it('submits selected project scopes and displays returned scopes in the table', async () => {
    const { fetchMock } = renderPage()
    const usable = await screen.findByRole('row', { name: /usable-key/ })
    expect(within(usable).getByText('gateway:use')).toBeInTheDocument()
    expect(within(usable).getByText('project:read')).toBeInTheDocument()

    const dialog = await openCreate()
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Key name' }), 'scoped key')
    await userEvent.click(within(dialog).getByRole('checkbox', { name: 'Use gateway APIs' }))
    await userEvent.click(within(dialog).getByRole('checkbox', { name: 'Read project resources' }))
    await userEvent.click(within(dialog).getByRole('checkbox', { name: 'Manage API keys' }))
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(sent(fetchMock)).toHaveLength(1))
    expect(JSON.parse(String((sent(fetchMock)[0][1] as RequestInit).body)).scopes).toEqual(['project:read', 'api_key:manage'])
  })

  it('requires at least one project scope before submission', async () => {
    const { fetchMock } = renderPage()
    const dialog = await openCreate()
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Key name' }), 'unscoped key')
    await userEvent.click(within(dialog).getByRole('checkbox', { name: 'Use gateway APIs' }))
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    expect(await within(dialog).findByRole('alert')).toHaveTextContent('Select at least one project scope.')
    expect(sent(fetchMock)).toHaveLength(0)
  })

  it('keeps an exact server rejection and the entered choices inline', async () => {
    const fetchMock = apiMock({ postError: 'unknown project scope `catalog:manage`' })
    renderPage(fetchMock)
    const dialog = await openCreate()
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Key name' }), 'rejected key')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    expect(await within(dialog).findByRole('alert')).toHaveTextContent('unknown project scope `catalog:manage`')
    expect(within(dialog).getByRole('textbox', { name: 'Key name' })).toHaveValue('rejected key')
    expect(within(dialog).getByRole('checkbox', { name: 'Use gateway APIs' })).toBeChecked()
  })
})
