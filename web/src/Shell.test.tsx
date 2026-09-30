import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from './i18n'
import Shell from './Shell'
import { ThemeProvider } from './theme'

/**
 * The sidebar is the only place the console tells a principal what it may do, so
 * a link it hides is a capability the operator cannot reach. Two grants are
 * deliberately reachable from surfaces the permission name does not suggest:
 * catalog subscriptions live on the Models page (`catalog:manage`), and the
 * project members tab is granted by `project:read`. A principal holding only one
 * of them must still find the link.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const user = { id: 'u1', email: 'principal@example.test', role: 'admin', language: 'en', theme: 'system:bronze', created_at: 1 }
const branding = { instance_name: 'Pangolin', branding_name: 'Pangolin', favicon_url: '/logo.webp', onboarding_complete: true }

const renderShell = (permissions: string[], path = '/', displayedBranding = branding, availableProjects = [project]) => {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json(availableProjects)
    if (path.includes('/permissions')) return json(permissions)
    return json({ data: [], total: 0 })
  }))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <ThemeProvider>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route element={<Shell user={user} branding={displayedBranding} />}>
            <Route path="/" element={<div>Overview body</div>} />
            <Route path="/access" element={<div>Access body</div>} />
            <Route path="/system" element={<div>System body</div>} />
            <Route path="/operations/requests/:id" element={<div>Request detail</div>} />
            <Route path="/operations/traces/:id" element={<div>Trace detail</div>} />
          </Route>
        </Routes>
      </MemoryRouter>
      </ThemeProvider>
    </QueryClientProvider>,
  )
  return { ...view, client }
}

describe('sidebar gating', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })

  it('offers the Models link to a catalog manager who can read a project', async () => {
    renderShell(['project:read', 'catalog:manage'])
    expect(await screen.findByRole('link', { name: 'Model catalog' })).toHaveAttribute('href', '/models?tab=catalog')
    // System settings live in the instance scope. Catalog managers with a
    // readable project can reach them without project management controls.
    await userEvent.click(screen.getByRole('button', { name: 'Instance management' }))
    expect(screen.getByRole('link', { name: 'System settings' })).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Channels' })).not.toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'API keys' })).not.toBeInTheDocument()
  })

  it('offers project members directly to a principal holding only project:read', async () => {
    renderShell(['project:read'])
    expect(await screen.findByRole('link', { name: 'Members' })).toHaveAttribute('href', '/access?tab=members')
    // Reading a project is not a catalog or channel grant.
    expect(screen.queryByRole('link', { name: 'Model catalog' })).not.toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Channels' })).not.toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'System settings' })).not.toBeInTheDocument()
  })

  it('keeps the administration group hidden from a principal with no grant', async () => {
    renderShell([])
    expect(await screen.findByRole('link', { name: 'Overview' })).toBeInTheDocument()
    expect(screen.queryByText('Instance management')).not.toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'API keys' })).not.toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Model catalog' })).not.toBeInTheDocument()
  })

  it('puts the project workflow and instance resources in one scoped navigation', async () => {
    renderShell(['*'])
    expect(await screen.findByText('Current project')).toBeInTheDocument()
    expect(await screen.findByRole('button', { name: 'Instance management' })).toHaveAttribute('aria-expanded', 'false')
    await userEvent.click(screen.getByRole('button', { name: 'Observe' }))
    expect(screen.getByRole('link', { name: 'Traces' })).toHaveAttribute('href', '/operations?tab=traces')
    await userEvent.click(screen.getByRole('button', { name: 'Instance management' }))
    expect(screen.getByRole('link', { name: 'Projects' })).toHaveAttribute('href', '/access?tab=projects')
    await userEvent.click(screen.getByRole('button', { name: 'Current project' }))
    expect(screen.getByRole('link', { name: 'API keys' })).toHaveAttribute('href', '/access?tab=keys')
    await userEvent.click(screen.getByRole('button', { name: 'Show Channels subpages' }))
    expect(screen.getByRole('link', { name: 'Credentials' })).toHaveAttribute('href', '/channels?tab=credentials')
    await userEvent.click(screen.getByRole('button', { name: 'Show Models & routes subpages' }))
    expect(screen.getByRole('link', { name: 'Model routing' })).toHaveAttribute('href', '/models?tab=routing')
  })

  it('keeps the matching operational link active on detail pages', async () => {
    renderShell(['*'], '/operations/requests/r1')
    expect(await screen.findByRole('link', { name: 'Requests' })).toHaveAttribute('data-active', 'true')
    expect(screen.getByRole('button', { name: 'Observe' })).toHaveAttribute('aria-expanded', 'true')
  })

  it('opens the instance scope for a direct project management link', async () => {
    renderShell(['*'], '/access?tab=projects')
    expect(await screen.findByRole('link', { name: 'Projects' })).toHaveAttribute('data-active', 'true')
    expect(screen.getByRole('button', { name: 'Instance management' })).toHaveAttribute('aria-expanded', 'true')
  })

  it('keeps system settings selected on a nested settings tab', async () => {
    renderShell(['*'], '/system?tab=storage')
    expect(await screen.findByRole('link', { name: 'System settings' })).toHaveAttribute('data-active', 'true')
    expect(screen.getByRole('button', { name: 'Instance management' })).toHaveAttribute('aria-expanded', 'true')
  })

  it('hides cached management grants when the permission refresh fails', async () => {
    const shell = renderShell(['*'])
    expect(await screen.findByRole('link', { name: 'API keys' })).toBeInTheDocument()
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => String(input).includes('/permissions')
      ? Promise.resolve(new Response(JSON.stringify({ error: { type: 'unavailable', message: 'Unavailable' } }), { status: 503, headers: { 'Content-Type': 'application/json' } }))
      : json([project])))
    await shell.client.invalidateQueries({ queryKey: ['project-permissions', project.id] })

    await waitFor(() => expect(screen.queryByRole('link', { name: 'API keys' })).not.toBeInTheDocument())
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Instance management' })).not.toBeInTheDocument()
  })

  it('keeps the selected project beside the desktop management navigation', async () => {
    const another = { ...project, id: 'p2', name: 'Project B', slug: 'project-b', is_default: false }
    renderShell(['*'], '/', branding, [project, another])
    const picker = await screen.findByRole('group', { name: 'Project' })
    await userEvent.click(within(picker).getByRole('combobox', { name: 'Project' }))
    await userEvent.click(screen.getByRole('option', { name: 'Project B' }))
    expect(localStorage.getItem('pangolin-project')).toBe('p2')
  })

  it('offers the instance setup review only to an owner who can finish it', async () => {
    const member = renderShell(['project:read'], '/', { ...branding, onboarding_complete: false })
    expect(await screen.findByRole('link', { name: 'Overview' })).toBeInTheDocument()
    expect(screen.queryByText('Review initial setup')).not.toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Channels' })).not.toBeInTheDocument()
    member.unmount()
    renderShell(['*'], '/', { ...branding, onboarding_complete: false })
    expect(await screen.findByText('Review initial setup')).toBeInTheDocument()
  })
})
