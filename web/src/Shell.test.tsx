import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from './i18n'
import Shell from './Shell'

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

const renderShell = (permissions: string[]) => {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(permissions)
    return json({ data: [], total: 0 })
  }))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/']}>
        <Routes>
          <Route element={<Shell user={user} branding={branding} />}>
            <Route path="/" element={<div>Overview body</div>} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('sidebar gating', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

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
})
