import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ConfirmHost } from '../components'
import { ProjectProvider, useProject } from '../project'
import AccessPage from './AccessPage'

const response = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))

describe('API key administration', () => {
  beforeEach(() => { void i18n.changeLanguage('en'); vi.stubGlobal('fetch', vi.fn((input:RequestInfo|URL) => { const path=String(input); const data=path.endsWith('/projects')||path.includes('/projects?')?[{id:'project-a',name:'Project A',slug:'project-a',owner_user_id:'owner',is_default:true,enabled:true}]:path.endsWith('/api/admin/v1/permissions')?['*']:path.includes('/permissions')?['project:read','project:manage','api_key:manage','role:manage']:[]; return Promise.resolve(new Response(JSON.stringify(data), { status: 200, headers: { 'Content-Type': 'application/json' } })) })) })
  it('explains generated and one-time imported token modes', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage/></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('button', { name: 'Create API key' }))
    expect(screen.getByText(/shown once/i)).toBeInTheDocument()
    await userEvent.click(screen.getByRole('combobox', { name: 'Token mode' }))
    await userEvent.click(screen.getByRole('option', { name: 'Import existing' }))
    expect(screen.getByText(/stores only an indexed digest and Argon2id/i)).toBeInTheDocument()
    expect(screen.getByLabelText(/Existing token/)).toHaveAttribute('autocomplete', 'off')
  })

  it('uses the selected project and the real PATCH endpoint for project updates', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage/></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('tab', { name: 'Projects' }))
    expect(await screen.findByRole('button', { name: 'Add project' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Delete Project A' })).not.toBeInTheDocument()
    await userEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0])
    const name=screen.getByLabelText(/Name/);await userEvent.clear(name);await userEvent.type(name,'Renamed project')
    await userEvent.click(screen.getByRole('button',{name:'Save'}))
    await vi.waitFor(()=>expect(vi.mocked(fetch).mock.calls.some(([path,init])=>String(path).endsWith('/projects/project-a')&&init?.method==='PATCH'&&new Headers(init.headers).get('X-Pangolin-CSRF')==='1')).toBe(true))
  })
  it('allows project edits without offering instance-level project creation', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/api/admin/v1/permissions')) return response(['project:read'])
      if (path.includes('/permissions')) return response(['project:manage'])
      if (path.endsWith('/projects') || path.includes('/projects?')) return response([
        { id: 'project-a', name: 'Project A', slug: 'project-a', owner_user_id: 'owner', is_default: true, enabled: true },
        { id: 'project-b', name: 'Project B', slug: 'project-b', owner_user_id: 'other', is_default: false, enabled: true },
      ])
      return response([])
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('tab', { name: 'Projects' }))
    expect((await screen.findAllByText('Project A')).length).toBeGreaterThan(0)
    expect(screen.queryByRole('button', { name: 'Add project' })).not.toBeInTheDocument()
    expect(screen.getByText(/Only instance owners can create projects/)).toBeInTheDocument()
    const rows = within(screen.getByRole('region', { name: 'Projects' })).getAllByRole('row')
    const own = rows.find((row) => row.textContent?.includes('Project A'))!
    const foreign = rows.find((row) => row.textContent?.includes('Project B'))!
    expect(within(own).getByRole('button', { name: 'Edit' })).toBeInTheDocument()
    expect(within(foreign).queryByRole('button', { name: 'Edit' })).not.toBeInTheDocument()
    expect(within(foreign).queryByRole('button', { name: /Delete/ })).not.toBeInTheDocument()
  })
  it('does not offer owner-granting creation to a global project manager', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/api/admin/v1/permissions')) return response(['project:manage'])
      if (path.includes('/permissions')) return response(['project:manage'])
      if (path.endsWith('/projects') || path.includes('/projects?')) return response([{ id: 'project-a', name: 'Project A', slug: 'project-a', owner_user_id: 'owner', is_default: true, enabled: true }])
      return response([])
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('tab', { name: 'Projects' }))
    expect((await screen.findAllByText('Project A')).length).toBeGreaterThan(0)
    expect(screen.queryByRole('button', { name: 'Add project' })).not.toBeInTheDocument()
    expect(screen.getByText(/Only instance owners can create projects/)).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Edit' }).length).toBeGreaterThan(0)
  })
  it('only shows access tabs the principal is authorized to use', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      const data = path.endsWith('/projects') || path.includes('/projects?')
        ? [{ id: 'project-a', name: 'Project A', slug: 'project-a', owner_user_id: 'owner', is_default: true, enabled: true }]
        : path.includes('/permissions') ? ['project:read', 'api_key:manage'] : []
      return Promise.resolve(new Response(JSON.stringify(data), { status: 200, headers: { 'Content-Type': 'application/json' } }))
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage/></ProjectProvider></MemoryRouter></QueryClientProvider>)
    expect(await screen.findByRole('tab', { name: 'API keys' })).toBeInTheDocument()
    for (const hidden of ['Users', 'Roles', 'OIDC', 'Invitations']) {
      expect(screen.queryByRole('tab', { name: hidden })).not.toBeInTheDocument()
    }
  })
})

