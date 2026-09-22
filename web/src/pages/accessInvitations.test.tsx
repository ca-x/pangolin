import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import AccessPage from './AccessPage'

/**
 * The invitations tab rendered a bare table header when a project had none, and
 * its role picker failed silently. Both are console-wide rules in AGENTS.md: a
 * list carries a useful empty state, and every async query has an explicit
 * error with a retry.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const invitation = { id: 'i1', email: 'invited@example.test', role_id: 'r1', expires_at: 1, accepted_at: null }

const mockApi = (options: { invitations?: unknown[]; rolesFail?: boolean } = {}) => {
  const fetchMock = vi.fn((input: RequestInfo | URL) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['*'])
    if (path.includes('/invitations')) return json(options.invitations ?? [])
    if (path.includes('/roles')) return options.rolesFail ? Promise.reject(new Error('roles unavailable')) : json([{ id: 'r1', name: 'Member' }])
    return json([])
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}
const renderPage = async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
  await userEvent.click(await screen.findByRole('tab', { name: 'Invitations' }))
}

describe('the invitations list', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('offers one useful action when the project has no invitations', async () => {
    mockApi()
    await renderPage()
    expect(await screen.findByText('No pending invitations.')).toBeInTheDocument()
    // The empty state owns the single call to action; the header drops its copy.
    const actions = screen.getAllByRole('button', { name: 'Invite user' })
    expect(actions).toHaveLength(1)
    await userEvent.click(actions[0])
    expect(await screen.findByLabelText(/^Email/)).toBeInTheDocument()
  })

  it('renders the invitations it has, with the header action', async () => {
    mockApi({ invitations: [invitation] })
    await renderPage()
    expect(await screen.findByText('invited@example.test')).toBeInTheDocument()
    expect(screen.queryByText('No pending invitations.')).not.toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Invite user' })).toHaveLength(1)
  })

  it('reports a failed role lookup in the invite dialog and retries it', async () => {
    const fetchMock = mockApi({ invitations: [invitation], rolesFail: true })
    await renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Invite user' }))
    expect(await screen.findByText('The project roles could not be read.')).toBeInTheDocument()
    const before = fetchMock.mock.calls.filter(([path]) => String(path).includes('/roles')).length
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await vi.waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('/roles')).length).toBeGreaterThan(before))
  })

  it('reports a failed key-profile lookup instead of offering only "no profile"', async () => {
    const fetchMock = mockApi()
    vi.mocked(fetch).mockImplementation((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('key-profiles')) return Promise.reject(new Error('profiles unavailable'))
      return json([])
    })
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
    await userEvent.click(await screen.findByRole('button', { name: 'Create API key' }))

    expect(await screen.findByText('The option list could not be loaded.')).toBeInTheDocument()
    expect(screen.queryByRole('combobox', { name: 'Profile ID' })).not.toBeInTheDocument()
    const before = fetchMock.mock.calls.filter(([path]) => String(path).includes('key-profiles')).length
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await vi.waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('key-profiles')).length).toBeGreaterThan(before))
  })
})
