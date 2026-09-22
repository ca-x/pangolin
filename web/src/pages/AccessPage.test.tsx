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
  beforeEach(() => { void i18n.changeLanguage('en'); vi.stubGlobal('fetch', vi.fn((input:RequestInfo|URL) => { const path=String(input); const data=path.endsWith('/projects')||path.includes('/projects?')?[{id:'project-a',name:'Project A',slug:'project-a',owner_user_id:'owner',is_default:true,enabled:true}]:path.includes('/permissions')?['project:read','project:manage','api_key:manage','role:manage']:[]; return Promise.resolve(new Response(JSON.stringify(data), { status: 200, headers: { 'Content-Type': 'application/json' } })) })) })
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
    await userEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0])
    const name=screen.getByLabelText(/Name/);await userEvent.clear(name);await userEvent.type(name,'Renamed project')
    await userEvent.click(screen.getByRole('button',{name:'Save'}))
    await vi.waitFor(()=>expect(vi.mocked(fetch).mock.calls.some(([path,init])=>String(path).endsWith('/projects/project-a')&&init?.method==='PATCH'&&new Headers(init.headers).get('X-Pangolin-CSRF')==='1')).toBe(true))
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