describe('OIDC identity administration', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })

  const project = { id: 'project-a', name: 'Project A', slug: 'project-a', owner_user_id: 'owner', is_default: true, enabled: true }
  const otherProject = { id: 'project-b', name: 'Project B', slug: 'project-b', owner_user_id: 'owner-b', is_default: false, enabled: true }
  const providers = [{ id: 'provider-a', name: 'Workforce' }, { id: 'provider-b', name: 'Partners' }]
  const identity = { id: 'identity-a', provider_id: 'provider-a', user_id: 'user-opaque', subject: 'subject-opaque', claims_json: '{}', last_login_at: null, created_at: 1_700_000_000 }
  const ProjectSwitch = () => {
    const { setProjectId } = useProject()
    return <button type="button" onClick={() => setProjectId(otherProject.id)}>Switch to Project B</button>
  }
  const renderIdentities = (withSwitch = false) => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage />{withSwitch && <ProjectSwitch />}<ConfirmHost /></ProjectProvider></MemoryRouter></QueryClientProvider>)
  }

  it('lists subject and bound user, then unlinks only after confirmation', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return response([project])
      if (path.includes('/permissions')) return response(['oidc:manage'])
      if (path.endsWith('/oidc/providers')) return response(providers)
      if (path.endsWith('/provider-a/identities')) return response([identity])
      if (init?.method === 'DELETE') return Promise.resolve(new Response(null, { status: 204 }))
      return response([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderIdentities()
    await userEvent.click(await screen.findByRole('tab', { name: 'OIDC identities' }))

    expect(await screen.findByText('subject-opaque')).toBeInTheDocument()
    expect(screen.getByText('user-opaque')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: /Unlink identity subject-opaque/ }))
    expect(fetchMock.mock.calls.some(([, init]) => init?.method === 'DELETE')).toBe(false)
    const dialog = await screen.findByRole('dialog')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Unlink identity' }))
    await waitFor(() => expect(fetchMock.mock.calls.some(([path, init]) => String(path).endsWith('/provider-a/identities/identity-a') && init?.method === 'DELETE')).toBe(true))
  })

  it('shows retry and a useful empty state after the provider list recovers', async () => {
    let providerReads = 0
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return response([project])
      if (path.includes('/permissions')) return response(['oidc:manage'])
      if (path.endsWith('/oidc/providers')) {
        providerReads += 1
        return providerReads === 1 ? response({ error: { message: 'unavailable' } }, 503) : response(providers)
      }
      if (path.includes('/identities')) return response([])
      return response([])
    }))
    renderIdentities()
    await userEvent.click(await screen.findByRole('tab', { name: 'OIDC identities' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Retry' }))
    expect(await screen.findByText('No identities are linked to this provider.')).toBeInTheDocument()
  })

  it('refuses a confirmed unlink after the selected project changes', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, _init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return response([project, otherProject])
      if (path.includes('/permissions')) return response(['oidc:manage'])
      if (path.endsWith('/oidc/providers')) return response(providers)
      if (path.endsWith('/provider-a/identities')) return response([identity])
      if (path.endsWith('/provider-b/identities')) return response([])
      return response([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderIdentities(true)
    await userEvent.click(await screen.findByRole('tab', { name: 'OIDC identities' }))
    await screen.findByText('subject-opaque')
    const switchProject = screen.getByRole('button', { name: 'Switch to Project B' })
    await userEvent.click(screen.getByRole('button', { name: /Unlink identity subject-opaque/ }))
    fireEvent.click(switchProject)
    const dialog = await screen.findByRole('dialog')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Unlink identity' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('provider changed')
    expect(fetchMock.mock.calls.some(([, init]) => init?.method === 'DELETE')).toBe(false)
  })
})

