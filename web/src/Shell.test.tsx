import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
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

const renderShell = (permissions: string[], path = '/', displayedBranding = branding) => {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(permissions)
    return json({ data: [], total: 0 })
  }))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <ThemeProvider>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route element={<Shell user={user} branding={displayedBranding} />}>
            <Route path="/" element={<div>Overview body</div>} />
            <Route path="/operations/requests/:id" element={<div>Request detail</div>} />
          </Route>
        </Routes>
      </MemoryRouter>
      </ThemeProvider>
    </QueryClientProvider>,
  )
}

describe('sidebar gating', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); localStorage.clear() })

  it('offers the Models link to a principal holding only catalog:manage', async () => {
    renderShell(['catalog:manage'])
    expect(await screen.findByRole('link', { name: 'Models & routes' })).toHaveAttribute('href', '/models')
    // System settings are reachable with the catalog grant; the channels,
    // prompts and access surfaces are not.
    expect(screen.getByRole('link', { name: 'System settings' })).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Channels' })).not.toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Access control' })).not.toBeInTheDocument()
  })

  it('offers the Access link to a principal holding only project:read', async () => {
    renderShell(['project:read'])
    expect(await screen.findByRole('link', { name: 'Access control' })).toHaveAttribute('href', '/access')
    // Reading a project is not a catalog or channel grant.
    expect(screen.queryByRole('link', { name: 'Models & routes' })).not.toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Channels' })).not.toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'System settings' })).not.toBeInTheDocument()
  })

  it('keeps the administration group hidden from a principal with no grant', async () => {
    renderShell([])
    expect(await screen.findByRole('link', { name: 'Overview' })).toBeInTheDocument()
    expect(screen.queryByText('Administration')).not.toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Access control' })).not.toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Models & routes' })).not.toBeInTheDocument()
  })

  it('offers direct links to operational views for an administrator', async () => {
    renderShell(['*'])
    await userEvent.click(await screen.findByRole('button', { name: 'Show Operations subpages' }))
    expect(screen.getByRole('link', { name: 'Traces' })).toHaveAttribute('href', '/operations?tab=traces')
    await userEvent.click(screen.getByRole('button', { name: 'Show Access control subpages' }))
    expect(await screen.findByRole('link', { name: 'Projects' })).toHaveAttribute('href', '/access?tab=projects')
    expect(await screen.findByRole('link', { name: 'API keys' })).toHaveAttribute('href', '/access?tab=keys')
  })

  it('keeps the operations section active on a request detail', async () => {
    renderShell(['*'], '/operations/requests/r1')
    expect(await screen.findByRole('link', { name: 'Operations' })).toHaveAttribute('data-active', 'true')
    expect(screen.getByRole('button', { name: 'Hide Operations subpages' })).toHaveAttribute('aria-expanded', 'true')
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
