import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
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
const invitation = { id: 'i1', email: 'invited@example.test', role_id: 'r1', expires_at: 4_102_444_800, accepted_at: null, max_uses: 2, use_count: 1 }

const mockApi = (options: { invitations?: unknown[]; rolesFail?: boolean } = {}) => {
  const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['*'])
    if (path.includes('/invitations')) return init?.method === 'POST'
      ? json({ invitation: { ...invitation, max_uses: 2, use_count: 0 }, token: 'reusable-token' })
      : json(options.invitations ?? [])
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
    expect(await screen.findByText('No invitations yet.')).toBeInTheDocument()
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
    expect(screen.getByText('1 / 2')).toBeInTheDocument()
    expect(screen.getByText('Available')).toBeInTheDocument()
    expect(screen.queryByText('No invitations yet.')).not.toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Invite user' })).toHaveLength(1)
  })

  it('does not present an expired invitation as available', async () => {
    mockApi({ invitations: [{ ...invitation, expires_at: 1 }] })
    await renderPage()
    expect(await screen.findByText('Expired')).toBeInTheDocument()
    expect(screen.queryByText('Available')).not.toBeInTheDocument()
  })

  it('creates an email-bound invitation with a bounded use count', async () => {
    const fetchMock = mockApi({ invitations: [invitation] })
    await renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Invite user' }))

    await userEvent.type(await screen.findByLabelText(/^Email/), 'second@example.test')
    const maxUses = screen.getByLabelText(/^Maximum uses/)
    await userEvent.clear(maxUses)
    await userEvent.type(maxUses, '2')
    await userEvent.click(within(screen.getByRole('dialog', { name: 'Invite user' })).getByRole('button', { name: 'Invite user' }))

    await vi.waitFor(() => {
      const request = fetchMock.mock.calls.find(([path, init]) => String(path).includes('/invitations') && init?.method === 'POST')
      expect(JSON.parse(String(request?.[1]?.body))).toMatchObject({
        email: 'second@example.test',
        role_id: 'r1',
        max_uses: 2,
      })
    })
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