describe('OIDC provider policy and branding administration', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })

  it('validates safe branding inline and submits login-only policy fields', async () => {
    const project = { id: 'project-a', name: 'Project A', slug: 'project-a', owner_user_id: 'owner', is_default: true, enabled: true }
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return response([project])
      if (path.includes('/permissions')) return response(['oidc:manage'])
      if (path.includes('/oidc/providers') && init?.method === 'POST') return response({ id: 'created' }, 201)
      if (path.includes('/oidc/providers')) return response([])
      return response([])
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('tab', { name: 'OIDC' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Add OIDC' }))

    expect(screen.getByRole('textbox', { name: 'Display name' })).toBeRequired()
    expect(screen.getByRole('switch', { name: /^Login only/ })).not.toBeChecked()
    await userEvent.type(screen.getByRole('textbox', { name: 'Button color' }), '#xyz')
    expect(await screen.findByText('Use a color in #RRGGBB format.')).toBeInTheDocument()
    await userEvent.type(screen.getByRole('textbox', { name: 'Logo key' }), 'https://example.test/logo.svg')
    expect(await screen.findByText('Use a bundled catalog key such as lobehub:Github.')).toBeInTheDocument()

    await userEvent.clear(screen.getByRole('textbox', { name: 'Button color' }))
    await userEvent.type(screen.getByRole('textbox', { name: 'Button color' }), '#0B5A46')
    await userEvent.clear(screen.getByRole('textbox', { name: 'Logo key' }))
    await userEvent.type(screen.getByRole('textbox', { name: 'Logo key' }), 'catalog:unknown')
    await userEvent.type(screen.getByRole('textbox', { name: 'Name' }), 'workforce')
    await userEvent.type(screen.getByRole('textbox', { name: 'Display name' }), 'Workforce SSO')
    await userEvent.type(screen.getByRole('textbox', { name: 'Issuer URL' }), 'https://id.example.test')
    await userEvent.type(screen.getByRole('textbox', { name: 'Client ID' }), 'client')
    await userEvent.type(screen.getByLabelText('Client secret'), 'secret-value')
    await userEvent.click(screen.getByRole('switch', { name: /^Login only/ }))
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([, init]) => {
      if (init?.method !== 'POST') return false
      const body = JSON.parse(String(init.body))
      return body.display_name === 'Workforce SSO'
        && body.button_color === '#0B5A46'
        && body.logo_key === 'catalog:unknown'
        && body.login_only === true
    })).toBe(true))
  })

  it('loads and updates existing provider policy and branding fields', async () => {
    const project = { id: 'project-a', name: 'Project A', slug: 'project-a', owner_user_id: 'owner', is_default: true, enabled: true }
    const provider = {
      id: 'provider-a', name: 'workforce', display_name: 'Workforce SSO', button_color: '#0B5A46', logo_key: 'catalog:unknown', login_only: true,
      issuer_url: 'https://id.example.test', client_id: 'client', scopes: '["openid"]', claim_mapping: '{"version":1}', enabled: true, secret_configured: true,
    }
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return response([project])
      if (path.includes('/permissions')) return response(['oidc:manage'])
      if (path.endsWith('/oidc/providers/provider-a') && init?.method === 'PATCH') return response({ ...provider, display_name: 'Workforce identity' })
      if (path.includes('/oidc/providers')) return response([provider])
      return response([])
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('tab', { name: 'OIDC' }))
    const table = await screen.findByRole('region', { name: 'OIDC' })
    fireEvent.click(within(table).getByRole('button', { name: 'Edit' }))
    const dialog = await screen.findByRole('dialog', { name: 'Edit OIDC' })

    const displayName = within(dialog).getByRole('textbox', { name: 'Display name' })
    expect(displayName).toHaveValue('Workforce SSO')
    expect(within(dialog).getByRole('textbox', { name: 'Button color' })).toHaveValue('#0B5A46')
    expect(within(dialog).getByRole('textbox', { name: 'Logo key' })).toHaveValue('catalog:unknown')
    expect(within(dialog).getByRole('switch', { name: /^Login only/ })).toBeChecked()
    await userEvent.clear(displayName)
    await userEvent.type(displayName, 'Workforce identity')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([path, init]) => {
      if (!String(path).endsWith('/oidc/providers/provider-a') || init?.method !== 'PATCH') return false
      return JSON.parse(String(init.body)).display_name === 'Workforce identity'
    })).toBe(true))
  })
})

describe('safe key client onboarding', () => {
  const project = { id: 'project-a', name: 'Project A', slug: 'project-a', enabled: true, is_default: true }
  const key = { id: 'key-a', name: 'Client key', key_prefix: 'pg_test', scopes: ['gateway:use'], enabled: true, key_type: 'service', expires_at: null, budget_micros: null, spent_micros: 0, last_used_at: null, profile_id: null, allowed_ips_json: '[]', denied_ips_json: '[]', lifecycle: 'active' }
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })
  function setup() {
    let keys: typeof key[] = []
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return response([project])
      if (path.includes('/permissions')) return response(['api_key:manage'])
      if (path.includes('/client-models?')) return response({ models: [{ id: 'public' }] })
      if (path.endsWith('/rotate') && init?.method === 'POST') return response({ key, token: 'rotated-secret' })
      if (path.endsWith('/api-keys')) {
        if (init?.method === 'POST') { keys = [key]; return response({ key, token: 'created-secret', mode: 'generated' }) }
        return response(keys)
      }
      return response({ data: [] })
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /><ConfirmHost /></ProjectProvider></MemoryRouter></QueryClientProvider>)
    return { client, fetchMock }
  }
  async function createKey() {
    await userEvent.click(await screen.findByRole('button', { name: 'Create API key' }))
    await userEvent.type(screen.getByLabelText(/Key name/), 'Client key')
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))
    await screen.findByText(/wire_api/)
  }
  it('shows a create token once, drops mutation token state, and reconnects using the environment', async () => {
    const { client, fetchMock } = setup()
    await createKey()
    expect(screen.getByText(/wire_api/)).toHaveTextContent('created-secret')
    await userEvent.click(screen.getByRole('button', { name: 'Close' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Connect Client key' }))
    expect(await screen.findByText(/wire_api/)).toHaveTextContent('env_key = "PANGOLIN_API_KEY"')
    expect(screen.queryByText(/created-secret/)).not.toBeInTheDocument()
    await waitFor(() => expect(JSON.stringify(client.getMutationCache().getAll().map((mutation) => mutation.state.data))).not.toContain('created-secret'))
    expect(fetchMock.mock.calls.some(([path]) => String(path).includes('/recovery'))).toBe(false)
    expect(JSON.stringify(Object.entries(localStorage))).not.toContain('created-secret')
  })
  it('offers the rotated token and setup while retaining one-time close semantics', async () => {
    setup()
    await createKey()
    await userEvent.click(screen.getByRole('button', { name: 'Close' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Rotate Client key' }))
    const confirmation = await screen.findByRole('dialog')
    await userEvent.click(within(confirmation).getByRole('button', { name: 'Rotate' }))
    expect(await screen.findByText(/wire_api/)).toHaveTextContent('rotated-secret')
    await userEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(screen.queryByText(/rotated-secret/)).not.toBeInTheDocument()
  })
})

describe('one-time key response lifecycle fences', () => {
  const projects = [
    { id: 'fence-a', name: 'Fence A', slug: 'a', enabled: true, is_default: true },
    { id: 'fence-b', name: 'Fence B', slug: 'b', enabled: true, is_default: false },
  ]
  const key = { id: 'fence-key', name: 'Fence key', key_prefix: 'pg_fence', scopes: ['gateway:use'], enabled: true, key_type: 'service', expires_at: null, budget_micros: null, spent_micros: 0, last_used_at: null, profile_id: null, allowed_ips_json: '[]', denied_ips_json: '[]', lifecycle: 'active' }
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })
  const Switch = () => {
    const { setProjectId } = useProject()
    return <>{projects.map((project) => <button key={project.id} onClick={() => setProjectId(project.id)}>{project.name}</button>)}</>
  }
  function harness() {
    const pending: Array<{ path: string; complete: (token?: string) => void }> = []
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (init?.method === 'POST' && (path.endsWith('/api-keys') || path.endsWith('/rotate'))) {
        return new Promise<Response>((resolve) => pending.push({ path, complete: (token = 'stale-sensitive-token') => resolve(new Response(JSON.stringify({ key, token, mode: 'generated' }), { status: 200, headers: { 'Content-Type': 'application/json' } })) }))
      }
      if (path.endsWith('/projects')) return response(projects)
      if (path.includes('/permissions')) return response(['api_key:manage'])
      if (path.includes('/client-models?')) return response({ models: [{ id: 'public' }] })
      if (path.endsWith('/api-keys')) return response([key])
      return response({ data: [] })
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const view = render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /><Switch /><ConfirmHost /></ProjectProvider></MemoryRouter></QueryClientProvider>)
    return { pending, client, view }
  }
  async function start(client: 'create' | 'rotate') {
    if (client === 'create') {
      await userEvent.click(await screen.findByRole('button', { name: 'Create API key' }))
      await userEvent.type(screen.getByLabelText(/Key name/), 'Delayed key')
      await userEvent.click(screen.getByRole('button', { name: 'Save' }))
    } else {
      await userEvent.click(await screen.findByRole('button', { name: 'Rotate Fence key' }))
      await userEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Rotate' }))
    }
  }
  async function settled(client: QueryClient) {
    await waitFor(() => expect(client.getMutationCache().getAll().some((mutation) => mutation.state.status === 'pending')).toBe(false))
    await waitFor(() => expect(JSON.stringify(client.getMutationCache().getAll().map((mutation) => mutation.state.data))).not.toContain('stale-sensitive-token'))
    expect(screen.queryByText(/stale-sensitive-token/)).not.toBeInTheDocument()
    expect(JSON.stringify(Object.entries(localStorage))).not.toContain('stale-sensitive-token')
  }
  it.each(['create', 'rotate'] as const)('rejects delayed %s token after A→B→A while preserving server attribution', async (operation) => {
    const { pending, client } = harness()
    await start(operation)
    await waitFor(() => expect(pending).toHaveLength(1))
    await userEvent.click(screen.getByRole('button', { name: 'Fence B' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Fence A' }))
    pending[0].complete()
    await settled(client)
    expect(pending[0].path).toContain('/projects/fence-a/api-keys')
    expect(screen.queryByRole('dialog', { name: /API key (created|rotated)/ })).not.toBeInTheDocument()
  })
  it.each(['Cancel', 'Close'])('rejects a delayed create token after an invalidating %s', async (name) => {
    const { pending, client } = harness()
    await start('create')
    await waitFor(() => expect(pending).toHaveLength(1))
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name }))
    pending[0].complete()
    await settled(client)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })
  it.each(['create', 'rotate'] as const)('disposes a pending %s response when the key panel closes', async (operation) => {
    const { pending, client } = harness()
    await start(operation)
    await waitFor(() => expect(pending).toHaveLength(1))
    await userEvent.click(screen.getByRole('tab', { name: 'Key profiles' }))
    await userEvent.click(screen.getByRole('tab', { name: 'API keys' }))
    pending[0].complete()
    await settled(client)
    expect(screen.queryByRole('dialog', { name: /API key (created|rotated)/ })).not.toBeInTheDocument()
  })
  it('keeps an earlier closed create from replacing a newer operation or its token', async () => {
    const { pending, client } = harness()
    await start('create')
    await waitFor(() => expect(pending).toHaveLength(1))
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Cancel' }))
    await start('create')
    await waitFor(() => expect(pending).toHaveLength(2))
    pending[0].complete()
    await waitFor(() => expect(client.getMutationCache().getAll().filter((mutation) => mutation.state.status === 'pending')).toHaveLength(1))
    expect(screen.queryByText(/stale-sensitive-token/)).not.toBeInTheDocument()
    expect(screen.getByRole('dialog', { name: 'Create API key' })).toBeInTheDocument()
    pending[1].complete('fresh-sensitive-token')
    expect(await screen.findByText(/wire_api/)).toHaveTextContent('fresh-sensitive-token')
    await userEvent.click(screen.getByRole('button', { name: 'Close' }))
    await settled(client)
    expect(JSON.stringify(client.getMutationCache().getAll().map((mutation) => mutation.state.data))).not.toContain('fresh-sensitive-token')
  })
})
